use serde_json::json;

use crate::{
    model::CreateJobRequest,
    store::{JobStore, StoreError},
};

pub async fn seed_demo_jobs(store: &JobStore) -> Result<(), StoreError> {
    let jobs = [
        CreateJobRequest {
            kind: "heartbeat".to_string(),
            payload: json!({"edge": "alpha-01", "work_units": 1}),
            idempotency_key: Some("demo-heartbeat-alpha-01".to_string()),
            max_attempts: Some(3),
        },
        CreateJobRequest {
            kind: "sensor-rollup".to_string(),
            payload: json!({"region": "iad", "window": "5m", "fail_until_attempt": 1, "work_units": 3}),
            idempotency_key: Some("demo-rollup-iad-5m".to_string()),
            max_attempts: Some(4),
        },
        CreateJobRequest {
            kind: "webhook-delivery".to_string(),
            payload: json!({"endpoint": "https://example.invalid/pulse", "fail_until_attempt": 2, "work_units": 2}),
            idempotency_key: Some("demo-webhook-pulse".to_string()),
            max_attempts: Some(5),
        },
        CreateJobRequest {
            kind: "cache-warm".to_string(),
            payload: json!({"segment": "operator-dashboard", "work_units": 2}),
            idempotency_key: Some("demo-cache-dashboard".to_string()),
            max_attempts: Some(3),
        },
        CreateJobRequest {
            kind: "cleanup".to_string(),
            payload: json!({"partition": "expired-history", "always_fail": true, "work_units": 1}),
            idempotency_key: Some("demo-cleanup-expired-history".to_string()),
            max_attempts: Some(2),
        },
    ];

    for job in jobs {
        store.enqueue(job).await?;
    }

    Ok(())
}
