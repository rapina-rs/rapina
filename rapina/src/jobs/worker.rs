//! Background job worker — polls `rapina_jobs`, claims work, and dispatches
//! to registered handlers.
//!
//! # Lifecycle
//!
//! Jobs move through three states in normal execution:
//!
//! ```text
//! pending → running → completed
//!                   ↘ failed   (or back to pending if retries remain)
//! ```
//!
//! The worker atomically transitions each job from `pending` to `running` via
//! a CTE-based `UPDATE … RETURNING` with `FOR UPDATE SKIP LOCKED`, so
//! multiple concurrent workers never claim the same row.
//!
//! # Graceful shutdown
//!
//! The worker installs its own SIGINT/SIGTERM listeners (identical to
//! `server.rs`). When a signal fires the poll loop exits after the
//! **current batch** finishes — no job is abandoned mid-execution.
//!
//! # Trace propagation
//!
//! When a job row has a `trace_id`, the worker opens a tracing span that
//! includes the original value so all log lines emitted during the job share
//! the same trace identifier as the HTTP request that enqueued it.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, FromQueryResult, Statement, Value};
use tracing::Instrument;

use crate::jobs::retry::{apply_failure, apply_success};
use crate::jobs::{JobDescriptor, JobResult, JobRow, RetryPolicy};
use crate::state::AppState;

/// Configuration for the in-process background job worker.
///
/// All fields have sensible defaults via [`Default`] — call [`JobConfig::default()`]
/// and override only what you need:
///
/// ```rust,ignore
/// use rapina::jobs::JobConfig;
/// use std::time::Duration;
///
/// let config = JobConfig::default()
///     .queues(["default", "emails"])
///     .poll_interval(Duration::from_secs(2));
/// ```
#[derive(Debug, Clone)]
pub struct JobConfig {
    /// How often the worker wakes up to check for new jobs.
    ///
    /// Shorter intervals reduce latency at the cost of more database round-trips.
    /// Default: 5 seconds.
    pub poll_interval: Duration,
    /// Maximum number of jobs claimed in a single poll cycle.
    ///
    /// Each claimed job is executed sequentially before the next poll.
    /// Increase this to raise throughput at the cost of higher tail latency
    /// for jobs at the back of the batch.
    /// Default: 10.
    pub batch_size: i32,
    /// Queues the worker subscribes to.
    ///
    /// Only jobs whose `queue` column matches one of these names are claimed.
    /// Default: `["default"]`.
    pub queues: Vec<String>,
    /// How long a job lock is held before another worker may reclaim it.
    ///
    /// Sets `locked_until = NOW() + job_timeout` when a job is claimed.
    /// If the worker process crashes the lock expires after this duration,
    /// at which point a new worker can pick up the job.
    /// Default: 30 seconds.
    pub job_timeout: Duration,
}

impl Default for JobConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            batch_size: 10,
            queues: vec!["default".to_string()],
            job_timeout: Duration::from_secs(30),
        }
    }
}

impl JobConfig {
    /// Overrides the poll interval.
    pub fn poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Overrides the per-cycle batch size.
    pub fn batch_size(mut self, size: i32) -> Self {
        self.batch_size = size;
        self
    }

    /// Overrides the list of queues to subscribe to.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// JobConfig::default().queues(["default", "emails", "heavy"])
    /// ```
    ///
    /// check if queues are empty
    /// if someone calls .queues([]), the build_claim_stmt generates `IN ()` which is invalid
    pub fn queues(mut self, queues: impl IntoIterator<Item = impl Into<String>>) -> Self {
        let q: Vec<String> = queues.into_iter().map(Into::into).collect();
        assert!(!q.is_empty(), "queues must not be empty");
        self.queues = q;
        self
    }

    /// Overrides the job execution lock timeout.
    pub fn job_timeout(mut self, timeout: Duration) -> Self {
        self.job_timeout = timeout;
        self
    }
}

/// The background worker that drives the job queue.
///
/// Constructed internally by [`Rapina::jobs`](crate::app::Rapina::jobs) and
/// spawned via `tokio::spawn` during server startup. Not intended for direct
/// construction outside the framework.
pub(crate) struct Worker {
    /// Shared application state passed to every job handler for DI.
    state: Arc<AppState>,
    /// Worker configuration (queues, intervals, timeouts).
    config: JobConfig,
}

impl Worker {
    /// Creates a new worker with the given state and configuration.
    pub(crate) fn new(state: Arc<AppState>, config: JobConfig) -> Self {
        Self { state, config }
    }

