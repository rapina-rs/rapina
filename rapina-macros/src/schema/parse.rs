//! Parsing layer for the schema macro.
//!
//! Handles custom syn parsing for entity definitions.

use proc_macro2::{Span, TokenStream};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, Result, Token, braced};

use super::types::{ScalarType, is_reserved_field};

/// A complete schema definition containing multiple entities.
#[derive(Debug)]
pub struct Schema {
    pub entities: Vec<EntityDef>,
}

/// Attributes that can be applied to an entity.
#[derive(Debug, Clone)]
pub struct EntityAttrs {
    /// Custom table name, e.g., #[table_name = "people"]
    pub table_name: Option<String>,
    /// Include created_at timestamp (default: true)
    pub has_created_at: bool,
    /// Include updated_at timestamp (default: true)
    pub has_updated_at: bool,
}

impl Default for EntityAttrs {
    fn default() -> Self {
        Self {
            table_name: None,
            has_created_at: true,
            has_updated_at: true,
        }
    }
}

/// Attributes that can be applied to a field.
#[derive(Debug, Default, Clone)]
pub struct FieldAttrs {
    /// Mark field as unique, e.g., #[unique]
    pub unique: bool,
    /// Custom column name, e.g., #[column = "email_address"]
    pub column_name: Option<String>,
    /// Mark field as indexed, e.g., #[index]
    pub indexed: bool,
}

/// A single entity definition.
#[derive(Debug)]
pub struct EntityDef {
    pub attrs: EntityAttrs,
    pub name: Ident,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

/// A field within an entity.
#[derive(Debug)]
pub struct FieldDef {
    pub attrs: FieldAttrs,
    pub name: Ident,
    pub ty: RawFieldType,
    pub span: Span,
}

/// Raw field type before entity resolution.
/// At parse time, we don't know if a type like `User` is an entity or invalid.
#[derive(Debug)]
pub enum RawFieldType {
    /// A known scalar type (String, i32, etc.)
    Scalar { scalar: ScalarType, optional: bool },
    /// Vec<T> - will become has_many if T is an entity
    Vec { inner: Ident },
    /// T or Option<T> where T is unknown - needs resolution
    Unknown { name: Ident, optional: bool },
}

impl Parse for Schema {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut entities = Vec::new();

        while !input.is_empty() {
            entities.push(input.parse()?);
        }

        if entities.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "schema! macro requires at least one entity definition",
            ));
        }

        Ok(Schema { entities })
    }
}

impl Parse for EntityDef {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse entity attributes
        let attrs = parse_entity_attrs(input)?;

        let name: Ident = input.parse()?;
        let span = name.span();

        let content;
        braced!(content in input);

        let fields_punctuated: Punctuated<FieldDef, Token![,]> =
            content.parse_terminated(FieldDef::parse, Token![,])?;

        let fields: Vec<FieldDef> = fields_punctuated.into_iter().collect();

        // Check for reserved field names
        for field in &fields {
            let field_name = field.name.to_string();
            if is_reserved_field(&field_name) {
                return Err(syn::Error::new(
                    field.name.span(),
                    format!(
                        "field '{}' is reserved and automatically generated (id, created_at, updated_at)",
                        field_name
                    ),
                ));
            }
        }

        // Check for duplicate field names
        let mut seen_fields = std::collections::HashSet::new();
        for field in &fields {
            let field_name = field.name.to_string();
            if !seen_fields.insert(field_name.clone()) {
                return Err(syn::Error::new(
                    field.name.span(),
                    format!("duplicate field name '{}'", field_name),
                ));
            }
        }

        Ok(EntityDef {
            attrs,
            name,
            fields,
            span,
        })
    }
}

