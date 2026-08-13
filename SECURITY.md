# Security Policy

## Supported Versions

This is a prototype project. Only the current `main` branch is expected to receive security fixes.

## Reporting a Vulnerability

Use a private GitHub Security Advisory if this repository is hosted with advisories enabled. If that is not available, open an issue with minimal reproduction details and avoid posting secrets, tokens, or private infrastructure data.

## Security Boundaries

- Jobs are simulated. The runner does not execute shell commands, scripts, or arbitrary user code.
- State is in memory and is not encrypted at rest because it is not persisted.
- The HTTP API has no authentication in this prototype.
- Do not expose this service directly to the public internet without adding authentication, authorization, rate limiting, and durable storage controls.
