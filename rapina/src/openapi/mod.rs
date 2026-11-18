// ! OpenAPI specification generation
// !
// ! This module provides automatic OpenAPI 3.0 spec generation
// ! derived from your route definitions

mod endpoint;
mod spec;
#[cfg(any(feature = "openapi-scalar", feature = "openapi-swagger"))]
mod ui;
#[cfg(any(feature = "openapi-scalar", feature = "openapi-swagger"))]
pub use ui::OpenApiUi;

pub use endpoint::*;
pub use spec::*;
