//! Background jobs support for Rapina applications.
//!
//! This module provides the database foundation for the background jobs system:
//! a migration to create the `rapina_jobs` table and types for working with job rows.
//!
//! **Note:** The migration uses PostgreSQL-specific features (`gen_random_uuid()`,
//! partial indexes). MySQL and SQLite are not currently supported for background jobs.
//!
//! # Setup
//!
//! Add the framework migration to your project's migration list:
//!
//! ```rust,ignore
//! use rapina::jobs::create_rapina_jobs;
//!
//! rapina::migrations! {
//!     create_rapina_jobs,
//!     m20260315_000001_create_users,
//! }
//! ```
//!
//! Or run `rapina jobs init` to configure it automatically.

pub mod create_rapina_jobs;
mod model;

pub use model::{JobRow, JobStatus};
