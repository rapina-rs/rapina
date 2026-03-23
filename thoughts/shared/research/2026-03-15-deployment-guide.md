---
date: 2026-03-15T00:00:00-04:00
researcher: Claude
git_commit: 46d75ef
branch: main
repository: rapina
topic: "Deployment documentation for Rapina applications"
tags: [research, codebase, deployment, docker, production, configuration]
status: complete
last_updated: 2026-03-15
last_updated_by: Claude
---

# Research: Deployment Documentation for Rapina Applications

**Date**: 2026-03-15
**Git Commit**: 46d75ef
**Branch**: main
**Repository**: rapina

## Research Question
What deployment-related features does Rapina provide, and how should they be documented for users deploying to production?

## Summary
Created `docs/content/docs/guides/deployment.md` covering: building for release, environment variables and configuration, Docker multi-stage builds, reverse proxy setup (nginx/Caddy), health check endpoints, graceful shutdown, deployment targets (Railway, Fly.io, AWS ECS, bare metal), and a production checklist (logging, tracing, metrics, rate limiting, CORS, request safeguards). Also created `docs/content/docs/guides/_index.md` as the section index.

## Detailed Findings

### Configuration System
- `load_dotenv()` loads `.env` via `dotenvy`; silent if missing
- `#[derive(Config)]` macro generates `from_env()` with fail-fast batch validation
- `DatabaseConfig::from_env()` reads `DATABASE_URL` (required) + pool settings
- `AuthConfig::from_env()` reads `JWT_SECRET` (required) + `JWT_EXPIRATION`
- Host/port not auto-read — users must format the string for `listen()`

### Graceful Shutdown
- SIGINT and SIGTERM both handled via `tokio::signal`
- `hyper_util::server::graceful::GracefulShutdown` drains connections
- Default timeout: 30 seconds, configurable via `.shutdown_timeout()`
- Shutdown hooks registered via `.on_shutdown()`, run sequentially after drain

### Health Checks
- No built-in health endpoint — users add a regular route
- `#[public]` attribute or `.public_route()` needed when auth is enabled

### Logging/Tracing
- `tracing` + `tracing-subscriber` with JSON output option
- `RUST_LOG` env var takes precedence over programmatic level
- `RequestLogMiddleware` logs method, path, status, duration per request

### Metrics
- Behind `metrics` feature flag
- Prometheus text format at `GET /metrics`
- Three metrics: `http_requests_total`, `http_request_duration_seconds`, `http_requests_in_flight`

### Middleware Stack
- CORS: `CorsConfig::permissive()` or `CorsConfig::with_origins()`
- Rate limiting: token bucket algorithm, per-IP by default
- Compression: gzip/deflate behind `compression` feature (default)
- Timeout, body limit, trace ID — opt-in via `.middleware()`

### Build System
- Single binary from `cargo build --release`
- No Dockerfiles in the repo
- MSRV 1.85, edition 2024

## Code References
- `rapina/src/server.rs:23-119` — Server startup, signal handling, graceful shutdown
- `rapina/src/app.rs:600-612` — `Rapina::listen()` entry point
- `rapina/src/config.rs` — Env utilities and `ConfigError`
- `rapina/src/database.rs:99-183` — `DatabaseConfig::from_env()` and `connect()`
- `rapina/src/auth/mod.rs:184-228` — `AuthConfig::from_env()`
- `rapina/src/observability/tracing.rs` — `TracingConfig`
- `rapina/src/middleware/cors.rs` — CORS middleware
- `rapina/src/middleware/rate_limit.rs` — Rate limiting middleware
- `rapina/src/metrics/prometheus.rs` — Metrics registry and handler

## Files Created
- `docs/content/docs/guides/_index.md` — Section index for guides
- `docs/content/docs/guides/deployment.md` — Full deployment guide
