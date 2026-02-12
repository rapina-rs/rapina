//! Semantic analysis for the schema macro.
//!
//! Two-pass analysis:
//! 1. Collect all entity names into a registry
//! 2. Resolve relationships and validate targets exist

use proc_macro2::Span;
use std::collections::HashSet;
use syn::{Ident, Result};

use super::parse::{EntityAttrs, EntityDef, FieldAttrs, FieldDef, RawFieldType, Schema};
use super::types::FieldType;

/// Analyzed schema with resolved relationships.
#[derive(Debug)]
pub struct AnalyzedSchema {
    pub entities: Vec<AnalyzedEntity>,
}

/// An entity with resolved field types.
#[derive(Debug)]
pub struct AnalyzedEntity {
    pub attrs: EntityAttrs,
    pub name: Ident,
    pub fields: Vec<AnalyzedField>,
    #[allow(dead_code)]
    pub span: Span,
}

/// A field with resolved type information.
#[derive(Debug)]
pub struct AnalyzedField {
    pub attrs: FieldAttrs,
    pub name: Ident,
    pub ty: FieldType,
    #[allow(dead_code)]
    pub span: Span,
}

/// Entity registry for cross-reference validation.
struct EntityRegistry {
    names: HashSet<String>,
}

impl EntityRegistry {
    fn new(entities: &[EntityDef]) -> Self {
        let names = entities.iter().map(|e| e.name.to_string()).collect();
        EntityRegistry { names }
    }

    fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }
}

/// Analyze a parsed schema, resolving relationships and validating references.
pub fn analyze_schema(schema: Schema) -> Result<AnalyzedSchema> {
    // Check for duplicate entity names
    let mut seen_entities = HashSet::new();
    for entity in &schema.entities {
        let entity_name = entity.name.to_string();
        if !seen_entities.insert(entity_name.clone()) {
            return Err(syn::Error::new(
                entity.name.span(),
                format!("duplicate entity name '{}'", entity_name),
            ));
        }
    }

    // Build entity registry for cross-reference
    let registry = EntityRegistry::new(&schema.entities);

    // Analyze each entity
    let mut analyzed_entities = Vec::new();
    for entity in schema.entities {
        analyzed_entities.push(analyze_entity(entity, &registry)?);
    }

    Ok(AnalyzedSchema {
        entities: analyzed_entities,
    })
}

fn analyze_entity(entity: EntityDef, registry: &EntityRegistry) -> Result<AnalyzedEntity> {
    let mut analyzed_fields = Vec::new();

    for field in entity.fields {
        analyzed_fields.push(analyze_field(field, registry)?);
    }

    Ok(AnalyzedEntity {
        attrs: entity.attrs,
        name: entity.name,
        fields: analyzed_fields,
        span: entity.span,
    })
}

fn analyze_field(field: FieldDef, registry: &EntityRegistry) -> Result<AnalyzedField> {
    let ty = match field.ty {
        RawFieldType::Scalar { scalar, optional } => FieldType::Scalar { scalar, optional },

        RawFieldType::Vec { inner } => {
            let inner_name = inner.to_string();

            // Vec<T> must reference an entity (has_many)
            if !registry.contains(&inner_name) {
                return Err(syn::Error::new(
                    inner.span(),
                    format!(
                        "unknown entity '{}' in Vec<{0}>. Did you define this entity?",
                        inner_name
                    ),
                ));
            }

            FieldType::HasMany { target: inner }
        }

        RawFieldType::Unknown { name, optional } => {
            let type_name = name.to_string();

            // If it's a known entity, it's a belongs_to relationship
            if registry.contains(&type_name) {
                FieldType::BelongsTo {
                    target: name,
                    optional,
                }
            } else {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "unknown type '{}'. Use a scalar type (String, i32, etc.) or reference a defined entity.",
                        type_name
                    ),
                ));
            }
        }
    };

    Ok(AnalyzedField {
        attrs: field.attrs,
        name: field.name,
        ty,
        span: field.span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::parse::parse_schema;
    use quote::quote;

    #[test]
    fn test_analyze_simple_schema() {
        let input = quote! {
            User {
                email: String,
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        assert_eq!(analyzed.entities.len(), 1);
        assert_eq!(analyzed.entities[0].fields.len(), 2);
    }

    #[test]
    fn test_analyze_has_many_relationship() {
        let input = quote! {
            User {
                posts: Vec<Post>,
            }

            Post {
                title: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let user = &analyzed.entities[0];
        assert!(matches!(user.fields[0].ty, FieldType::HasMany { .. }));
    }

    #[test]
    fn test_analyze_belongs_to_relationship() {
        let input = quote! {
            User {
                email: String,
            }

            Post {
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let post = &analyzed.entities[1];
        assert!(matches!(
            post.fields[0].ty,
            FieldType::BelongsTo {
                optional: false,
                ..
            }
        ));
    }

    #[test]
    fn test_analyze_optional_belongs_to() {
        let input = quote! {
            User {
                email: String,
            }

            Comment {
                author: Option<User>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let comment = &analyzed.entities[1];
        assert!(matches!(
            comment.fields[0].ty,
            FieldType::BelongsTo { optional: true, .. }
        ));
    }

    #[test]
    fn test_unknown_entity_in_vec_error() {
        let input = quote! {
            User {
                posts: Vec<UnknownEntity>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown entity"));
    }

    #[test]
    fn test_unknown_type_error() {
        let input = quote! {
            User {
                foo: UnknownType,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown type"));
    }

    #[test]
    fn test_duplicate_entity_error() {
        let input = quote! {
            User {
                email: String,
            }

            User {
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("duplicate entity"));
    }

    #[test]
    fn test_analyze_preserves_entity_attrs() {
        let input = quote! {
            #[table_name = "people"]
            Person {
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        assert_eq!(
            analyzed.entities[0].attrs.table_name,
            Some("people".to_string())
        );
    }

    #[test]
    fn test_analyze_preserves_field_attrs() {
        let input = quote! {
            User {
                #[unique]
                #[column = "user_email"]
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let field = &analyzed.entities[0].fields[0];
        assert!(field.attrs.unique);
        assert_eq!(field.attrs.column_name, Some("user_email".to_string()));
    }
}
