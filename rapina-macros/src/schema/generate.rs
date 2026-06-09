//! Code generation for SeaORM entity modules.

use heck::ToSnakeCase;
use proc_macro2::{Ident, TokenStream};
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

    let model_fields = generate_model_fields(entity, schema);
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
        generate_custom_pk_fields(entity, pk_cols, schema)
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

fn sea_orm_parts(field: &AnalyzedField) -> Vec<TokenStream> {
    let mut parts = Vec::new();
    if field.attrs.unique {
        parts.push(quote! {unique});
    }
    if field.attrs.indexed {
        parts.push(quote! {indexed});
    }
    if let Some(ref col_name) = field.attrs.column_name {
        parts.push(quote! {column_name = #col_name});
    }
    parts
}

fn build_field_attr(parts: &[TokenStream], column_type: Option<TokenStream>) -> TokenStream {
    match (parts.is_empty(), column_type) {
        (true, ct) => ct.unwrap_or_default(),
        (false, Some(ct)) => quote! {#[sea_orm(#(#parts), *)] #ct },
        (false, None) => quote! {#[sea_orm(#(#parts), *)]},
    }
}

fn generate_custom_pk_fields(
    entity: &AnalyzedEntity,
    pk_cols: &[String],
    schema: &AnalyzedSchema,
) -> TokenStream {
    let fields: Vec<TokenStream> = pk_cols
        .iter()
        .filter_map(|col_name| {
            let field = entity.fields.iter().find(|f| f.name == col_name)?;
            let field_name = &field.name;
            // A relationship PK resolves to the target's PK scalar, which carries
            // no column_type attribute of its own.
            let (rust_type, column_type) = match &field.ty {
                FieldType::Scalar { scalar, .. } => (scalar.rust_type(), scalar.column_type_attr()),
                FieldType::BelongsTo { target, .. } => {
                    (resolve_target_pk_type(target, schema), None)
                }
                FieldType::HasMany { .. } => return None,
            };

            let mut parts = vec![quote! {primary_key}, quote! {auto_increment = false}];
            parts.extend(sea_orm_parts(field));
            let field_attr = build_field_attr(&parts, column_type);

            Some(quote! {
                #field_attr
                pub #field_name: #rust_type,
            })
        })
        .collect();

    quote! { #(#fields)* }
}

fn resolve_target_pk_type(target: &Ident, schema: &AnalyzedSchema) -> TokenStream {
    resolve_target_pk_type_inner(target, schema, &mut Vec::new())
}

fn resolve_target_pk_type_inner(
    target: &Ident,
    schema: &AnalyzedSchema,
    visiting: &mut Vec<String>,
) -> TokenStream {
    let target_name = target.to_string();
    // Guard against self-referential or mutually-referential relationship PKs,
    // which would otherwise recurse forever during macro expansion. Falling back
    // to i32 keeps expansion terminating; such a cyclic PK has no scalar anchor.
    if visiting.contains(&target_name) {
        return quote! { i32 };
    }

    let resolved = schema
        .entities
        .iter()
        .find(|e| &e.name == target)
        .and_then(|e| {
            if let Some(ref pk_cols) = e.attrs.primary_key {
                if pk_cols.len() == 1 {
                    let pk_field = e.fields.iter().find(|f| f.name == pk_cols[0])?;
                    match &pk_field.ty {
                        FieldType::Scalar { scalar, .. } => return Some(scalar.rust_type()),
                        // The target's PK is itself a relationship (a join-table
                        // style PK). Resolve transitively to the entity it points
                        // at so the FK column adopts the underlying scalar type.
                        FieldType::BelongsTo {
                            target: inner_target,
                            ..
                        } => {
                            visiting.push(target_name.clone());
                            let ty = resolve_target_pk_type_inner(inner_target, schema, visiting);
                            visiting.pop();
                            return Some(ty);
                        }
                        FieldType::HasMany { .. } => {}
                    }
                }
            } else {
                // Default PK is i32
                return Some(quote! { i32 });
            }
            None
        });

    // Fallback to i32 if target not found or complex PK.
    resolved.unwrap_or_else(|| quote! { i32 })
}

/// PascalCase name of the target entity's single primary-key column, used for
/// the `to = "...::Column::X"` side of a generated belongs_to relation.
/// Defaults to `Id` for entities using the implicit auto-increment primary key.
fn resolve_target_pk_column(target: &Ident, schema: &AnalyzedSchema) -> String {
    schema
        .entities
        .iter()
        .find(|e| &e.name == target)
        .and_then(|e| {
            let pk_cols = e.attrs.primary_key.as_ref()?;
            if pk_cols.len() == 1 {
                Some(to_pascal_case(&pk_cols[0]))
            } else {
                None
            }
        })
        .unwrap_or_else(|| "Id".to_string())
}

fn generate_model_fields(entity: &AnalyzedEntity, schema: &AnalyzedSchema) -> TokenStream {
    let pk_cols = entity.attrs.primary_key.as_deref().unwrap_or_default();

    let fields: Vec<TokenStream> = entity
        .fields
        .iter()
        .filter(|f| !pk_cols.iter().any(|pk| pk == &f.name.to_string()))
        .filter_map(|f| generate_model_field(f, schema))
        .collect();

    quote! {
        #(#fields)*
    }
}

fn generate_model_field(field: &AnalyzedField, schema: &AnalyzedSchema) -> Option<TokenStream> {
    let field_name = &field.name;

    match &field.ty {
        FieldType::Scalar { scalar, optional } => {
            let rust_type = scalar.rust_type();
            let final_type = if *optional {
                quote! { Option<#rust_type> }
            } else {
                rust_type
            };
            let field_attr = build_field_attr(&sea_orm_parts(field), scalar.column_type_attr());

            Some(quote! {
                #field_attr
                pub #field_name: #final_type,
            })
        }

        FieldType::BelongsTo { target, optional } => {
            // Generate foreign key column: author -> author_id
            let fk_name = format_ident!("{}_id", field_name.to_string().to_snake_case());

            // Look up target entity's primary key type
            let target_pk_type = resolve_target_pk_type(target, schema);

            if *optional {
                Some(quote! {
                    pub #fk_name: Option<#target_pk_type>,
                })
            } else {
                Some(quote! {
                    pub #fk_name: #target_pk_type,
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
    entity: &AnalyzedEntity,
    schema: &AnalyzedSchema,
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
            let field_name = field.name.to_string();
            let is_pk_column = entity
                .attrs
                .primary_key
                .as_ref()
                .is_some_and(|pk_cols| pk_cols.iter().any(|pk| pk == &field_name));
            let fk_column = if is_pk_column {
                field_name
            } else {
                format!("{}_id", field.name.to_string().to_snake_case())
            };
            let fk_column_str = format!("Column::{}", to_pascal_case(&fk_column));
            let to_column_str = format!(
                "super::{}::Column::{}",
                target_mod_str,
                resolve_target_pk_column(target, schema)
            );

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
    fn test_generate_composite_primary_key_with_belongs_to_fields() {
        let input = quote! {
            #[table_name = "transactions"]
            Tx {
                name: String,
            }

            #[table_name = "labels"]
            Label {
                #[unique]
                name: String,
            }

            #[table_name = "transaction_labels"]
            #[timestamps(none)]
            #[primary_key(tx_id, label_id)]
            TxLabel {
                tx_id: Tx,
                label_id: Label,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();
        let tx_label_start = output.find("pub mod tx_label").unwrap();
        let reexports_start = output.find("pub use tx :: Entity").unwrap();
        let tx_label_module = &output[tx_label_start..reexports_start];

        assert!(tx_label_module.contains("pub tx_id : i32"));
        assert!(tx_label_module.contains("pub label_id : i32"));
        assert!(tx_label_module.contains("auto_increment = false"));
        assert!(!tx_label_module.contains("pub id : i32"));
        assert!(!tx_label_module.contains("pub tx_id_id"));
        assert!(!tx_label_module.contains("pub label_id_id"));
        assert!(tx_label_module.contains("from = \"Column::TxId\""));
        assert!(tx_label_module.contains("from = \"Column::LabelId\""));
        assert!(!tx_label_module.contains("Column::TxIdId"));
        assert!(!tx_label_module.contains("Column::LabelIdId"));
        // Both targets use the default auto-increment `id` PK, so the relation
        // points at Column::Id.
        assert!(tx_label_module.contains("to = \"super::tx::Column::Id\""));
        assert!(tx_label_module.contains("to = \"super::label::Column::Id\""));
    }

    #[test]
    fn test_generate_belongs_to_pk_targets_non_id_pk_column() {
        // The target's single PK column is not named `id`; the relation `to`
        // side must reference that actual column, not a nonexistent Column::Id.
        let input = quote! {
            #[primary_key(uuid_pk)]
            Label {
                uuid_pk: Uuid,
                name: String,
            }

            #[timestamps(none)]
            #[primary_key(label_id)]
            LabelRef {
                label_id: Label,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();
        let label_ref_start = output.find("pub mod label_ref").unwrap();
        let reexports_start = output.find("pub use label :: Entity").unwrap();
        let label_ref_module = &output[label_ref_start..reexports_start];

        // FK column adopts the target PK scalar type and verbatim name.
        assert!(label_ref_module.contains("pub label_id : rapina :: uuid :: Uuid"));
        // Relation references the target's real PK column, not Column::Id.
        assert!(label_ref_module.contains("from = \"Column::LabelId\""));
        assert!(label_ref_module.contains("to = \"super::label::Column::UuidPk\""));
        assert!(!label_ref_module.contains("super::label::Column::Id"));
    }

    #[test]
    fn test_generate_belongs_to_pk_resolves_transitively() {
        // A relation to an entity whose own PK is a belongs_to must resolve the
        // FK scalar type transitively, not fall back to i32.
        let input = quote! {
            #[primary_key(id)]
            Org {
                id: Uuid,
                name: String,
            }

            #[timestamps(none)]
            #[primary_key(org_id)]
            Project {
                org_id: Org,
            }

            Task {
                project: Project,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // Project.org_id resolves to Org's Uuid PK.
        assert!(output.contains("pub org_id : rapina :: uuid :: Uuid"));
        // Task.project_id resolves transitively through Project's belongs_to PK.
        assert!(output.contains("pub project_id : rapina :: uuid :: Uuid"));
        assert!(!output.contains("pub project_id : i32"));
    }

    #[test]
    fn test_generate_self_referential_belongs_to_pk_terminates() {
        // A relationship PK that points back at its own entity has no scalar
        // anchor; generation must terminate (falling back to i32) rather than
        // recursing forever during macro expansion.
        let input = quote! {
            #[timestamps(none)]
            #[primary_key(parent)]
            Category {
                parent: Category,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        assert!(output.contains("pub parent : i32"));
        assert!(output.contains("auto_increment = false"));
    }

    #[test]
    fn test_generate_belongs_to_primary_key_uses_target_uuid_type() {
        let input = quote! {
            #[primary_key(id)]
            Tx {
                id: Uuid,
                name: String,
            }

            #[timestamps(none)]
            #[primary_key(tx_id)]
            TxLabel {
                tx_id: Tx,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();
        let tx_label_start = output.find("pub mod tx_label").unwrap();
        let reexports_start = output.find("pub use tx :: Entity").unwrap();
        let tx_label_module = &output[tx_label_start..reexports_start];

        assert!(tx_label_module.contains("pub tx_id : rapina :: uuid :: Uuid"));
        assert!(!tx_label_module.contains("pub tx_id_id"));
        assert!(tx_label_module.contains("from = \"Column::TxId\""));
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
    fn test_generate_custom_pk_keeps_column_type() {
        let input = quote! {
            #[primary_key(code)]
            #[timestamps(none)]
            Region {
                code: Text,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let output = generate_schema(analyzed).to_string();

        assert!(output.contains("column_type = \"Text\""));
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

    #[test]
    fn test_generate_belongs_to_uuid_pk() {
        let input = quote! {
            #[primary_key(id)]
            Organization {
                id: Uuid,
                name: String,
            }

            User {
                name: String,
                org: Organization,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // Organization should have id: Uuid
        assert!(output.contains("pub id : rapina :: uuid :: Uuid"));
        // User should have org_id: Uuid (resolved from Organization's PK)
        assert!(output.contains("pub org_id : rapina :: uuid :: Uuid"));
    }

    #[test]
    fn test_schema_macro_uuid_pk_integration() {
        use crate::schema::schema_impl;

        let input = quote! {
            #[primary_key(id)]
            Organization {
                id: Uuid,
                name: String,
            }

            User {
                name: String,
                org: Organization,
            }
        };

        let output = schema_impl(input);
        let output_str = output.to_string();

        // Verify PK in first entity
        assert!(output_str.contains("pub id : rapina :: uuid :: Uuid"));
        // Verify PK attribute
        assert!(output_str.contains("primary_key"));

        // Verify FK in second entity correctly resolved to Uuid
        assert!(output_str.contains("pub org_id : rapina :: uuid :: Uuid"));
    }

    #[test]
    fn test_generate_belongs_to_chained_uuid_pk() {
        let input = quote! {
            #[primary_key(id)]
            Organization {
                id: Uuid,
                name: String,
            }

            #[primary_key(id)]
            Project {
                id: Uuid,
                name: String,
                org: Organization,
            }

            Task {
                name: String,
                project: Project,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();
        let generated = generate_schema(analyzed);
        let output = generated.to_string();

        // Project.org_id should be Uuid
        assert!(output.contains("pub org_id : rapina :: uuid :: Uuid"));
        // Task.project_id should be Uuid (resolved from Project's PK)
        assert!(output.contains("pub project_id : rapina :: uuid :: Uuid"));
    }
}
