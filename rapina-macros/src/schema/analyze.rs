//! Semantic analysis for the schema macro.
//!
//! Two-pass analysis:
//! 1. Collect all entity names into a registry
//! 2. Resolve relationships and validate targets exist

use proc_macro2::Span;
use std::collections::{BTreeMap, HashSet};
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

    pub implement_related: bool,
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

    let analyzed = AnalyzedSchema {
        entities: analyzed_entities,
    };

    validate_relationship_primary_keys(&analyzed)?;

    Ok(analyzed)
}

fn validate_relationship_primary_keys(schema: &AnalyzedSchema) -> Result<()> {
    for entity in &schema.entities {
        let Some(pk_cols) = &entity.attrs.primary_key else {
            continue;
        };
        if pk_cols.len() != 1 {
            continue;
        }

        let Some(field) = entity.fields.iter().find(|field| field.name == pk_cols[0]) else {
            continue;
        };
        let FieldType::BelongsTo { target, .. } = &field.ty else {
            continue;
        };

        let mut visiting = HashSet::from([entity.name.to_string()]);
        if primary_key_relationship_has_cycle(target, schema, &mut visiting) {
            return Err(syn::Error::new(
                field.name.span(),
                format!(
                    "schema relationship `{}.{}` forms a primary key cycle; primary-key belongs_to relationships must resolve to a scalar primary key column",
                    entity.name, field.name
                ),
            ));
        }
    }

    for entity in &schema.entities {
        for field in &entity.fields {
            let FieldType::BelongsTo { target, .. } = &field.ty else {
                continue;
            };

            let Some(target_entity) = schema.entities.iter().find(|e| &e.name == target) else {
                continue;
            };
            let Some(pk_cols) = &target_entity.attrs.primary_key else {
                continue;
            };

            if pk_cols.len() > 1 {
                let pk_list = pk_cols
                    .iter()
                    .map(|col| format!("`{}`", col))
                    .collect::<Vec<_>>()
                    .join(", ");

                return Err(syn::Error::new(
                    field.name.span(),
                    format!(
                        "schema relationship `{}.{}` targets `{}` with composite primary key ({}); belongs_to relationships currently require a target with a single primary key column",
                        entity.name, field.name, target_entity.name, pk_list
                    ),
                ));
            }
        }
    }

    Ok(())
}

fn primary_key_relationship_has_cycle(
    target: &Ident,
    schema: &AnalyzedSchema,
    visiting: &mut HashSet<String>,
) -> bool {
    let target_name = target.to_string();
    if !visiting.insert(target_name.clone()) {
        return true;
    }

    let has_cycle = schema
        .entities
        .iter()
        .find(|entity| entity.name == target_name)
        .and_then(|entity| {
            let pk_cols = entity.attrs.primary_key.as_ref()?;
            if pk_cols.len() != 1 {
                return None;
            }

            let pk_field = entity
                .fields
                .iter()
                .find(|field| field.name == pk_cols[0])?;
            let FieldType::BelongsTo { target, .. } = &pk_field.ty else {
                return None;
            };

            Some(primary_key_relationship_has_cycle(target, schema, visiting))
        })
        .unwrap_or(false);

    visiting.remove(&target_name);
    has_cycle
}

