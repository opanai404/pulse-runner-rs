# Contributing

Thanks for considering a contribution to Pulse Runner RS.

## Development Setup

Install Rust 1.94.0 with `rustfmt` and `clippy`, then run:

```bash
cargo fmt --check
cargo test
cargo clippy -- -D warnings
```

## Pull Request Guidelines

- Keep changes focused and explain the behavior change.
- Add or update tests for runner, store, and API behavior.
- Keep the safe-executor boundary intact. Do not add shell execution, arbitrary code execution, or untrusted plugin loading.
- Update README or docs when public API behavior changes.

## Local Run

```bash
cargo run
```

The dashboard is served at `http://127.0.0.1:8080`.
