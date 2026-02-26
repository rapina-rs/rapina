//! CLI command implementations.

pub mod add;
pub(crate) mod codegen;
pub mod dev;
#[cfg(feature = "import")]
pub mod import;
pub mod doctor;
pub mod migrate;
pub mod new;
pub mod openapi;
pub mod routes;
pub mod test;