fn analyze_entity(entity: EntityDef, registry: &EntityRegistry) -> Result<AnalyzedEntity> {
    // Reject created_at/updated_at only when they'd collide with auto-generated timestamps
    for field in &entity.fields {
        let name = field.name.to_string();
        if name == "created_at" && entity.attrs.has_created_at {
            return Err(syn::Error::new(
                field.name.span(),
                "field 'created_at' is auto-generated. Use #[timestamps(none)] or #[timestamps(updated_at)] to declare it manually",
            ));
        }
        if name == "updated_at" && entity.attrs.has_updated_at {
            return Err(syn::Error::new(
                field.name.span(),
                "field 'updated_at' is auto-generated. Use #[timestamps(none)] or #[timestamps(created_at)] to declare it manually",
            ));
        }
    }

    let mut analyzed_fields = Vec::new();

    for field in entity.fields {
        analyzed_fields.push(analyze_field(field, registry)?);
    }

    validate_relation_rules(&entity.name, &mut analyzed_fields)?;

    // Validate custom primary key columns exist in the entity
    if let Some(ref pk_cols) = entity.attrs.primary_key {
        let field_names: HashSet<String> =
            analyzed_fields.iter().map(|f| f.name.to_string()).collect();

        for col in pk_cols {
            if !field_names.contains(col) {
                return Err(syn::Error::new(
                    entity.name.span(),
                    format!(
                        "primary_key column '{}' does not exist in entity '{}'",
                        col, entity.name
                    ),
                ));
            }
        }

        // Validate PK columns are database columns. A (required) belongs_to field
        // is allowed because it generates a non-null FK column using the target
        // entity's PK type. A has_many field has no column, and an optional
        // belongs_to would produce a nullable column, neither of which can be a
        // primary key.
        for field in &analyzed_fields {
            let fname = field.name.to_string();
            if !pk_cols.contains(&fname) {
                continue;
            }
            match &field.ty {
                FieldType::HasMany { .. } => {
                    return Err(syn::Error::new(
                        field.name.span(),
                        format!(
                            "primary_key column '{}' cannot be a has_many relationship",
                            fname
                        ),
                    ));
                }
                FieldType::BelongsTo { optional: true, .. } => {
                    return Err(syn::Error::new(
                        field.name.span(),
                        format!(
                            "primary_key column '{}' cannot be an optional relationship; a primary key cannot be nullable",
                            fname
                        ),
                    ));
                }
                _ => {}
            }
        }
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
        implement_related: false,
    })
}

