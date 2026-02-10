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

/// A single entity definition.
#[derive(Debug)]
pub struct EntityDef {
    pub name: Ident,
    pub fields: Vec<FieldDef>,
    pub span: Span,
}

/// A field within an entity.
#[derive(Debug)]
pub struct FieldDef {
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

        Ok(EntityDef { name, fields, span })
    }
}

impl Parse for FieldDef {
    fn parse(input: ParseStream) -> Result<Self> {
        let name: Ident = input.parse()?;
        let span = name.span();
        input.parse::<Token![:]>()?;
        let ty = parse_field_type(input)?;

        Ok(FieldDef { name, ty, span })
    }
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
}
