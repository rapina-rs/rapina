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
    Date,
    Decimal,
    Json,
}

impl ScalarType {
    /// Parse a type identifier into a scalar type.
    pub fn from_ident(ident: &str) -> Option<Self> {
        match ident {
            "String" => Some(ScalarType::String),
            "Text" => Some(ScalarType::Text),
            "i32" => Some(ScalarType::I32),
            "i64" => Some(ScalarType::I64),
            "f32" => Some(ScalarType::F32),
            "f64" => Some(ScalarType::F64),
            "bool" => Some(ScalarType::Bool),
            "Uuid" => Some(ScalarType::Uuid),
            "DateTime" => Some(ScalarType::DateTime),
            "Date" => Some(ScalarType::Date),
            "Decimal" => Some(ScalarType::Decimal),
            "Json" => Some(ScalarType::Json),
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
            ScalarType::Date => quote! { Date },
            ScalarType::Decimal => quote! { rapina::rust_decimal::Decimal },
            ScalarType::Json => quote! { Json },
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

/// Reserved field names that are auto-generated.
pub const RESERVED_FIELDS: &[&str] = &["id", "created_at", "updated_at"];

/// Check if a field name is reserved.
pub fn is_reserved_field(name: &str) -> bool {
    RESERVED_FIELDS.contains(&name)
}
