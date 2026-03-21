+++
title = "Background Jobs"
description = "Persistent job queue backed by PostgreSQL"
weight = 9
date = 2026-03-17
+++

Background jobs let you defer work to run outside the request cycle. Sending emails, processing uploads, generating reports — anything that shouldn't block an HTTP response.

Rapina's job system uses your existing PostgreSQL database as the queue. No Redis, no RabbitMQ, no extra infrastructure. Jobs are rows in a `rapina_jobs` table, claimed by workers with `FOR UPDATE SKIP LOCKED` for safe concurrent processing.

This page covers the foundation: the database table, the types, and the CLI setup. The `#[job]` macro, `Jobs` extractor, and worker runtime are coming in future releases.

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
