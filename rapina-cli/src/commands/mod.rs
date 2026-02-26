//! CLI command implementations.

pub mod add;
pub(crate) mod codegen;
pub mod dev;
pub mod doctor;
#[cfg(feature = "import")]
pub mod import;
pub mod migrate;
pub mod new;
pub mod openapi;
pub mod routes;
pub mod test;
