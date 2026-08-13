use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use tokio::{sync::Semaphore, task::JoinHandle, time::sleep};
use tracing::{info, warn};

use crate::{http::AppState, model::Job, store::JobStore};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BackoffConfig {
    pub base_ms: u64,
    pub max_ms: u64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            base_ms: 500,
            max_ms: 10_000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunnerConfig {
    pub concurrency: usize,
    pub backoff: BackoffConfig,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        Self {
            concurrency: 4,
            backoff: BackoffConfig::default(),
        }
    }
}

pub fn spawn_runner(state: AppState) -> JoinHandle<()> {
    tokio::spawn(run_dispatcher(state))
}

pub async fn run_dispatcher(state: AppState) {
    let semaphore = Arc::new(Semaphore::new(state.config.concurrency.max(1)));

    loop {
        if let Some(job) = state.store.claim_next().await {
            let permit = semaphore
                .clone()
                .acquire_owned()
                .await
                .expect("worker semaphore should not close");
            let worker_state = state.clone();

            tokio::spawn(async move {
                let _permit = permit;
                process_claimed_job(worker_state, job).await;
            });

            continue;
        }

        tokio::select! {
            () = state.store.wait_for_work() => {},
            () = sleep(Duration::from_millis(250)) => {},
        }
    }
}

#[tracing::instrument(skip(state, job), fields(job_id = %job.id, kind = %job.kind, attempt = job.attempts))]
pub async fn process_claimed_job(state: AppState, job: Job) {
    match execute_job(&job).await {
        Ok(output) => {
            state.store.complete_success(&job.id, output).await;
            info!("job completed");
        }
        Err(error) => {
            let backoff_ms = backoff_ms(job.attempts, &state.config.backoff);
            state
                .store
                .complete_failure(&job.id, error.clone(), backoff_ms)
                .await;
            warn!(%error, backoff_ms, "job attempt failed");
        }
    }
}

pub fn backoff_ms(attempt: u32, config: &BackoffConfig) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    config
        .base_ms
        .saturating_mul(1_u64 << exponent)
        .min(config.max_ms)
}

async fn execute_job(job: &Job) -> Result<String, String> {
    let units = job
        .payload
        .get("work_units")
        .and_then(|value| value.as_u64())
        .unwrap_or_else(|| default_work_units(&job.kind))
        .clamp(1, 10);
    sleep(Duration::from_millis(40 + units * 35)).await;

    if job
        .payload
        .get("always_fail")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
    {
        return Err("payload requested deterministic permanent failure".to_string());
    }

    let fail_until_attempt = job
        .payload
        .get("fail_until_attempt")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);

    if u64::from(job.attempts) <= fail_until_attempt {
        return Err(format!(
            "deterministic transient failure through attempt {fail_until_attempt}"
        ));
    }

    Ok(format!(
        "simulated {} completed after {} work unit(s)",
        job.kind, units
    ))
}

fn default_work_units(kind: &str) -> u64 {
    match kind {
        "heartbeat" => 1,
        "webhook-delivery" => 2,
        "sensor-rollup" => 4,
        "cache-warm" => 3,
        "cleanup" => 2,
        _ => 2,
    }
}

#[allow(dead_code)]
pub async fn drain_ready_jobs_once(store: &JobStore, state: AppState) -> usize {
    let mut processed = 0;
    while let Some(job) = store.claim_next().await {
        process_claimed_job(state.clone(), job).await;
        processed += 1;
    }
    processed
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio::time::timeout;

    use super::*;
    use crate::{model::CreateJobRequest, model::JobStatus};

    #[test]
    fn backoff_is_deterministic_and_capped() {
        let config = BackoffConfig {
            base_ms: 250,
            max_ms: 1_000,
        };

        assert_eq!(backoff_ms(1, &config), 250);
        assert_eq!(backoff_ms(2, &config), 500);
        assert_eq!(backoff_ms(3, &config), 1_000);
        assert_eq!(backoff_ms(4, &config), 1_000);
    }

    #[tokio::test]
    async fn worker_retries_then_succeeds() {
        let store = JobStore::new();
        let config = RunnerConfig {
            concurrency: 1,
            backoff: BackoffConfig {
                base_ms: 10,
                max_ms: 10,
            },
        };
        let state = AppState::new(store.clone(), config);
        let response = store
            .enqueue(CreateJobRequest {
                kind: "sensor-rollup".to_string(),
                payload: json!({"fail_until_attempt": 1, "work_units": 1}),
                idempotency_key: None,
                max_attempts: Some(3),
            })
            .await
            .expect("enqueue should succeed");

        let first = store.claim_next().await.expect("job should be ready");
        process_claimed_job(state.clone(), first).await;
        let after_first = store.get(&response.job.id).await.expect("job exists");
        assert_eq!(after_first.status, JobStatus::Pending);
        assert_eq!(after_first.attempts, 1);

        timeout(Duration::from_millis(200), async {
            loop {
                if let Some(second) = store.claim_next().await {
                    process_claimed_job(state.clone(), second).await;
                    break;
                }
                sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("retry should become ready");

        let final_job = store.get(&response.job.id).await.expect("job exists");
        assert_eq!(final_job.status, JobStatus::Succeeded);
        assert_eq!(final_job.attempts, 2);
    }
}
