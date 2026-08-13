use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Notify, RwLock};

use crate::model::{
    CreateJobRequest, DEFAULT_MAX_ATTEMPTS, EnqueueResponse, HistoryRecord, Job, JobEvent,
    JobEventKind, JobStatus, MAX_ATTEMPTS_LIMIT, RunnerMetrics,
};

#[derive(Clone, Debug)]
pub struct JobStore {
    inner: Arc<StoreInner>,
}

#[derive(Debug)]
struct StoreInner {
    jobs: RwLock<StoreData>,
    next_id: AtomicU64,
    notify: Notify,
}

#[derive(Debug, Default)]
struct StoreData {
    jobs: BTreeMap<String, Job>,
    idempotency: HashMap<String, String>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum StoreError {
    InvalidKind,
    InvalidMaxAttempts(u32),
    InvalidIdempotencyKey,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::InvalidKind => write!(f, "job kind must not be empty"),
            StoreError::InvalidMaxAttempts(value) => {
                write!(
                    f,
                    "max_attempts must be between 1 and {MAX_ATTEMPTS_LIMIT}; got {value}"
                )
            }
            StoreError::InvalidIdempotencyKey => {
                write!(f, "idempotency_key must not be empty when provided")
            }
        }
    }
}

impl std::error::Error for StoreError {}

impl Default for JobStore {
    fn default() -> Self {
        Self::new()
    }
}