    /// Runs the poll loop until a shutdown signal is received.
    ///
    /// The loop polls immediately on startup, executes the claimed batch, then
    /// sleeps for `poll_interval`. SIGINT and SIGTERM both break the loop after
    /// the current batch completes — no job is left in `running` state on a
    /// clean shutdown.
    pub(crate) async fn run(self) {
        let mut ctrl_c = pin!(tokio::signal::ctrl_c());

        // Platform-specific SIGTERM future.  On non-Unix targets (Windows) this
        // future never resolves so only ctrl-c triggers shutdown.
        let mut sigterm: std::pin::Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::SignalKind;
                tokio::signal::unix::signal(SignalKind::terminate())
                    .expect("failed to install SIGTERM handler")
                    .recv()
                    .await;
            }
            #[cfg(not(unix))]
            {
                std::future::pending::<()>().await;
            }
        });

        tracing::info!(
            queues = ?self.config.queues,
            poll_interval_secs = self.config.poll_interval.as_secs(),
            "Job worker started"
        );

        let Some(db) = self.state.get::<DatabaseConnection>() else {
            tracing::error!(
                "Job worker: no DatabaseConnection in AppState — worker will not start. \
                             Call .with_database() before .jobs()."
            );
            return;
        };

        loop {
            // Claim and execute a batch before sleeping so jobs enqueued just
            // before startup are processed without an initial delay.
            match claim_batch(db, &self.config).await {
                Ok(jobs) => {
                    let n = jobs.len();
                    if n > 0 {
                        tracing::debug!(claimed = n, "Claimed job batch");
                    }
                    for job in jobs {
                        self.dispatch(db, job).await;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to claim jobs from database");
                }
            }

            // Wait for the next poll tick or a shutdown signal.
            tokio::select! {
                _ = tokio::time::sleep(self.config.poll_interval) => {}
                _ = ctrl_c.as_mut() => {
                    tracing::info!("Job worker received shutdown signal, stopping.");
                    break;
                }
                _ = sigterm.as_mut() => {
                    tracing::info!("Job worker received shutdown signal, stopping.");
                    break;
                }
            }
        }
    }

    /// Dispatches a single claimed job to its registered handler.
    ///
    /// Looks up the handler by `job_type` in the `inventory` registry.  If no
    /// handler is found the job is permanently failed immediately (no retry).
    /// Otherwise the handler is called and the result is forwarded to
    /// [`apply_success`] or [`apply_failure`].
    async fn dispatch(&self, db: &DatabaseConnection, job: JobRow) {
        let handler = inventory::iter::<JobDescriptor>
            .into_iter()
            .find(|d| d.job_type == job.job_type);

        let Some(descriptor) = handler else {
            tracing::warn!(
                job_id = %job.id,
                job_type = %job.job_type,
                "No handler registered for job type — permanently failing job"
            );
            // max_retries = 0 forces apply_failure to mark the job as failed
            // immediately without scheduling a retry.
            let _ = apply_failure(
                db,
                job.id,
                &format!("no handler registered for job type: {}", job.job_type),
                job.attempts,
                0,
                &RetryPolicy::None,
            )
            .await;
            return;
        };

        // Restore the original trace context so log lines from the handler are
        // correlated with the HTTP request that enqueued the job.
        let span = tracing::info_span!(
            "job",
            job_type = %job.job_type,
            job_id   = %job.id,
            trace_id = job.trace_id.as_deref().unwrap_or(""),
        );

        let result: JobResult = (descriptor.handle)(job.payload.clone(), self.state.clone())
            .instrument(span)
            .await;
        // Policy type and base delay come from the descriptor (set by `#[job]`
        // attributes at compile time); max_retries comes from the job row.
        let policy = build_policy(
            descriptor.retry_policy,
            job.max_retries,
            descriptor.retry_delay_secs,
        );

        match result {
            Ok(()) => {
                tracing::debug!(job_id = %job.id, job_type = %job.job_type, "Job completed");
                if let Err(e) = apply_success(db, job.id).await {
                    tracing::error!(job_id = %job.id, error = %e, "Failed to mark job as completed");
                }
            }
            Err(e) => {
                tracing::warn!(job_id = %job.id, job_type = %job.job_type, error = %e, "Job failed");
                if let Err(db_err) = apply_failure(
                    db,
                    job.id,
                    &e.to_string(),
                    job.attempts,
                    job.max_retries,
                    &policy,
                )
                .await
                {
                    tracing::error!(job_id = %job.id, error = %db_err, "Failed to record job failure");
                }
            }
        }
    }
}

