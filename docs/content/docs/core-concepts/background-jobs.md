+++
title = "Background Jobs"
description = "Persistent job queue backed by PostgreSQL"
weight = 9
date = 2026-03-17
+++

Background jobs let you defer work to run outside the request cycle. Sending emails, processing uploads, generating reports — anything that shouldn't block an HTTP response.

Rapina's job system uses your existing PostgreSQL database as the queue. No Redis, no RabbitMQ, no extra infrastructure. Jobs are rows in a `rapina_jobs` table, claimed by in-process workers with `FOR UPDATE SKIP LOCKED` for safe concurrent processing.

This page covers setup, defining jobs, enqueuing, running the worker, and the retry system.

## Prerequisites

You need the `database` feature with PostgreSQL. The jobs migration uses PostgreSQL-specific features (`gen_random_uuid()`, partial indexes) and does not support MySQL or SQLite.

```toml
[dependencies]
rapina = { version = "0.10", features = ["postgres"] }
```

You also need a database connection configured in your app — see the [Database](/docs/core-concepts/database/) page.

## Setup

Run the CLI command from your project root:

```bash
rapina jobs init
```

This adds the framework's `create_rapina_jobs` migration to your `src/migrations/mod.rs`. If the file doesn't exist yet, it creates one. If the migration is already configured, it skips silently.

The result looks like this:

```rust
use rapina::jobs::create_rapina_jobs;

mod m20260315_000001_create_users;

rapina::migrations! {
    create_rapina_jobs,
    m20260315_000001_create_users,
}
```

The framework migration uses a zero timestamp (`m00000000_000000_`) so it always sorts before your application migrations, regardless of their dates.

Next time your app starts and runs migrations, the `rapina_jobs` table will be created.

## Defining a Job

Use the `#[job]` macro to define a handler. The first argument is always the payload — a struct that implements `Serialize + DeserializeOwned`. Remaining arguments are dependency-injected from `AppState` via `State<T>` or `Db`.

```rust
use rapina::prelude::*;

#[derive(Serialize, Deserialize)]
pub struct WelcomeEmailPayload {
    pub email: String,
}

#[job(queue = "emails", max_retries = 5)]
async fn send_welcome_email(
    payload: WelcomeEmailPayload,
    mailer: State<Mailer>,
) -> JobResult {
    mailer.send(&payload.email).await?;
    Ok(())
}
```

The macro generates a `send_welcome_email(payload) -> JobRequest` helper used for enqueuing.

| Attribute | Default | Description |
|-----------|---------|-------------|
| `queue` | `"default"` | Queue to place the job in |
| `max_retries` | `3` | Total execution count before permanent failure (includes the initial run) |
| `retry_policy` | `"exponential"` | Retry strategy: `"exponential"`, `"fixed"`, or `"none"` |
| `retry_delay_secs` | `1.0` | Base delay in seconds — used as the backoff base for `"exponential"` and the fixed interval for `"fixed"` |

## Enqueuing Jobs

Use the `Jobs` extractor in HTTP handlers to dispatch jobs.

```rust
#[post("/users")]
async fn create_user(body: Json<CreateUserRequest>, jobs: Jobs) -> Result<StatusCode> {
    jobs.enqueue(send_welcome_email(WelcomeEmailPayload {
        email: body.email.clone(),
    })).await?;

    Ok(StatusCode::CREATED)
}
```

For transactional enqueue — where the job row commits atomically with your business logic — use `enqueue_with`:

```rust
let txn = db.conn().begin().await?;
let user = User::insert(&txn, &body).await?;
jobs.enqueue_with(&txn, send_welcome_email(WelcomeEmailPayload {
    email: user.email.clone(),
})).await?;
txn.commit().await?;
```

If the transaction rolls back, the job is never created.

## Starting the Worker

Call `.jobs()` on the application builder before `.listen()`. The worker spawns in-process alongside the HTTP server and shuts down gracefully on SIGINT/SIGTERM — it finishes its current batch before stopping.

```rust
use rapina::jobs::JobConfig;

Rapina::new()
    .with_database(db_config).await?
    .jobs(JobConfig::default())
    .listen("127.0.0.1:3000")
    .await
```

All options have sensible defaults. Override only what you need:

```rust
JobConfig::default()
    .poll_interval(Duration::from_secs(2))
    .batch_size(20)
    .queues(["default", "emails", "heavy"])
    .job_timeout(Duration::from_secs(60))
```

| Option | Default | Description |
|--------|---------|-------------|
| `poll_interval` | 5s | How often the worker wakes up to claim jobs |
| `batch_size` | 10 | Maximum jobs claimed per poll cycle |
| `queues` | `["default"]` | Queue names to subscribe to |
| `job_timeout` | 30s | How long a job lock is held — expired locks can be reclaimed after a worker crash |

## Job Lifecycle

```
pending → running → completed
                  ↘ failed   (or back to pending if retries remain)
```

The worker atomically transitions each job from `pending` to `running` in a single SQL statement. On completion the job moves to `completed` or `failed`.

