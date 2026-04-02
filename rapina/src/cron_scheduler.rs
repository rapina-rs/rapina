//! Background task scheduler for recurring cron-based jobs.
//!
//! For durable, persistent work use [`crate::jobs`] instead. This scheduler
//! is in-memory only and does not survive process restarts.
use std::sync::Arc;
use tokio_cron_scheduler::{Job, JobScheduler};
use tokio_util::sync::CancellationToken;

/// A background job scheduler that wraps `tokio_cron_scheduler::JobScheduler`.
pub struct CronScheduler {
    jobs: Vec<Job>,
    scheduler: Option<JobScheduler>,
    cancellation_token: CancellationToken,
    started: bool,
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl CronScheduler {
    /// Create a new CronScheduler.
    pub fn new() -> Self {
        Self {
            jobs: Vec::new(),
            scheduler: None,
            cancellation_token: CancellationToken::new(),
            started: false,
        }
    }

    /// Schedules a new background task to be executed according to the provided configuration.
    ///
    /// The task must be an asynchronous closure or function that returns a `std::io::Result<()>`.
    /// If the scheduled task returns an error, it will be automatically logged.
    ///
    /// In case blocking code is executed in `task`, the task can _not_ be
    /// interrupted automatically and will continue to execute even after a shutdown signal was received.
    /// The reason is that Rust async cancellation is cooperative. Tokio cannot forcefully kill an OS thread.
    /// Tokio must wait for the code to hit an .await point to check if it should cancel the task.
    /// Because there are no .await points during the blocking `task`, the task never yields.
    ///
    /// For graceful shutdown semantics to work as expected, make sure to not run blocking code as part of `task`.
    ///
    /// # Panics
    ///
    /// Panics if the job cannot be created or if it fails to be added to the underlying scheduler.
    pub fn schedule<F, Fut>(&mut self, cron_schedule: String, task: F) -> std::io::Result<()>
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::io::Result<()>> + Send + 'static,
    {
        //Wrap task function in an Arc so it can be safely cloned for every cron tick
        let task = Arc::new(task);

        // Clone main cancellation token
        let cronjob_cancellation_token = self.cancellation_token.clone();

        let job = Job::new_async(&cron_schedule, move |_uuid, _l| {
            let cronjob_cancellation_token = cronjob_cancellation_token.clone();
            let task = task.clone();
            Box::pin(async move {
                tokio::select! {
                _ = cronjob_cancellation_token.cancelled() => {
                    tracing::info!("Shutdown signal received, stopping job scheduler");
                }
                result = task() => {
                    if let Err(e) = result {
                        tracing::error!("Error while running Rapina background job: {:?}", e);
                    }
                }
                }
            })
        })
        .expect("Failed to create Rapina background job template");

        let job_uuid = job.guid();

        // Store the Job synchronously for later
        self.jobs.push(job);

        tracing::debug!(
            "Added cron job with uuid '{}' and schedule '{}' to cron job queue",
            job_uuid,
            &cron_schedule
        );

        Ok(())
    }

    /// Starts the scheduler
    ///
    /// # Panics
    ///
    /// Panics if the underlying `JobScheduler` fails to start.
    pub async fn start(&mut self) {
        let scheduler = JobScheduler::new()
            .await
            .expect("Failed to create cronjob scheduler");

        for job in self.jobs.drain(..) {
            scheduler
                .add(job)
                .await
                .expect("Failed to schedule Rapina background job");
        }

        scheduler
            .start()
            .await
            .expect("Failed to start Rapina background job scheduler");

        self.scheduler = Some(scheduler);

        self.started = true;

        tracing::info!("Started Rapina background job scheduler");
    }

    /// Initiates a graceful shutdown of the scheduler and its running tasks.
    ///
    /// This method triggers the `CancellationToken` to notify all currently running tasks to stop.
    /// It then shuts down the underlying `JobScheduler`.
    ///
    /// # Panics
    ///
    /// Panics if the underlying `JobScheduler` fails to shut down properly.
    pub async fn shutdown(&mut self) {
        // instantly trigger all tokio::select! branches that watch the token cancellation, stopping the execution immediately in case no blocking code is being executed
        self.cancellation_token.cancel();

        if let Some(mut scheduler) = self.scheduler.take() {
            scheduler
                .shutdown()
                .await
                .expect("Failed to shutdown Rapina background jobs");
        }
        tracing::info!("Shutdown signal received, stopping cron scheduler");
    }

    /// Returns the number of scheduled jobs, which have not yet been drained by starting the cron scheduler
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    /// Returns whether the list of scheduled jobs, which have not yet been drained by starting the cron scheduler, is empty
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Returns whether the cron scheduler has been started
    pub fn is_started(&self) -> bool {
        self.started
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn test_new_scheduler_is_empty() {
        let scheduler = CronScheduler::new();
        assert!(scheduler.jobs.is_empty());
        assert!(scheduler.scheduler.is_none());
        assert!(!scheduler.cancellation_token.is_cancelled());
    }

    #[test]
    fn test_schedule_adds_job_to_vec_synchronously() {
        let mut scheduler = CronScheduler::new();

        let result = scheduler.schedule("1/1 * * * * *".to_string(), || async { Ok(()) });

        assert!(result.is_ok());
        assert_eq!(scheduler.jobs.len(), 1);
        assert!(scheduler.scheduler.is_none()); // Scheduler should not be created yet
    }

    #[tokio::test]
    async fn test_start_creates_and_empties_jobs() {
        let mut scheduler = CronScheduler::new();

        scheduler
            .schedule("1/1 * * * * *".to_string(), || async { Ok(()) })
            .unwrap();
        scheduler
            .schedule("1/2 * * * * *".to_string(), || async { Ok(()) })
            .unwrap();

        assert_eq!(scheduler.jobs.len(), 2);

        // Start the scheduler
        scheduler.start().await;

        // Jobs should be moved into the underlying JobScheduler
        assert!(scheduler.jobs.is_empty());
        assert!(scheduler.scheduler.is_some());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_cron_execution_and_shutdown() {
        let mut scheduler = CronScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Schedule a job that runs every second
        scheduler
            .schedule("*/1 * * * * *".to_string(), move || {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .unwrap();

        scheduler.start().await;

        // Wait a little over 2 seconds to ensure the cron ticks at least twice
        tokio::time::sleep(Duration::from_millis(2200)).await;

        let executions_before_shutdown = counter.load(Ordering::SeqCst);
        assert!(
            executions_before_shutdown >= 2,
            "Expected at least 2 executions, got {}",
            executions_before_shutdown
        );

        // Initiate graceful shutdown
        scheduler.shutdown().await;
        assert!(scheduler.cancellation_token.is_cancelled());

        // Wait another 2 seconds to ensure no more ticks occur after shutdown
        tokio::time::sleep(Duration::from_millis(2200)).await;

        let executions_after_shutdown = counter.load(Ordering::SeqCst);

        // The counter should not have incremented significantly after shutdown.
        // Depending on exact timing, 1 extra tick might sneak in during the shutdown process,
        // but it shouldn't jump by 2.
        assert!(
            executions_after_shutdown - executions_before_shutdown <= 1,
            "Job continued executing after shutdown! Before: {}, After: {}",
            executions_before_shutdown,
            executions_after_shutdown
        );
    }

    #[test]
    fn test_is_empty_after_scheduling() {
        let mut scheduler = CronScheduler::new();
        scheduler
            .schedule("1/1 * * * * *".to_string(), || async { Ok(()) })
            .unwrap();

        assert!(
            !scheduler.is_empty(),
            "Scheduler should not be empty after a job is added"
        );
    }

    #[test]
    fn test_len_increments_with_jobs() {
        let mut scheduler = CronScheduler::new();

        scheduler
            .schedule("1/1 * * * * *".to_string(), || async { Ok(()) })
            .unwrap();
        assert_eq!(scheduler.len(), 1);

        scheduler
            .schedule("1/2 * * * * *".to_string(), || async { Ok(()) })
            .unwrap();
        assert_eq!(scheduler.len(), 2);
    }

    #[tokio::test]
    async fn test_is_started_becomes_true_after_start() {
        let mut scheduler = CronScheduler::new();
        scheduler.start().await;

        assert!(
            scheduler.is_started(),
            "Scheduler must be marked as started after calling start()"
        );
    }
}
