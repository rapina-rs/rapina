//! Type mapping for schema fields to Rust/SeaORM types.

use proc_macro2::TokenStream;
use quote::quote;

/// Scalar types supported in schema definitions.
#[derive(Debug, Clone, PartialEq)]
pub enum ScalarType {
    String,
    Text,
    I32,
    I64,
    F32,
    F64,
    Bool,
    Uuid,
    DateTime,
    NaiveDateTime,
    Date,
    Time,
    Decimal,
    Json,
    Bytes,
}

impl ScalarType {
    /// Parse a type identifier into a scalar type.
    pub fn from_ident(ident: &str) -> Option<Self> {
        match ident {
            "String" => Some(ScalarType::String),
            "Text" => Some(ScalarType::Text),
            "i32" | "i16" | "i8" | "u16" | "u8" | "integer" | "int" => Some(ScalarType::I32),
            "i64" | "u64" | "u32" | "bigint" => Some(ScalarType::I64),
            "f32" | "float" => Some(ScalarType::F32),
            "f64" | "double" => Some(ScalarType::F64),
            "bool" | "boolean" => Some(ScalarType::Bool),
            "Uuid" | "uuid" => Some(ScalarType::Uuid),
            "DateTime" | "DateTimeUtc" | "timestamptz" => Some(ScalarType::DateTime),
            "NaiveDateTime" | "timestamp" => Some(ScalarType::NaiveDateTime),
            "Date" | "date" => Some(ScalarType::Date),
            "Time" | "time" => Some(ScalarType::Time),
            "Decimal" | "numeric" | "money" => Some(ScalarType::Decimal),
            "Json" | "json" | "jsonb" => Some(ScalarType::Json),
            "Bytes" | "Blob" | "binary" | "bytea" | "varbinary" => Some(ScalarType::Bytes),
            _ => None,
        }
    }

    /// Generate the Rust type for this scalar.
    pub fn rust_type(&self) -> TokenStream {
        match self {
            ScalarType::String | ScalarType::Text => quote! { String },
            ScalarType::I32 => quote! { i32 },
            ScalarType::I64 => quote! { i64 },
            ScalarType::F32 => quote! { f32 },
            ScalarType::F64 => quote! { f64 },
            ScalarType::Bool => quote! { bool },
            ScalarType::Uuid => quote! { rapina::uuid::Uuid },
            ScalarType::DateTime => quote! { DateTimeUtc },
            ScalarType::NaiveDateTime => quote! { DateTime },
            ScalarType::Date => quote! { Date },
            ScalarType::Time => quote! { Time },
            ScalarType::Decimal => quote! { rapina::rust_decimal::Decimal },
            ScalarType::Json => quote! { Json },
            ScalarType::Bytes => quote! { Vec<u8> },
        }
    }

    /// Generate SeaORM column type attribute if needed.
    /// Returns None if the default column type is correct.
    pub fn column_type_attr(&self) -> Option<TokenStream> {
        match self {
            ScalarType::Text => Some(quote! { #[sea_orm(column_type = "Text")] }),
            ScalarType::Decimal => {
                Some(quote! { #[sea_orm(column_type = "Decimal(Some((19, 4)))")] })
            }
            ScalarType::Json => Some(quote! { #[sea_orm(column_type = "Json")] }),
            _ => None,
        }
    }
}

/// Field type classification.
#[derive(Debug, Clone)]
pub enum FieldType {
    /// A scalar database column (String, i32, etc.)
    Scalar { scalar: ScalarType, optional: bool },
    /// A has_many relationship (Vec<Entity>)
    HasMany { target: syn::Ident },
    /// A belongs_to relationship (Entity or Option<Entity>)
    BelongsTo { target: syn::Ident, optional: bool },
}