/// Constructs a [`RetryPolicy`] from the descriptor's compile-time attributes
/// and the job row's `max_retries`.
fn build_policy(retry_policy: &str, max_retries: i32, delay_secs: f64) -> RetryPolicy {
    let delay = Duration::from_secs_f64(delay_secs);
    match retry_policy {
        "fixed" => RetryPolicy::fixed(max_retries, delay),
        "none" => RetryPolicy::none(),
        _ => RetryPolicy::exponential(max_retries, delay),
    }
}

/// Claims up to `config.batch_size` jobs from the subscribed queues in a
/// single atomic statement and returns their rows.
///
/// Uses `FOR UPDATE SKIP LOCKED` so concurrent workers never claim the same
/// row, and the CTE + `UPDATE … FROM` ensures the transition from `pending`
/// to `running` is a single round-trip.
async fn claim_batch(
    db: &DatabaseConnection,
    config: &JobConfig,
) -> Result<Vec<JobRow>, sea_orm::DbErr> {
    let stmt = build_claim_stmt(config);
    let rows = db.query_all(stmt).await?;
    rows.iter()
        .map(|row| JobRow::from_query_result(row, ""))
        .collect()
}

/// Builds the claiming statement for [`claim_batch`].
///
/// Generates dynamic `IN ($1, $2, …)` placeholders for the queue list so the
/// number of parameters matches the number of subscribed queues without
/// requiring PostgreSQL array literals (which SeaORM's plain `Value` type
/// does not support directly).
///
/// Parameter layout:
/// - `$1 … $n` — queue names (one per configured queue)
/// - `$n+1`    — batch size (`INTEGER`)
/// - `$n+2`    — job timeout in fractional seconds (`DOUBLE PRECISION`)
fn build_claim_stmt(config: &JobConfig) -> Statement {
    // One placeholder per queue, e.g. "$1, $2, $3".
    let placeholders = (1..=config.queues.len())
        .map(|i| format!("${i}"))
        .collect::<Vec<_>>()
        .join(", ");

    let batch_param = config.queues.len() + 1;
    let timeout_param = config.queues.len() + 2;

    let sql = format!(
        r#"WITH claimed AS (
               SELECT id FROM rapina_jobs
               WHERE  status  = 'pending'
                 AND  queue   IN ({placeholders})
                 AND  run_at <= NOW()
               ORDER  BY run_at ASC
               LIMIT  ${batch_param}
               FOR UPDATE SKIP LOCKED
           )
           UPDATE rapina_jobs
           SET status       = 'running',
               started_at   = NOW(),
               locked_until = NOW() + make_interval(secs => ${timeout_param})
           FROM claimed
           WHERE rapina_jobs.id = claimed.id
           RETURNING rapina_jobs.*"#
    );

    let mut values: Vec<Value> = config
        .queues
        .iter()
        .map(|q| Value::String(Some(Box::new(q.clone()))))
        .collect();
    values.push(Value::Int(Some(config.batch_size)));
    values.push(Value::Double(Some(config.job_timeout.as_secs_f64())));

    Statement::from_sql_and_values(DbBackend::Postgres, &sql, values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::state::AppState;

    // ── retry policy resolution ───────────────────────────────────────────────

    #[test]
    fn descriptor_exponential_produces_exponential_policy() {
        let policy = build_policy("exponential", 5, 2.0);
        assert!(matches!(
            policy,
            RetryPolicy::Exponential { max_retries: 5, .. }
        ));
    }

    #[test]
    fn descriptor_fixed_produces_fixed_policy() {
        let policy = build_policy("fixed", 3, 30.0);
        assert!(matches!(policy, RetryPolicy::Fixed { max_retries: 3, .. }));
    }

    #[test]
    fn descriptor_none_produces_none_policy() {
        let policy = build_policy("none", 0, 0.0);
        assert!(matches!(policy, RetryPolicy::None));
    }

    #[test]
    fn descriptor_unknown_policy_falls_back_to_exponential() {
        let policy = build_policy("bogus", 3, 1.0);
        assert!(matches!(policy, RetryPolicy::Exponential { .. }));
    }

    #[test]
    fn descriptor_base_delay_is_forwarded() {
        let policy = build_policy("exponential", 3, 5.0);
        match policy {
            RetryPolicy::Exponential { base_delay, .. } => {
                assert_eq!(base_delay, Duration::from_secs(5));
            }
            _ => panic!("expected Exponential"),
        }
    }

    #[test]
    fn descriptor_fixed_delay_is_forwarded() {
        let policy = build_policy("fixed", 3, 20.0);
        match policy {
            RetryPolicy::Fixed { delay, .. } => {
                assert_eq!(delay, Duration::from_secs(20));
            }
            _ => panic!("expected Fixed"),
        }
    }

    // ── no-database exit ─────────────────────────────────────────────────────

    /// Worker must not block forever when no `DatabaseConnection` is registered
    /// in `AppState`.  It should log an error and return immediately so the
    /// spawned task doesn't leak.
    #[tokio::test]
    async fn worker_exits_immediately_without_database() {
        let state = Arc::new(AppState::new()); // no DB registered
        let worker = Worker::new(state, JobConfig::default());
        let handle = tokio::spawn(worker.run());

        let result = tokio::time::timeout(Duration::from_millis(500), handle).await;
        assert!(
            result.is_ok(),
            "worker should return quickly when no DB is in AppState"
        );
        assert!(result.unwrap().is_ok(), "worker task should not panic");
    }

    #[test]
    fn job_config_defaults() {
        let config = JobConfig::default();
        assert_eq!(config.poll_interval, Duration::from_secs(5));
        assert_eq!(config.batch_size, 10);
        assert_eq!(config.queues, vec!["default"]);
        assert_eq!(config.job_timeout, Duration::from_secs(30));
    }

    #[test]
    fn job_config_builder_methods() {
        let config = JobConfig::default()
            .poll_interval(Duration::from_secs(2))
            .batch_size(5)
            .queues(["emails", "default"])
            .job_timeout(Duration::from_secs(60));

        assert_eq!(config.poll_interval, Duration::from_secs(2));
        assert_eq!(config.batch_size, 5);
        assert_eq!(config.queues, vec!["emails", "default"]);
        assert_eq!(config.job_timeout, Duration::from_secs(60));
    }

    #[test]
    fn build_claim_stmt_sql_shape() {
        let config = JobConfig::default(); // one queue: "default"
        let stmt = build_claim_stmt(&config);
        let sql = &stmt.sql;

        assert!(
            sql.contains("FOR UPDATE SKIP LOCKED"),
            "should lock claimed rows"
        );
        assert!(sql.contains("'running'"), "should transition to running");
        assert!(sql.contains("locked_until"), "should set lock expiry");
        assert!(
            sql.contains("RETURNING rapina_jobs.*"),
            "should return the claimed rows"
        );
        assert!(sql.contains("run_at <= NOW()"), "should filter by run_at");
    }

    #[test]
    fn build_claim_stmt_param_count_single_queue() {
        // 1 queue + batch_size + timeout = 3 params
        let config = JobConfig::default();
        let stmt = build_claim_stmt(&config);
        let params = stmt.values.as_ref().map(|v| v.0.len()).unwrap_or(0);
        assert_eq!(params, 3);
    }

    #[test]
    fn build_claim_stmt_param_count_multiple_queues() {
        // 3 queues + batch_size + timeout = 5 params
        let config = JobConfig::default().queues(["default", "emails", "heavy"]);
        let stmt = build_claim_stmt(&config);
        let params = stmt.values.as_ref().map(|v| v.0.len()).unwrap_or(0);
        assert_eq!(params, 5);
    }

    #[test]
    fn build_claim_stmt_uses_postgres_backend() {
        let stmt = build_claim_stmt(&JobConfig::default());
        assert_eq!(stmt.db_backend, DbBackend::Postgres);
    }

    #[test]
    fn build_claim_stmt_queue_values() {
        let config = JobConfig::default().queues(["emails"]);
        let stmt = build_claim_stmt(&config);
        let values = &stmt.values.as_ref().unwrap().0;
        assert_eq!(
            values[0],
            Value::String(Some(Box::new("emails".to_string())))
        );
    }

    #[test]
    fn build_claim_stmt_batch_and_timeout_values() {
        let config = JobConfig::default()
            .batch_size(7)
            .job_timeout(Duration::from_secs(45));
        let stmt = build_claim_stmt(&config);
        let values = &stmt.values.as_ref().unwrap().0;

        // 1 queue + batch + timeout → indices 1 and 2
        assert_eq!(values[1], Value::Int(Some(7)));
        assert_eq!(values[2], Value::Double(Some(45.0)));
    }
}
