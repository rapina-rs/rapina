// ! OpenAPI specification generation
// !
// ! This module provides automatic OpenAPI 3.0 spec generation
// ! derived from your route definitions

mod endpoint;
mod spec;
#[cfg(feature = "swagger-ui")]
pub mod swagger_ui;

pub use endpoint::*;
pub use spec::*;
#[cfg(feature = "swagger-ui")]
pub use swagger_ui::{SwaggerUiConfig, swagger_ui_handler};