/// Parse entity-level attributes like #[table_name = "people"] or #[timestamps(created_at)]
fn parse_entity_attrs(input: ParseStream) -> Result<EntityAttrs> {
    let mut attrs = EntityAttrs::default();

    while input.peek(Token![#]) {
        input.parse::<Token![#]>()?;
        let content;
        syn::bracketed!(content in input);

        let attr_name: Ident = content.parse()?;
        let attr_name_str = attr_name.to_string();

        match attr_name_str.as_str() {
            "table_name" => {
                content.parse::<Token![=]>()?;
                let value: syn::LitStr = content.parse()?;
                attrs.table_name = Some(value.value());
            }
            "timestamps" => {
                // Parse timestamps(created_at) or timestamps(updated_at) or timestamps(none)
                let inner;
                syn::parenthesized!(inner in content);
                let ts_type: Ident = inner.parse()?;
                let ts_str = ts_type.to_string();

                match ts_str.as_str() {
                    "created_at" => {
                        attrs.has_created_at = true;
                        attrs.has_updated_at = false;
                    }
                    "updated_at" => {
                        attrs.has_created_at = false;
                        attrs.has_updated_at = true;
                    }
                    "none" => {
                        attrs.has_created_at = false;
                        attrs.has_updated_at = false;
                    }
                    _ => {
                        return Err(syn::Error::new(
                            ts_type.span(),
                            format!(
                                "unknown timestamps option '{}'. Supported: created_at, updated_at, none",
                                ts_str
                            ),
                        ));
                    }
                }
            }
            _ => {
                return Err(syn::Error::new(
                    attr_name.span(),
                    format!(
                        "unknown entity attribute '{}'. Supported: table_name, timestamps",
                        attr_name_str
                    ),
                ));
            }
        }
    }

    Ok(attrs)
}

impl Parse for FieldDef {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse field attributes
        let attrs = parse_field_attrs(input)?;

        let name: Ident = input.parse()?;
        let span = name.span();
        input.parse::<Token![:]>()?;
        let ty = parse_field_type(input)?;

        Ok(FieldDef {
            attrs,
            name,
            ty,
            span,
        })
    }
}

/// Parse field-level attributes like #[unique] or #[column = "email_address"]
fn parse_field_attrs(input: ParseStream) -> Result<FieldAttrs> {
    let mut attrs = FieldAttrs::default();

    while input.peek(Token![#]) {
        input.parse::<Token![#]>()?;
        let content;
        syn::bracketed!(content in input);

        let attr_name: Ident = content.parse()?;
        let attr_name_str = attr_name.to_string();

        match attr_name_str.as_str() {
            "unique" => {
                attrs.unique = true;
            }
            "index" => {
                attrs.indexed = true;
            }
            "column" => {
                content.parse::<Token![=]>()?;
                let value: syn::LitStr = content.parse()?;
                attrs.column_name = Some(value.value());
            }
            _ => {
                return Err(syn::Error::new(
                    attr_name.span(),
                    format!(
                        "unknown field attribute '{}'. Supported: unique, index, column",
                        attr_name_str
                    ),
                ));
            }
        }
    }

    Ok(attrs)
}

/// Parse a field type from the input stream.
fn parse_field_type(input: ParseStream) -> Result<RawFieldType> {
    // Check for Option<T>
    if input.peek(Ident) {
        let ident: Ident = input.parse()?;
        let ident_str = ident.to_string();

        if ident_str == "Option" {
            // Parse Option<T>
            input.parse::<Token![<]>()?;
            let inner_type = parse_inner_type(input)?;
            input.parse::<Token![>]>()?;

            return match inner_type {
                InnerType::Scalar(scalar) => Ok(RawFieldType::Scalar {
                    scalar,
                    optional: true,
                }),
                InnerType::Ident(name) => Ok(RawFieldType::Unknown {
                    name,
                    optional: true,
                }),
            };
        }

        if ident_str == "Vec" {
            // Parse Vec<T>
            input.parse::<Token![<]>()?;
            let inner: Ident = input.parse()?;
            input.parse::<Token![>]>()?;

            return Ok(RawFieldType::Vec { inner });
        }

        // Try to parse as scalar
        if let Some(scalar) = ScalarType::from_ident(&ident_str) {
            return Ok(RawFieldType::Scalar {
                scalar,
                optional: false,
            });
        }

        // Unknown type - might be an entity reference
        Ok(RawFieldType::Unknown {
            name: ident,
            optional: false,
        })
    } else {
        Err(syn::Error::new(input.span(), "expected type"))
    }
}

enum InnerType {
    Scalar(ScalarType),
    Ident(Ident),
}

fn parse_inner_type(input: ParseStream) -> Result<InnerType> {
    let ident: Ident = input.parse()?;
    let ident_str = ident.to_string();

    if let Some(scalar) = ScalarType::from_ident(&ident_str) {
        Ok(InnerType::Scalar(scalar))
    } else {
        Ok(InnerType::Ident(ident))
    }
}

/// Parse the schema from a token stream.
pub fn parse_schema(input: TokenStream) -> Result<Schema> {
    syn::parse2(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn test_parse_simple_entity() {
        let input = quote! {
            User {
                email: String,
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert_eq!(schema.entities.len(), 1);
        assert_eq!(schema.entities[0].name.to_string(), "User");
        assert_eq!(schema.entities[0].fields.len(), 2);
    }

    #[test]
    fn test_parse_multiple_entities() {
        let input = quote! {
            User {
                email: String,
            }

            Post {
                title: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert_eq!(schema.entities.len(), 2);
    }

    #[test]
    fn test_parse_vec_field() {
        let input = quote! {
            User {
                posts: Vec<Post>,
            }
        };

        let schema = parse_schema(input).unwrap();
        let field = &schema.entities[0].fields[0];
        assert!(matches!(field.ty, RawFieldType::Vec { .. }));
    }

    #[test]
    fn test_parse_option_field() {
        let input = quote! {
            Post {
                author: Option<User>,
            }
        };

        let schema = parse_schema(input).unwrap();
        let field = &schema.entities[0].fields[0];
        assert!(matches!(
            field.ty,
            RawFieldType::Unknown { optional: true, .. }
        ));
    }

    #[test]
    fn test_reserved_field_error() {
        let input = quote! {
            User {
                id: i32,
            }
        };

        let result = parse_schema(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("reserved"));
    }

    #[test]
    fn test_duplicate_field_error() {
        let input = quote! {
            User {
                email: String,
                email: String,
            }
        };

        let result = parse_schema(input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate"));
    }

    #[test]
    fn test_parse_table_name_attr() {
        let input = quote! {
            #[table_name = "people"]
            Person {
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert_eq!(
            schema.entities[0].attrs.table_name,
            Some("people".to_string())
        );
    }

    #[test]
    fn test_parse_unique_attr() {
        let input = quote! {
            User {
                #[unique]
                email: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(schema.entities[0].fields[0].attrs.unique);
    }

    #[test]
    fn test_parse_column_attr() {
        let input = quote! {
            User {
                #[column = "email_address"]
                email: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert_eq!(
            schema.entities[0].fields[0].attrs.column_name,
            Some("email_address".to_string())
        );
    }

    #[test]
    fn test_parse_multiple_field_attrs() {
        let input = quote! {
            User {
                #[unique]
                #[column = "user_email"]
                email: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        let field = &schema.entities[0].fields[0];
        assert!(field.attrs.unique);
        assert_eq!(field.attrs.column_name, Some("user_email".to_string()));
    }

    #[test]
    fn test_unknown_entity_attr_error() {
        let input = quote! {
            #[unknown_attr = "value"]
            User {
                email: String,
            }
        };

        let result = parse_schema(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown entity attribute")
        );
    }

    #[test]
    fn test_unknown_field_attr_error() {
        let input = quote! {
            User {
                #[unknown]
                email: String,
            }
        };

        let result = parse_schema(input);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown field attribute")
        );
    }

    #[test]
    fn test_parse_timestamps_created_at_only() {
        let input = quote! {
            #[timestamps(created_at)]
            User {
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(schema.entities[0].attrs.has_created_at);
        assert!(!schema.entities[0].attrs.has_updated_at);
    }

    #[test]
    fn test_parse_timestamps_updated_at_only() {
        let input = quote! {
            #[timestamps(updated_at)]
            User {
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(!schema.entities[0].attrs.has_created_at);
        assert!(schema.entities[0].attrs.has_updated_at);
    }

    #[test]
    fn test_parse_timestamps_none() {
        let input = quote! {
            #[timestamps(none)]
            User {
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(!schema.entities[0].attrs.has_created_at);
        assert!(!schema.entities[0].attrs.has_updated_at);
    }

    #[test]
    fn test_parse_index_attr() {
        let input = quote! {
            User {
                #[index]
                email: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(schema.entities[0].fields[0].attrs.indexed);
    }

    #[test]
    fn test_parse_combined_field_attrs() {
        let input = quote! {
            User {
                #[unique]
                #[index]
                #[column = "user_email"]
                email: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        let field = &schema.entities[0].fields[0];
        assert!(field.attrs.unique);
        assert!(field.attrs.indexed);
        assert_eq!(field.attrs.column_name, Some("user_email".to_string()));
    }

    #[test]
    fn test_default_timestamps_enabled() {
        let input = quote! {
            User {
                name: String,
            }
        };

        let schema = parse_schema(input).unwrap();
        assert!(schema.entities[0].attrs.has_created_at);
        assert!(schema.entities[0].attrs.has_updated_at);
    }
}
