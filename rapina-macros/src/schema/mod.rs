//! Schema macro for defining database entities with Prisma-like syntax.
//!
//! This module provides the `schema!` macro that generates SeaORM entity definitions
//! from a concise, declarative syntax where types indicate relationships.

mod analyze;
mod generate;
mod parse;
mod types;

use proc_macro2::TokenStream;

pub use analyze::analyze_schema;
pub use generate::generate_schema;
pub use parse::parse_schema;

/// Entry point for the schema macro implementation.
pub fn schema_impl(input: TokenStream) -> TokenStream {
    let parsed = match parse_schema(input) {
        Ok(schema) => schema,
        Err(err) => return err.to_compile_error(),
    };

    let analyzed = match analyze_schema(parsed) {
        Ok(schema) => schema,
        Err(err) => return err.to_compile_error(),
    };

    generate_schema(analyzed)
}