impl JobStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(StoreInner {
                jobs: RwLock::new(StoreData::default()),
                next_id: AtomicU64::new(1),
                notify: Notify::new(),
            }),
        }
    }

    pub async fn enqueue(&self, request: CreateJobRequest) -> Result<EnqueueResponse, StoreError> {
        let kind = request.kind.trim();
        if kind.is_empty() {
            return Err(StoreError::InvalidKind);
        }

        let idempotency_key = match request.idempotency_key.as_deref().map(str::trim) {
            Some("") => return Err(StoreError::InvalidIdempotencyKey),
            Some(key) => Some(key.to_string()),
            None => None,
        };

        let max_attempts = request.max_attempts.unwrap_or(DEFAULT_MAX_ATTEMPTS);
        if !(1..=MAX_ATTEMPTS_LIMIT).contains(&max_attempts) {
            return Err(StoreError::InvalidMaxAttempts(max_attempts));
        }

        let mut data = self.inner.jobs.write().await;
        if let Some(key) = &idempotency_key
            && let Some(existing_id) = data.idempotency.get(key)
            && let Some(job) = data.jobs.get(existing_id)
        {
            return Ok(EnqueueResponse {
                deduplicated: true,
                job: job.clone(),
            });
        }

        let now = now_ms();
        let id = format!(
            "job-{:06}",
            self.inner.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let job = Job {
            id: id.clone(),
            kind: kind.to_string(),
            payload: request.payload,
            idempotency_key: idempotency_key.clone(),
            status: JobStatus::Pending,
            attempts: 0,
            max_attempts,
            queued_at_ms: now,
            scheduled_for_ms: now,
            updated_at_ms: now,
            completed_at_ms: None,
            output: None,
            last_error: None,
            history: vec![JobEvent::new(now, JobEventKind::Queued, "job queued", 0)],
        };

        if let Some(key) = idempotency_key {
            data.idempotency.insert(key, id.clone());
        }

        data.jobs.insert(id, job.clone());
        drop(data);
        self.inner.notify.notify_one();

        Ok(EnqueueResponse {
            deduplicated: false,
            job,
        })
    }

    pub async fn claim_next(&self) -> Option<Job> {
        let mut data = self.inner.jobs.write().await;
        let now = now_ms();
        let next_id = data
            .jobs
            .values()
            .filter(|job| job.status == JobStatus::Pending && job.scheduled_for_ms <= now)
            .min_by_key(|job| (job.scheduled_for_ms, job.queued_at_ms, job.id.clone()))
            .map(|job| job.id.clone())?;

        let job = data.jobs.get_mut(&next_id)?;
        job.status = JobStatus::Running;
        job.attempts += 1;
        job.updated_at_ms = now;
        job.last_error = None;
        job.history.push(JobEvent::new(
            now,
            JobEventKind::Started,
            "job attempt started",
            job.attempts,
        ));

        Some(job.clone())
    }

    pub async fn complete_success(&self, id: &str, output: String) -> Option<Job> {
        let mut data = self.inner.jobs.write().await;
        let now = now_ms();
        let job = data.jobs.get_mut(id)?;

        if job.status != JobStatus::Running {
            return Some(job.clone());
        }

        job.status = JobStatus::Succeeded;
        job.updated_at_ms = now;
        job.completed_at_ms = Some(now);
        job.output = Some(output);
        job.last_error = None;
        job.history.push(JobEvent::new(
            now,
            JobEventKind::Succeeded,
            "job completed successfully",
            job.attempts,
        ));

        Some(job.clone())
    }

    pub async fn complete_failure(&self, id: &str, error: String, backoff_ms: u64) -> Option<Job> {
        let mut data = self.inner.jobs.write().await;
        let now = now_ms();
        let job = data.jobs.get_mut(id)?;

        if job.status != JobStatus::Running {
            return Some(job.clone());
        }

        job.updated_at_ms = now;
        job.last_error = Some(error.clone());

        if job.attempts >= job.max_attempts {
            job.status = JobStatus::Failed;
            job.completed_at_ms = Some(now);
            job.history.push(JobEvent::new(
                now,
                JobEventKind::Failed,
                format!("job failed permanently: {error}"),
                job.attempts,
            ));
        } else {
            job.status = JobStatus::Pending;
            job.scheduled_for_ms = now.saturating_add(backoff_ms);
            job.history.push(JobEvent::new(
                now,
                JobEventKind::RetryScheduled,
                format!("retry scheduled in {backoff_ms}ms: {error}"),
                job.attempts,
            ));
        }

        let updated = job.clone();
        drop(data);
        self.inner.notify.notify_one();
        Some(updated)
    }

    pub async fn cancel(&self, id: &str) -> Option<Job> {
        let mut data = self.inner.jobs.write().await;
        let now = now_ms();
        let job = data.jobs.get_mut(id)?;

        if job.status.is_terminal() {
            return Some(job.clone());
        }

        job.status = JobStatus::Canceled;
        job.updated_at_ms = now;
        job.completed_at_ms = Some(now);
        job.history.push(JobEvent::new(
            now,
            JobEventKind::Canceled,
            "job canceled by operator",
            job.attempts,
        ));

        Some(job.clone())
    }

    pub async fn get(&self, id: &str) -> Option<Job> {
        self.inner.jobs.read().await.jobs.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<Job> {
        self.inner
            .jobs
            .read()
            .await
            .jobs
            .values()
            .cloned()
            .collect()
    }

    pub async fn history(&self) -> Vec<HistoryRecord> {
        self.inner
            .jobs
            .read()
            .await
            .jobs
            .values()
            .flat_map(|job| {
                job.history.iter().cloned().map(|event| HistoryRecord {
                    job_id: job.id.clone(),
                    status: job.status.clone(),
                    event,
                })
            })
            .collect()
    }

    pub async fn metrics(&self) -> RunnerMetrics {
        let data = self.inner.jobs.read().await;
        let mut metrics = RunnerMetrics {
            total_jobs: data.jobs.len(),
            ..RunnerMetrics::default()
        };
        let mut finished_attempts = 0_u64;
        let mut finished_count = 0_u64;

        for job in data.jobs.values() {
            match job.status {
                JobStatus::Pending => metrics.pending += 1,
                JobStatus::Running => metrics.running += 1,
                JobStatus::Succeeded => metrics.succeeded += 1,
                JobStatus::Failed => metrics.failed += 1,
                JobStatus::Canceled => metrics.canceled += 1,
            }

            metrics.retry_scheduled_events += job
                .history
                .iter()
                .filter(|event| event.kind == JobEventKind::RetryScheduled)
                .count();

            if job.status.is_terminal() {
                finished_attempts += u64::from(job.attempts);
                finished_count += 1;
            }
        }

        if finished_count > 0 {
            metrics.average_attempts_finished = finished_attempts as f64 / finished_count as f64;
        }

        metrics
    }

    pub async fn wait_for_work(&self) {
        self.inner.notify.notified().await;
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn idempotency_key_returns_existing_job() {
        let store = JobStore::new();
        let first = store
            .enqueue(CreateJobRequest {
                kind: "heartbeat".to_string(),
                payload: json!({"edge": "alpha"}),
                idempotency_key: Some("idem-alpha".to_string()),
                max_attempts: Some(3),
            })
            .await
            .expect("first enqueue should succeed");

        let second = store
            .enqueue(CreateJobRequest {
                kind: "heartbeat".to_string(),
                payload: json!({"edge": "changed"}),
                idempotency_key: Some("idem-alpha".to_string()),
                max_attempts: Some(3),
            })
            .await
            .expect("second enqueue should dedupe");

        assert!(!first.deduplicated);
        assert!(second.deduplicated);
        assert_eq!(first.job.id, second.job.id);
        assert_eq!(store.list().await.len(), 1);
    }

    #[tokio::test]
    async fn invalid_max_attempts_is_rejected() {
        let store = JobStore::new();
        let err = store
            .enqueue(CreateJobRequest {
                kind: "heartbeat".to_string(),
                payload: json!({}),
                idempotency_key: None,
                max_attempts: Some(MAX_ATTEMPTS_LIMIT + 1),
            })
            .await
            .expect_err("large max_attempts should fail");

        assert_eq!(err, StoreError::InvalidMaxAttempts(MAX_ATTEMPTS_LIMIT + 1));
    }
}
