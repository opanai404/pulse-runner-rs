# Changelog

## 0.1.0 - 2026-08-13

- Initial working prototype.
- Added Axum HTTP API for health, jobs, metrics, history, and cancel.
- Added Tokio worker dispatcher with bounded concurrency.
- Added idempotency-key deduplication.
- Added deterministic exponential retry/backoff.
- Added in-memory job history and metrics.
- Added static operations dashboard and preview asset.
- Added tests, Dockerfile, CI workflow, and project documentation.