Failed jobs are retried according to the `retry_policy` set on the handler.

### Exponential backoff (default)

```rust
#[job(max_retries = 5, retry_policy = "exponential", retry_delay_secs = 1.0)]
async fn send_welcome_email(payload: EmailPayload) -> JobResult { ... }
```

| Attempt | Delay (base = 1s) |
|---------|-------------------|
| 1 | immediate |
| 2 | 1s + jitter |
| 3 | 4s + jitter |
| 4 | 16s + jitter |

Jitter is seeded from the job's UUID so concurrent failures don't retry in lockstep. Delay is capped at one week.

### Fixed delay

```rust
#[job(max_retries = 10, retry_policy = "fixed", retry_delay_secs = 30.0)]
async fn sync_inventory(payload: SyncPayload) -> JobResult { ... }
```

Every retry waits the same `retry_delay_secs`. The first retry is always immediate regardless of the configured delay.

### No retries

```rust
#[job(max_retries = 1, retry_policy = "none")]
async fn charge_card(payload: ChargePayload) -> JobResult { ... }
```

The job is permanently marked `failed` on the first error. Use this for operations that must not be duplicated.


## DI Limitations

Job handlers run outside the request cycle. Only `State<T>` and `Db` work — they source data from `AppState` directly. Request-bound extractors (`Context`, `Headers`, `Path`, `Query`, `CurrentUser`) will fail at runtime and must not be used in job handlers.

## Trace Propagation

When a job is enqueued from an HTTP handler, the request's `trace_id` is stored on the job row. The worker restores it into its tracing span before calling the handler, so all log lines emitted during job execution are correlated with the original HTTP request.

## Table Schema

The migration creates a `rapina_jobs` table with the following columns:

| Column | Type | Default | Description |
|--------|------|---------|-------------|
| `id` | UUID | `gen_random_uuid()` | Primary key |
| `queue` | VARCHAR(255) | `'default'` | Logical queue name |
| `job_type` | VARCHAR(255) | — | Fully-qualified type name for dispatch |
| `payload` | JSONB | `'{}'` | Arbitrary data passed to the handler |
| `status` | VARCHAR(32) | `'pending'` | Lifecycle state |
| `attempts` | INTEGER | `0` | Number of times this job has been attempted |
| `max_retries` | INTEGER | `3` | Maximum retry count before permanent failure |
| `run_at` | TIMESTAMPTZ | `now()` | Earliest time to execute |
| `started_at` | TIMESTAMPTZ | NULL | When a worker started processing |
| `locked_until` | TIMESTAMPTZ | NULL | Lease expiry for crash recovery |
| `finished_at` | TIMESTAMPTZ | NULL | When the job completed or permanently failed |
| `last_error` | TEXT | NULL | Error from the most recent failed attempt |
| `trace_id` | VARCHAR(64) | NULL | Distributed trace ID from the enqueuing request |
| `created_at` | TIMESTAMPTZ | `now()` | Insertion timestamp |

A partial index on `(queue, run_at) WHERE status = 'pending'` optimizes the worker's claim query.

## Types

### JobStatus

The `JobStatus` enum represents the lifecycle of a job:

```rust
use rapina::prelude::*;

// Available when the `database` feature is enabled
let status = JobStatus::Pending;
println!("{status}"); // "pending"

let parsed: JobStatus = "running".parse().unwrap();
```

| Variant | Meaning |
|---------|---------|
| `Pending` | Queued and waiting for a worker |
| `Running` | Claimed by a worker, currently executing |
| `Completed` | Finished successfully |
| `Failed` | Exhausted all retries or hit a fatal error |

`JobStatus` implements `Display`, `FromStr`, `Serialize`, `Deserialize`, `Hash`, `Copy`, and `Eq`. The string representation is always lowercase.

### JobRow

`JobRow` is a plain struct that maps directly to a row in the `rapina_jobs` table. It derives SeaORM's `FromQueryResult` so you can use it with raw queries:

```rust
use rapina::jobs::JobRow;
use rapina::sea_orm::{FromQueryResult, Statement, DatabaseBackend};
use rapina::database::Db;

let rows: Vec<JobRow> = JobRow::find_by_statement(
    Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT * FROM rapina_jobs WHERE queue = 'emails' AND status = 'failed'"
    )
)
.all(db.conn())
.await
.map_err(DbError::from)?;

for row in &rows {
    let status = row.parse_status().unwrap();
    println!("{}: {} (attempts: {})", row.id, status, row.attempts);
}
```

The `status` field is a `String` because SeaORM's `FromQueryResult` derive doesn't support custom enum deserialization. Use `parse_status()` to get a typed `JobStatus`.

## Manual Setup

If you prefer not to use the CLI, add the migration reference manually:

```rust
// src/migrations/mod.rs
use rapina::jobs::create_rapina_jobs;

mod m20260315_000001_create_users;

rapina::migrations! {
    create_rapina_jobs,
    m20260315_000001_create_users,
}
```

The `create_rapina_jobs` module is exported from the `rapina` crate, so there's no file to create in your project — just the `use` import and the macro entry.
