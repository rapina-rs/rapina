## Background jobs

Jobs implement the `Job` trait and are enqueued via the job context:

```rust
pub struct SendEmail {
    pub to: String,
    pub subject: String,
}

#[async_trait::async_trait]
impl Job for SendEmail {
    async fn perform(&self, _ctx: &JobContext) -> Result<(), JobError> {
        // ...
        Ok(())
    }
}

// Enqueue from a handler:
ctx.enqueue(SendEmail { to, subject }).await?;
```

Run `rapina jobs init` once to set up the jobs migration, then `rapina migrate up`.
