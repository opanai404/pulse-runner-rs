use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use pulse_runner_rs::{AppState, JobStore, RunnerConfig, build_router, model::Job};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn create_job_api_deduplicates_by_key() {
    let store = JobStore::new();
    let app = build_router(AppState::new(store, RunnerConfig::default()));
    let body = json!({
        "kind": "heartbeat",
        "payload": {"edge": "api-01"},
        "idempotency_key": "api-key-01",
        "max_attempts": 3
    });

    let first = app
        .clone()
        .oneshot(json_request("/api/jobs", body.clone()))
        .await
        .expect("request should complete");
    assert_eq!(first.status(), StatusCode::CREATED);
    let first_body = read_json(first).await;
    let first_id = first_body["job"]["id"]
        .as_str()
        .expect("job id should be present")
        .to_string();
    assert_eq!(first_body["deduplicated"], false);

    let second = app
        .oneshot(json_request("/api/jobs", body))
        .await
        .expect("request should complete");
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = read_json(second).await;
    assert_eq!(second_body["deduplicated"], true);
    assert_eq!(second_body["job"]["id"], first_id);
}

#[tokio::test]
async fn job_detail_returns_404_for_unknown_id() {
    let store = JobStore::new();
    let app = build_router(AppState::new(store, RunnerConfig::default()));

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/jobs/missing")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_jobs_api_returns_created_job() {
    let store = JobStore::new();
    let app = build_router(AppState::new(store, RunnerConfig::default()));

    let create = app
        .clone()
        .oneshot(json_request(
            "/api/jobs",
            json!({"kind": "cache-warm", "payload": {"segment": "test"}}),
        ))
        .await
        .expect("request should complete");
    assert_eq!(create.status(), StatusCode::CREATED);

    let list = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/jobs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("request should complete");
    assert_eq!(list.status(), StatusCode::OK);

    let jobs: Vec<Job> = serde_json::from_value(read_json(list).await).expect("jobs should parse");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].kind, "cache-warm");
}

fn json_request(uri: &str, value: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(value.to_string()))
        .expect("request should build")
}

async fn read_json(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should read");
    serde_json::from_slice(&bytes).expect("response should be json")
}
