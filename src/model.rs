use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_MAX_ATTEMPTS: u32 = 4;
pub const MAX_ATTEMPTS_LIMIT: u32 = 10;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Canceled
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobEventKind {
    Queued,
    Started,
    RetryScheduled,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct JobEvent {
    pub at_ms: u64,
    pub kind: JobEventKind,
    pub message: String,
    pub attempt: u32,
}

impl JobEvent {
    pub fn new(at_ms: u64, kind: JobEventKind, message: impl Into<String>, attempt: u32) -> Self {
        Self {
            at_ms,
            kind,
            message: message.into(),
            attempt,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub payload: Value,
    pub idempotency_key: Option<String>,
    pub status: JobStatus,
    pub attempts: u32,
    pub max_attempts: u32,
    pub queued_at_ms: u64,
    pub scheduled_for_ms: u64,
    pub updated_at_ms: u64,
    pub completed_at_ms: Option<u64>,
    pub output: Option<String>,
    pub last_error: Option<String>,
    pub history: Vec<JobEvent>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CreateJobRequest {
    pub kind: String,
    #[serde(default)]
    pub payload: Value,
    pub idempotency_key: Option<String>,
    pub max_attempts: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnqueueResponse {
    pub deduplicated: bool,
    pub job: Job,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistoryRecord {
    pub job_id: String,
    pub status: JobStatus,
    pub event: JobEvent,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RunnerMetrics {
    pub total_jobs: usize,
    pub pending: usize,
    pub running: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub canceled: usize,
    pub retry_scheduled_events: usize,
    pub average_attempts_finished: f64,
}
