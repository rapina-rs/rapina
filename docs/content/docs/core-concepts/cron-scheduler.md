+++
title = "Cron Scheduler"
description = "Run recurring background tasks on a cron schedule"
weight = 10
date = 2026-04-01
+++

The cron scheduler lets you run recurring tasks inside your Rapina application on a cron-based schedule, e.g. health checks, cache warm-ups, periodic cleanup, report generation. Anything that needs to happen on a timer without an external trigger.

Rapina makes scheduling cron jobs easy and integrates it into the application lifecycle. Scheduled jobs start automatically when the server boots and shut down gracefully on SIGINT/SIGTERM. Running tasks are cancelled cooperatively.

## Cron Scheduler vs Background Jobs

tl;dr: Use the cron scheduler for lightweight, periodic tasks that are safe to miss if the server restarts. Use [Background Jobs](background-jobs.md)  for durable, transactional work that must complete reliably.

| | Cron Scheduler                               | Background Jobs                                                |
|---|----------------------------------------------|----------------------------------------------------------------|
| **Trigger** | Time-based (cron expression)                 | Event-based (enqueued from code)                               |
| **Persistence** | None, in-memory only                         | PostgreSQL-backed                                              |
| **Retries** | None built-in                                | Configurable (exponential, fixed, none)                        |
| **Survives restarts** | No. Schedule restarts with the process       | Yes. Pending jobs persist in the database                      |
| **Use case** | Periodic maintenance, polling, cache refresh | Durable, transactional deferred work: emails, uploads, reports |
| **Infrastructure** | No extra dependencies                        | Requires PostgreSQL                                            |

## Prerequisites

Enable the `cron-scheduler` feature flag:

```toml
[dependencies]
rapina = { version = "0.11", features = ["cron-scheduler"] }
```

No database or external service is required. The scheduler runs entirely in-process.

## Defining a Cron Job

A cron job is an async function (or closure) that returns a `Result<(), E>` where `E` implements `std::error::Error`.
The simplest option is to use Rapina's own `Result<()>` from the prelude.
If the function returns an error, it is automatically logged and the schedule continues. One failure does not stop future executions.

```rust
use rapina::prelude::*;

async fn cleanup_expired_sessions() -> Result<()> {
    tracing::info!("Cleaning up expired sessions");
    // your cleanup logic here
    Ok(())
}

async fn sync_exchange_rates() -> Result<()> {
    tracing::info!("Syncing exchange rates");
    // your sync logic here
    Ok(())
}
```

## Registering Cron Jobs

Chain `.cron(schedule, task)` calls on the `Rapina` builder. Each call takes a cron expression and the task function:

```rust
use rapina::prelude::*;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    Rapina::new()
        .discover()
        .cron("0 */5 * * * *", cleanup_expired_sessions)
        .cron("0 0 * * * *", sync_exchange_rates)
        .listen("127.0.0.1:3000")
        .await
}
```

You can register as many cron jobs as you need. They all run concurrently on the Tokio runtime.

## Cron Expression Syntax

Rapina uses a **six-field** cron expression format (seconds granularity), powered by the [`croner`](https://crates.io/crates/croner) crate:

```
┌──────────── second (0–59)
│ ┌────────── minute (0–59)
│ │ ┌──────── hour (0–23)
│ │ │ ┌────── day of month (1–31)
│ │ │ │ ┌──── month (1–12)
│ │ │ │ │ ┌── day of week (0–6, Sunday = 0)
│ │ │ │ │ │
* * * * * *
```

### Common Examples

| Expression | Description |
|---|---|
| `1/5 * * * * *` | Every 5 seconds (starting at second 1) |
| `0 */10 * * * *` | Every 10 minutes |
| `0 0 * * * *` | Every hour, on the hour |
| `0 0 0 * * *` | Every day at midnight |
| `0 30 9 * * 1-5` | Weekdays at 9:30 AM |
| `0 0 */6 * * *` | Every 6 hours |

Note the **six fields** — the first field is seconds, which standard five-field cron expressions don't have. If you're adapting a traditional crontab schedule, prepend `0 ` to run at second zero of each matching minute.

## Graceful Shutdown

When Rapina receives a shutdown signal (SIGINT/SIGTERM), it:

1. Triggers a `CancellationToken` that signals all running cron tasks to stop.
2. Shuts down the underlying `JobScheduler`, preventing new ticks from firing.
3. Waits for the scheduler to fully drain before the process exits.

Each scheduled task runs inside a `tokio::select!` that races the task's future against the cancellation token. When the token fires, the task exits at its next `.await` point.

> **Important:** Graceful shutdown is _cooperative_. If your task runs blocking (non-async) code with no `.await` points, it cannot be interrupted — Tokio must wait for it to complete. Keep cron tasks async-friendly for shutdown to work as expected.

## Full Example

A complete application with HTTP routes and two scheduled cron jobs:

```rust
use rapina::prelude::*;

#[get("/")]
async fn hello() -> &'static str {
    "Hello, Rapina!"
}

async fn first_cronjob() -> Result<()> {
    tracing::info!("Doing some work (every 5 seconds)");
    Ok(())
}

async fn second_cronjob() -> Result<()> {
    tracing::info!("Doing some work (every 10 seconds)");
    Ok(())
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt().init();

    Rapina::new()
        .discover()
        .cron("*/5 * * * * *", first_cronjob)
        .cron("*/10 * * * * *", second_cronjob)
        .listen("127.0.0.1:3000")
        .await
}
```

Run it, and you'll see the cron jobs ticking in your terminal alongside normal HTTP traffic:

```
INFO first_cronjob: Doing some work (every 5 seconds)
INFO second_cronjob: Doing some work (every 10 seconds)
INFO first_cronjob: Doing some work (every 5 seconds)
```

## Error Handling

If a cron task returns an `Err`, the error is logged at the `error` level and the schedule continues uninterrupted:

```rust
use rapina::prelude::*;

async fn flaky_task() -> Result<()> {
    do_something_unreliable().await?;
    Ok(())
}
```

```
ERROR Error while running Rapina background job: something went wrong
```

The next scheduled tick will still fire as normal. If you need retry semantics or persistent job tracking, use [Background Jobs](background-jobs.md) instead.