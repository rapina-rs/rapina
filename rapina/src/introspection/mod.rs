//! Introspection utilities for Rapina applications.
//!
//! This module provides tools for inspecting route metadata,
//! enabling documentation generation and AI-native tooling.

mod endpoint;
mod llms;
mod llms_endpoint;
mod route_info;

pub use endpoint::{RouteRegistry, list_routes};
pub use llms::to_llms_txt;
pub use llms_endpoint::{LlmsRegistry, llms_txt_handler};
pub use route_info::RouteInfo;
