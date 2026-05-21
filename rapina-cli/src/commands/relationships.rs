pub(crate) struct RelationshipSpec {
    pub field_name: String,
    pub related_pascal: String,
    pub kind: RelationshipKind,
}

// Variants are only constructed in import.rs, which is #[cfg(feature = "import")].
// The allow is scoped to non-import builds where the construction sites are compiled out.
#[cfg_attr(not(feature = "import"), allow(dead_code))]
#[derive(Clone, Copy, Debug)]
pub(crate) enum RelationshipKind {
    BelongsTo { nullable: bool },
    HasMany,
}
