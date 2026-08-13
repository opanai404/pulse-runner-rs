use std::time::Instant;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Serialize;
use tower_http::{
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::{
    model::{CreateJobRequest, EnqueueResponse, Job, RunnerMetrics},
    runner::RunnerConfig,
    store::{JobStore, StoreError},
};

#[derive(Clone)]
pub struct AppState {
    pub store: JobStore,
    pub config: RunnerConfig,
    started_at: Instant,
}

impl AppState {
    pub fn new(store: JobStore, config: RunnerConfig) -> Self {
        Self {
            store,
            config,
            started_at: Instant::now(),
        }
    }

    fn uptime_ms(&self) -> u128 {
        self.started_at.elapsed().as_millis()
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/jobs", get(list_jobs).post(create_job))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/cancel", post(cancel_job))
        .route("/api/history", get(history))
        .route("/api/metrics", get(metrics))
        .route_service("/", ServeFile::new("assets/index.html"))
        .route_service("/dashboard", ServeFile::new("assets/index.html"))
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health(State(state): State<AppState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_ms: state.uptime_ms(),
    })
}

async fn create_job(
    State(state): State<AppState>,
    Json(request): Json<CreateJobRequest>,
) -> Result<(StatusCode, Json<EnqueueResponse>), ApiError> {
    let response = state.store.enqueue(request).await.map_err(ApiError::from)?;
    let status = if response.deduplicated {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };

    Ok((status, Json(response)))
}

async fn list_jobs(State(state): State<AppState>) -> Json<Vec<Job>> {
    Json(state.store.list().await)
}

async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    state
        .store
        .get(&id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("job {id} was not found")))
}

async fn cancel_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, ApiError> {
    state
        .store
        .cancel(&id)
        .await
        .map(Json)
        .ok_or_else(|| ApiError::not_found(format!("job {id} was not found")))
}

async fn history(State(state): State<AppState>) -> Json<Vec<crate::model::HistoryRecord>> {
    Json(state.store.history().await)
}

async fn metrics(State(state): State<AppState>) -> Json<RunnerMetrics> {
    Json(state.store.metrics().await)
}

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    NotFound(String),
}

impl ApiError {
    fn not_found(message: String) -> Self {
        Self::NotFound(message)
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        Self::BadRequest(error.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            ApiError::NotFound(message) => (StatusCode::NOT_FOUND, message),
        };

        (status, Json(ErrorResponse { error: message })).into_response()
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_ms: u128,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}
