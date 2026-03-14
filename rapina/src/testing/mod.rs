//! Testing utilities for Rapina applications.
//!
//! This module provides a test client for integration testing without
//! starting a full HTTP server.

mod client;
mod snapshot;

pub use client::{TestClient, TestRequestBuilder, TestResponse};
pub use snapshot::assert_snapshot;
