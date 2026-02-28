//! Code generation for SeaORM entity modules.

use heck::ToSnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use super::analyze::{AnalyzedEntity, AnalyzedField, AnalyzedSchema};
use super::types::{FieldType, ScalarType};

/// Generate the complete schema code from analyzed entities.
pub fn generate_schema(schema: AnalyzedSchema) -> TokenStream {
    let entity_modules: Vec<TokenStream> = schema
        .entities
        .iter()
        .map(|entity| generate_entity_module(entity, &schema))
        .collect();

    // Generate re-exports: pub use user::Entity as User;
    let reexports: Vec<TokenStream> = schema
        .entities
        .iter()
        .map(|entity| {
            let mod_name = format_ident!("{}", entity.name.to_string().to_snake_case());
            let entity_name = &entity.name;
            quote! {
                pub use #mod_name::Entity as #entity_name;
            }
        })
        .collect();

    quote! {
        #(#entity_modules)*
        #(#reexports)*
    }
}

fn generate_entity_module(entity: &AnalyzedEntity, schema: &AnalyzedSchema) -> TokenStream {
    let mod_name = format_ident!("{}", entity.name.to_string().to_snake_case());

    // Use custom table name if provided, otherwise auto-pluralize
    let table_name = entity
        .attrs
        .table_name
        .clone()
        .unwrap_or_else(|| format!("{}s", entity.name.to_string().to_snake_case()));

    let model_fields = generate_model_fields(entity);
    let relation_variants = generate_relation_variants(entity, schema);
    let related_impls = generate_related_impls(entity, schema);

    // Generate timestamp fields based on entity attrs
    let created_at_field = if entity.attrs.has_created_at {
        quote! { pub created_at: DateTimeUtc, }
    } else {
        quote! {}
    };

    let updated_at_field = if entity.attrs.has_updated_at {
        quote! { pub updated_at: DateTimeUtc, }
    } else {
        quote! {}
    };

    // f32/f64 don't implement Eq, so omit it when model has float fields
    let has_floats = entity.fields.iter().any(|f| {
        matches!(
            &f.ty,
            FieldType::Scalar {
                scalar: ScalarType::F32 | ScalarType::F64,
                ..
            }
        )
    });

    let derive_attr = if has_floats {
        quote! { #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, JsonSchema)] }
    } else {
        quote! { #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize, JsonSchema)] }
    };

    // Generate primary key fields
    let pk_fields = if let Some(ref pk_cols) = entity.attrs.primary_key {
        // Custom primary key: mark specified fields with #[sea_orm(primary_key, auto_increment = false)]
        generate_custom_pk_fields(entity, pk_cols)
    } else {
        // Default: auto-increment id
        quote! {
            #[sea_orm(primary_key)]
            pub id: i32,
        }
    };

    quote! {
        pub mod #mod_name {
            use rapina::sea_orm;
            use sea_orm::entity::prelude::*;
            use serde::{Deserialize, Serialize};
            use rapina::schemars::{self, JsonSchema};

            #derive_attr
            #[sea_orm(table_name = #table_name)]
            pub struct Model {
                #pk_fields
                #model_fields
                #created_at_field
                #updated_at_field
            }

            #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
            pub enum Relation {
                #relation_variants
            }

            #related_impls

            impl ActiveModelBehavior for ActiveModel {}
        }
    }
}

fn generate_custom_pk_fields(entity: &AnalyzedEntity, pk_cols: &[String]) -> TokenStream {
    let fields: Vec<TokenStream> = pk_cols
        .iter()
        .filter_map(|col_name| {
            let field = entity.fields.iter().find(|f| f.name == col_name)?;
            if let FieldType::Scalar { scalar, .. } = &field.ty {
                let field_name = &field.name;
                let rust_type = scalar.rust_type();
                Some(quote! {
                    #[sea_orm(primary_key, auto_increment = false)]
                    pub #field_name: #rust_type,
                })
            } else {
                None
            }
        })
        .collect();

    quote! { #(#fields)* }
}

fn generate_model_fields(entity: &AnalyzedEntity) -> TokenStream {
    let pk_cols = entity.attrs.primary_key.as_deref().unwrap_or_default();

    let fields: Vec<TokenStream> = entity
        .fields
        .iter()
        .filter(|f| !pk_cols.iter().any(|pk| pk == &f.name.to_string()))
        .filter_map(generate_model_field)
        .collect();

    quote! {
        #(#fields)*
    }
}

fn generate_model_field(field: &AnalyzedField) -> Option<TokenStream> {
    let field_name = &field.name;

    match &field.ty {
        FieldType::Scalar { scalar, optional } => {
            let rust_type = scalar.rust_type();
            let column_type_attr = scalar.column_type_attr();

            let final_type = if *optional {
                quote! { Option<#rust_type> }
            } else {
                rust_type
            };

            // Build sea_orm attribute parts
            let mut sea_orm_parts: Vec<TokenStream> = Vec::new();

            // Add unique if specified
            if field.attrs.unique {
                sea_orm_parts.push(quote! { unique });
            }

            // Add indexed if specified
            if field.attrs.indexed {
                sea_orm_parts.push(quote! { indexed });
            }

            // Add custom column name if specified
            if let Some(ref col_name) = field.attrs.column_name {
                sea_orm_parts.push(quote! { column_name = #col_name });
            }

            // Combine column_type_attr with other attributes
            let field_attr = if sea_orm_parts.is_empty() {
                column_type_attr.unwrap_or_default()
            } else if let Some(col_type) = column_type_attr {
                // Extract the column_type value and combine
                let col_type_str = col_type.to_string();
                if col_type_str.contains("column_type") {
                    // Parse out the column_type value
                    let combined = quote! {
                        #[sea_orm(#(#sea_orm_parts),*)]
                        #col_type
                    };
                    combined
                } else {
                    quote! { #[sea_orm(#(#sea_orm_parts),*)] }
                }
            } else {
                quote! { #[sea_orm(#(#sea_orm_parts),*)] }
            };

            Some(quote! {
                #field_attr
                pub #field_name: #final_type,
            })
        }

        FieldType::BelongsTo {
            target: _,
            optional,
        } => {
            // Generate foreign key column: author -> author_id
            let fk_name = format_ident!("{}_id", field_name.to_string().to_snake_case());

            if *optional {
                Some(quote! {
                    pub #fk_name: Option<i32>,
                })
            } else {
                Some(quote! {
                    pub #fk_name: i32,
                })
            }
        }

        FieldType::HasMany { .. } => {
            // has_many doesn't generate a column, just a relation
            None
        }
    }
}

fn generate_relation_variants(entity: &AnalyzedEntity, schema: &AnalyzedSchema) -> TokenStream {
    let variants: Vec<TokenStream> = entity
        .fields
        .iter()
        .filter_map(|field| generate_relation_variant(field, entity, schema))
        .collect();

    quote! {
        #(#variants)*
    }
}

fn generate_relation_variant(
    field: &AnalyzedField,
    _entity: &AnalyzedEntity,
    _schema: &AnalyzedSchema,
) -> Option<TokenStream> {
    match &field.ty {
        FieldType::HasMany { target } => {
            let variant_name = to_pascal_case(&field.name.to_string());
            let variant_ident = format_ident!("{}", variant_name);
            let target_mod_str = target.to_string().to_snake_case();
            let has_many_path = format!("super::{}::Entity", target_mod_str);

            Some(quote! {
                #[sea_orm(has_many = #has_many_path)]
                #variant_ident,
            })
        }

        FieldType::BelongsTo {
            target,
            optional: _,
        } => {
            let variant_name = to_pascal_case(&field.name.to_string());
            let variant_ident = format_ident!("{}", variant_name);
            let target_mod_str = target.to_string().to_snake_case();
            let belongs_to_path = format!("super::{}::Entity", target_mod_str);
            let fk_column_str = format!(
                "Column::{}",
                to_pascal_case(&format!("{}_id", field.name.to_string().to_snake_case()))
            );
            let to_column_str = format!("super::{}::Column::Id", target_mod_str);

            Some(quote! {
                #[sea_orm(
                    belongs_to = #belongs_to_path,
                    from = #fk_column_str,
                    to = #to_column_str
                )]
                #variant_ident,
            })
        }

        FieldType::Scalar { .. } => None,
    }
}

fn generate_related_impls(entity: &AnalyzedEntity, _schema: &AnalyzedSchema) -> TokenStream {
    let impls: Vec<TokenStream> = entity
        .fields
        .iter()
        .filter_map(generate_related_impl)
        .collect();

    quote! {
        #(#impls)*
    }
}

fn generate_related_impl(field: &AnalyzedField) -> Option<TokenStream> {
    let variant_name = to_pascal_case(&field.name.to_string());
    let variant_ident = format_ident!("{}", variant_name);

    match &field.ty {
        FieldType::HasMany { target } | FieldType::BelongsTo { target, .. } => {
            let target_mod = format_ident!("{}", target.to_string().to_snake_case());

            Some(quote! {
                impl Related<super::#target_mod::Entity> for Entity {
                    fn to() -> RelationDef {
                        Relation::#variant_ident.def()
                    }
                }
            })
        }
        FieldType::Scalar { .. } => None,
    }
}

/// Convert snake_case or camelCase to PascalCase.
fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::analyze::analyze_schema;
    use crate::schema::parse::parse_schema;
    use quote::quote;

    #[test]
    fn test_generate_simple_entity() {
        let input = quote! {
            User {
                email: String,
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("pub mod user"));
        assert!(output.contains("table_name = \"users\""));
        assert!(output.contains("pub email : String"));
        assert!(output.contains("pub name : String"));
        assert!(output.contains("pub id : i32"));
        assert!(output.contains("pub created_at : DateTimeUtc"));
        assert!(output.contains("pub updated_at : DateTimeUtc"));
    }

    #[test]
    fn test_generate_text_column() {
        let input = quote! {
            Post {
                content: Text,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("column_type = \"Text\""));
        assert!(output.contains("pub content : String"));
    }

    #[test]
    fn test_generate_belongs_to() {
        let input = quote! {
            User {
                email: String,
            }

            Post {
                title: String,
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("pub author_id : i32"));
        assert!(output.contains("belongs_to = \"super::user::Entity\""));
        assert!(output.contains("from = \"Column::AuthorId\""));
        assert!(output.contains("to = \"super::user::Column::Id\""));
    }

    #[test]
    fn test_generate_has_many() {
        let input = quote! {
            User {
                email: String,
                posts: Vec<Post>,
            }

            Post {
                title: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("has_many = \"super::post::Entity\""));
        assert!(output.contains("impl Related < super :: post :: Entity >"));
    }

    #[test]
    fn test_generate_optional_belongs_to() {
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
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("pub author_id : Option < i32 >"));
    }

    #[test]
    fn test_generate_custom_table_name() {
        let input = quote! {
            #[table_name = "people"]
            Person {
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("table_name = \"people\""));
        assert!(!output.contains("table_name = \"persons\""));
    }

    #[test]
    fn test_generate_unique_field() {
        let input = quote! {
            User {
                #[unique]
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("unique"));
    }

    #[test]
    fn test_generate_custom_column_name() {
        let input = quote! {
            User {
                #[column = "user_email"]
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("column_name = \"user_email\""));
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("hello_world"), "HelloWorld");
        assert_eq!(to_pascal_case("user"), "User");
        assert_eq!(to_pascal_case("author_id"), "AuthorId");
    }

    #[test]
    fn test_generate_no_timestamps() {
        let input = quote! {
            #[timestamps(none)]
            User {
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(!output.contains("created_at"));
        assert!(!output.contains("updated_at"));
    }

    #[test]
    fn test_generate_only_created_at() {
        let input = quote! {
            #[timestamps(created_at)]
            User {
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("created_at"));
        assert!(!output.contains("updated_at"));
    }

    #[test]
    fn test_generate_only_updated_at() {
        let input = quote! {
            #[timestamps(updated_at)]
            User {
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(!output.contains("created_at"));
        assert!(output.contains("updated_at"));
    }

    #[test]
    fn test_generate_indexed_field() {
        let input = quote! {
            User {
                #[index]
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("indexed"));
    }

    #[test]
    fn test_generate_float_field_omits_eq() {
        let input = quote! {
            Measurement {
                value: f32,
                label: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("PartialEq"));
        assert!(!output.contains("PartialEq , Eq"));
    }

    #[test]
    fn test_generate_no_float_field_includes_eq() {
        let input = quote! {
            User {
                name: String,
                age: i32,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("PartialEq , Eq"));
    }

    #[test]
    fn test_generate_composite_primary_key() {
        let input = quote! {
            #[table_name = "users_roles"]
            #[primary_key(user_id, role_id)]
            #[timestamps(none)]
            UsersRole {
                user_id: i32,
                role_id: i32,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // Should NOT have auto-generated id field
        assert!(!output.contains("pub id : i32"));
        // Should have PK attributes on both columns
        assert!(output.contains("primary_key"));
        assert!(output.contains("auto_increment = false"));
        assert!(output.contains("pub user_id : i32"));
        assert!(output.contains("pub role_id : i32"));
        // Should use custom table name
        assert!(output.contains("table_name = \"users_roles\""));
        // Should NOT have timestamps
        assert!(!output.contains("created_at"));
        assert!(!output.contains("updated_at"));
    }

    #[test]
    fn test_generate_composite_pk_with_extra_fields() {
        let input = quote! {
            #[primary_key(user_id, role_id)]
            #[timestamps(none)]
            UsersRole {
                user_id: i32,
                role_id: i32,
                assigned_by: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // PK fields should have the primary_key attribute
        assert!(output.contains("pub user_id : i32"));
        assert!(output.contains("pub role_id : i32"));
        // Non-PK field should be present without PK attribute
        assert!(output.contains("pub assigned_by : String"));
    }

    #[test]
    fn test_generate_single_custom_pk() {
        let input = quote! {
            #[primary_key(uuid_pk)]
            #[timestamps(none)]
            LegacyItem {
                uuid_pk: Uuid,
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(!output.contains("pub id : i32"));
        assert!(output.contains("auto_increment = false"));
        assert!(output.contains("pub uuid_pk"));
        assert!(output.contains("pub name : String"));
    }

    #[test]
    fn test_generate_default_pk_unchanged() {
        // Entities without #[primary_key] should still get auto id
        let input = quote! {
            User {
                name: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("# [sea_orm (primary_key)]"));
        assert!(output.contains("pub id : i32"));
    }

    #[test]
    fn test_generate_composite_pk_preserves_field_order() {
        let input = quote! {
            #[primary_key(b_id, a_id)]
            #[timestamps(none)]
            JoinTable {
                b_id: i32,
                a_id: i32,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // PK fields should appear in the order specified in #[primary_key(...)]
        let b_pos = output.find("pub b_id").unwrap();
        let a_pos = output.find("pub a_id").unwrap();
        assert!(b_pos < a_pos, "b_id should come before a_id in the output");
    }
}