/// Decide which fields on one entity own a `Related` impl.
fn validate_relation_rules(entity: &Ident, fields: &mut [AnalyzedField]) -> Result<()> {
    validate_related_attr_placement(fields)?;

    let mut error: Option<syn::Error> = None;

    for (target, group) in group_fk_fields_by_target(fields) {
        match validate_target_relations(entity, &target, &group, fields) {
            Ok(winner) => fields[winner].implement_related = true,
            Err(e) => match error {
                Some(ref mut acc) => acc.combine(e),
                None => error = Some(e),
            },
        }
    }

    match error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Group an entity's relationship fields by the entity they target.
///
/// Ordered by target name so an entity with several bad groups reports its
/// errors the same way on every compile.
///
/// Members are returned as indices rather than references so the result borrows
/// nothing: the caller stays free to read `fields` while validating a group and
/// to take `&mut` on it afterwards to grant `implement_related` to the winner.
fn group_fk_fields_by_target(fields: &[AnalyzedField]) -> BTreeMap<String, Vec<usize>> {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();

    for (index, field) in fields.iter().enumerate() {
        let target = match &field.ty {
            FieldType::BelongsTo { target, .. } | FieldType::HasMany { target } => {
                target.to_string()
            }
            FieldType::Scalar { .. } => continue,
        };

        groups.entry(target).or_default().push(index);
    }

    groups
}

/// Decide which field in one target group owns `Related`, returning its index.
fn validate_target_relations(
    entity: &Ident,
    target: &str,
    members: &[usize],
    fields: &[AnalyzedField],
) -> Result<usize> {
    // One field pointing at this target: nothing to disambiguate, whichever
    // kind it is.
    if let [only] = members {
        return Ok(*only);
    }

    let (belongs_to, has_many): (Vec<usize>, Vec<usize>) = members
        .iter()
        .copied()
        .partition(|&i| matches!(fields[i].ty, FieldType::BelongsTo { .. }));

    // Well-formedness first: two has_many to one target stay ambiguous whichever
    // field ends up owning `Related`, so choosing an owner cannot rescue them.
    validate_has_many_group(entity, target, &has_many, fields)?;

    validate_belongs_to_group(entity, target, &belongs_to, fields)
}

/// Two `has_many` to one target are indistinguishable: each expands to
/// `R::to().rev()`, the *target's* back-edge, so they produce identical
/// `RelationDef`s regardless of which field wins the nomination. Reject them
/// rather than emit two differently-named links that run the same query.
fn validate_has_many_group(
    entity: &Ident,
    target: &str,
    has_many: &[usize],
    fields: &[AnalyzedField],
) -> Result<()> {
    if has_many.len() < 2 {
        return Ok(());
    }

    Err(syn::Error::new(
        entity.span(),
        format!(
            "entity '{}' has {} has_many fields referencing '{}' ({}); this is not supported yet — SeaORM cannot distinguish them without an explicit foreign key",
            entity,
            has_many.len(),
            target,
            field_list(has_many, fields),
        ),
    ))
}

/// Exactly one `belongs_to` owns `Related` for this target.
fn validate_belongs_to_group(
    entity: &Ident,
    target: &str,
    belongs_to: &[usize],
    fields: &[AnalyzedField],
) -> Result<usize> {
    if let [only] = belongs_to {
        return Ok(*only);
    }

    let marked: Vec<usize> = belongs_to
        .iter()
        .copied()
        .filter(|&i| fields[i].attrs.related)
        .collect();

    match marked.as_slice() {
        [one] => Ok(*one),

        [] => Err(syn::Error::new(
            entity.span(),
            format!(
                "entity '{}' has {} belongs_to fields referencing '{}' ({}); mark exactly one with #[related] to choose which owns `Related`",
                entity,
                belongs_to.len(),
                target,
                field_list(belongs_to, fields),
            ),
        )),

        [_, second, ..] => Err(syn::Error::new(
            fields[*second].name.span(),
            format!(
                "entity '{}' marks {} belongs_to fields referencing '{}' with #[related] ({}); only one may be marked",
                entity,
                marked.len(),
                target,
                field_list(&marked, fields),
            ),
        )),
    }
}

fn field_list(indices: &[usize], fields: &[AnalyzedField]) -> String {
    indices
        .iter()
        .map(|&i| format!("'{}'", fields[i].name))
        .collect::<Vec<_>>()
        .join(", ")
}

// Validate that #[related] is only used on belongs_to fields, not has_many or scalar fields.
fn validate_related_attr_placement(fields: &[AnalyzedField]) -> Result<()> {
    for field in fields.iter() {
        if !field.attrs.related {
            continue;
        }

        match &field.ty {
            FieldType::BelongsTo { .. } => {}

            // For now , we dont support #[related] on has_many fields. TODO for later
            FieldType::HasMany { .. } => {
                return Err(syn::Error::new(
                    field.name.span(),
                    format!(
                        "#[related] cannot be used on the has_many field '{}'; mark the belongs_to field that owns the foreign key instead",
                        field.name
                    ),
                ));
            }

            FieldType::Scalar { .. } => {
                return Err(syn::Error::new(
                    field.name.span(),
                    format!(
                        "#[related] can only be used on a foreign key field (belongs_to), but was used on the scalar field '{}'",
                        field.name
                    ),
                ));
            }
        }
    }

    Ok(())
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
    fn test_created_at_allowed_with_timestamps_none() {
        let input = quote! {
            #[timestamps(none)]
            User {
                email: String,
                created_at: NaiveDateTime,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);
        assert!(result.is_ok());
    }

    #[test]
    fn test_created_at_rejected_with_default_timestamps() {
        let input = quote! {
            User {
                email: String,
                created_at: NaiveDateTime,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("auto-generated"));
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

    #[test]
    fn test_analyze_composite_primary_key() {
        let input = quote! {
            #[primary_key(user_id, role_id)]
            #[timestamps(none)]
            UsersRole {
                user_id: i32,
                role_id: i32,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let entity = &analyzed.entities[0];
        assert_eq!(
            entity.attrs.primary_key,
            Some(vec!["user_id".to_string(), "role_id".to_string()])
        );
    }

    #[test]
    fn test_analyze_primary_key_column_not_found() {
        let input = quote! {
            #[primary_key(user_id, nonexistent)]
            #[timestamps(none)]
            UsersRole {
                user_id: i32,
                role_id: i32,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_analyze_primary_key_allows_belongs_to_fields() {
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

        let tx_label = &analyzed.entities[2];
        assert_eq!(
            tx_label.attrs.primary_key,
            Some(vec!["tx_id".to_string(), "label_id".to_string()])
        );
        assert!(matches!(tx_label.fields[0].ty, FieldType::BelongsTo { .. }));
        assert!(matches!(tx_label.fields[1].ty, FieldType::BelongsTo { .. }));
    }

    #[test]
    fn test_analyze_primary_key_rejects_has_many_fields() {
        let input = quote! {
            User {
                posts: Vec<Post>,
            }

            Post {
                title: String,
            }

            #[primary_key(posts)]
            #[timestamps(none)]
            UserPosts {
                posts: Vec<Post>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("has_many"));
    }

    #[test]
    fn test_analyze_primary_key_rejects_optional_belongs_to() {
        let input = quote! {
            User {
                email: String,
            }

            #[primary_key(user_id)]
            #[timestamps(none)]
            Membership {
                user_id: Option<User>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let result = analyze_schema(parsed);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("optional"));
    }

    #[test]
    fn test_analyze_primary_key_rejects_self_referential_cycle_at_field() {
        let input = r#"
            #[timestamps(none)]
            #[primary_key(parent)]
            Category {
                parent: Category,
            }
        "#
        .parse()
        .unwrap();

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err();

        assert!(error.to_string().contains("Category.parent"));
        assert!(error.to_string().contains("primary key cycle"));
        assert_eq!(error.span().source_text().as_deref(), Some("parent"));
    }

    #[test]
    fn test_analyze_composite_pk_target_error_points_at_field() {
        let input = r#"
            #[primary_key(left_id, right_id)]
            #[timestamps(none)]
            Pair {
                left_id: i32,
                right_id: i32,
            }

            PairRef {
                pair: Pair,
            }
        "#
        .parse()
        .unwrap();

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err();

        assert!(error.to_string().contains("PairRef.pair"));
        assert!(error.to_string().contains("left_id"));
        assert!(error.to_string().contains("right_id"));
        assert_eq!(error.span().source_text().as_deref(), Some("pair"));
    }

    #[test]
    fn test_analyze_primary_key_with_extra_fields() {
        let input = quote! {
            #[primary_key(user_id, role_id)]
            #[timestamps(none)]
            UsersRole {
                user_id: i32,
                role_id: i32,
                assigned_at: NaiveDateTime,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        let entity = &analyzed.entities[0];
        assert_eq!(entity.fields.len(), 3);
        assert_eq!(
            entity.attrs.primary_key,
            Some(vec!["user_id".to_string(), "role_id".to_string()])
        );
    }

    /// Every message in a `syn::Error`. `to_string()` renders only the first,
    /// so combined errors have to be walked to be seen at all.
    fn error_messages(error: syn::Error) -> Vec<String> {
        error.into_iter().map(|e| e.to_string()).collect()
    }

    /// Which fields of `entity` were nominated to own a `Related` impl.
    fn nominated(entity: &AnalyzedEntity) -> Vec<String> {
        entity
            .fields
            .iter()
            .filter(|f| f.implement_related)
            .map(|f| f.name.to_string())
            .collect()
    }

    #[test]
    fn test_related_single_belongs_to_is_nominated() {
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

        // One field to a target needs no #[related] to disambiguate.
        assert_eq!(nominated(&analyzed.entities[1]), vec!["author"]);
    }

    #[test]
    fn test_related_single_has_many_is_nominated() {
        let input = quote! {
            User {
                posts: Vec<Post>,
            }

            Post {
                title: String,
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        assert_eq!(nominated(&analyzed.entities[0]), vec!["posts"]);
    }

    #[test]
    fn test_related_belongs_to_beats_has_many_to_same_target() {
        let input = quote! {
            Category {
                name: String,
                parent: Option<Category>,
                children: Vec<Category>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        // The has_many cannot own `Related`: SeaORM derives it as the reverse
        // of the target's own impl, so `children` resolves through `parent`.
        assert_eq!(nominated(&analyzed.entities[0]), vec!["parent"]);
    }

    #[test]
    fn test_related_attr_picks_the_marked_belongs_to() {
        let input = quote! {
            Account {
                name: String,
            }

            Tx {
                from: Option<Account>,
                #[related]
                to: Option<Account>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        // Marked on the second field, so a "first field wins" regression fails.
        assert_eq!(nominated(&analyzed.entities[1]), vec!["to"]);
    }

    #[test]
    fn test_related_attr_leaves_other_targets_alone() {
        let input = quote! {
            Warehouse {
                name: String,
            }

            Currency {
                code: String,
            }

            Shipment {
                origin: Warehouse,
                destination: Warehouse,
                #[related]
                backup: Option<Warehouse>,
                currency: Currency,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let analyzed = analyze_schema(parsed).unwrap();

        // `currency` is the only field for its target, so it is nominated
        // independently of the contested Warehouse group.
        assert_eq!(nominated(&analyzed.entities[2]), vec!["backup", "currency"]);
    }

    #[test]
    fn test_related_rejects_two_belongs_to_with_none_marked() {
        let input = quote! {
            Account {
                name: String,
            }

            Tx {
                from: Option<Account>,
                to: Option<Account>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        assert!(error.contains("mark exactly one with #[related]"));
        assert!(error.contains("'from', 'to'"));
    }

    #[test]
    fn test_related_rejects_two_belongs_to_both_marked() {
        let input = quote! {
            Account {
                name: String,
            }

            Tx {
                #[related]
                from: Option<Account>,
                #[related]
                to: Option<Account>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        assert!(error.contains("only one may be marked"));
        assert!(error.contains("'from', 'to'"));
    }

    #[test]
    fn test_related_rejects_two_has_many_to_same_target() {
        let input = quote! {
            User {
                posts: Vec<Post>,
                drafts: Vec<Post>,
            }

            Post {
                title: String,
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        // Both mirror Post's single `Related<User>`, so they would produce
        // identical joins and `drafts` would silently mean every post.
        assert!(error.contains("has_many fields referencing"));
        assert!(error.contains("'posts', 'drafts'"));
    }

    #[test]
    fn test_related_rejects_two_has_many_even_when_a_belongs_to_could_win() {
        let input = quote! {
            User {
                posts: Vec<Post>,
                drafts: Vec<Post>,
                favourite: Option<Post>,
            }

            Post {
                title: String,
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        // `favourite` is eligible and would work, but the has_many pair is
        // broken with or without it: both mirror Post's back-edge, so they
        // resolve identically no matter who owns `Related`.
        assert!(error.contains("has_many fields referencing"));
    }

    #[test]
    fn test_related_attr_rejected_on_has_many_field() {
        let input = quote! {
            User {
                #[related]
                posts: Vec<Post>,
            }

            Post {
                title: String,
                author: User,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        assert!(error.contains("cannot be used on the has_many field 'posts'"));
        assert!(error.contains("mark the belongs_to field that owns the foreign key instead"));
    }

    #[test]
    fn test_related_attr_rejected_on_scalar_field() {
        let input = quote! {
            User {
                #[related]
                email: String,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let error = analyze_schema(parsed).unwrap_err().to_string();

        assert!(error.contains("foreign key field (belongs_to)"));
        assert!(error.contains("scalar field 'email'"));
    }

    #[test]
    fn test_related_reports_every_contested_group() {
        let input = quote! {
            Account {
                name: String,
            }

            Warehouse {
                name: String,
            }

            Tx {
                from: Option<Account>,
                to: Option<Account>,
                origin: Option<Warehouse>,
                destination: Option<Warehouse>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let messages = error_messages(analyze_schema(parsed).unwrap_err());

        // Groups are walked in target-name order, so both groups are reported
        // and always in the same order.
        assert_eq!(messages.len(), 2);
        assert!(messages[0].contains("'Account'"));
        assert!(messages[1].contains("'Warehouse'"));
    }

    #[test]
    fn test_related_misplaced_attr_reported_before_group_rules() {
        let input = quote! {
            Account {
                name: String,
            }

            Tx {
                #[related]
                amount: i64,
                from: Option<Account>,
                to: Option<Account>,
            }
        };

        let parsed = parse_schema(input).unwrap();
        let messages = error_messages(analyze_schema(parsed).unwrap_err());

        // A misplaced #[related] makes the group rules meaningless, so it is
        // reported alone rather than alongside the unmarked Account group.
        assert_eq!(messages.len(), 1);
        assert!(messages[0].contains("scalar field 'amount'"));
    }
}
