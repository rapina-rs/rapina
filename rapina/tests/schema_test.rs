//! Integration tests for the schema! macro.
//!
//! These tests verify that the generated code compiles and matches SeaORM patterns.

#![cfg(feature = "database")]

use rapina::prelude::*;
use rapina::sea_orm::entity::prelude::*;

// Define a test schema with various relationship types
schema! {
    TestUser {
        email: String,
        name: String,
        bio: Option<Text>,
        posts: Vec<TestPost>,
        comments: Vec<TestComment>,
    }

    TestPost {
        title: String,
        content: Text,
        published: bool,
        author: TestUser,
        comments: Vec<TestComment>,
    }

    TestComment {
        content: Text,
        post: TestPost,
        author: Option<TestUser>,
    }
}

#[test]
fn test_user_model_compiles() {
    use test_user::Model;

    // Verify the Model struct has the expected fields
    let user = Model {
        id: 1,
        email: "test@example.com".to_string(),
        name: "Test User".to_string(),
        bio: Some("A test user".to_string()),
        created_at: DateTimeUtc::default(),
        updated_at: DateTimeUtc::default(),
    };

    assert_eq!(user.id, 1);
    assert_eq!(user.email, "test@example.com");
}

#[test]
fn test_post_model_has_foreign_key() {
    use test_post::Model;

    // Verify the belongs_to relationship generates author_id
    let post = Model {
        id: 1,
        title: "Test Post".to_string(),
        content: "Test content".to_string(),
        published: true,
        author_id: 1, // Foreign key from belongs_to
        created_at: DateTimeUtc::default(),
        updated_at: DateTimeUtc::default(),
    };

    assert_eq!(post.author_id, 1);
}

#[test]
fn test_comment_model_has_optional_foreign_key() {
    use test_comment::Model;

    // Verify optional belongs_to generates Option<i32> FK
    let comment_with_author = Model {
        id: 1,
        content: "Great post!".to_string(),
        post_id: 1,
        author_id: Some(1), // Optional FK
        created_at: DateTimeUtc::default(),
        updated_at: DateTimeUtc::default(),
    };

    let comment_without_author = Model {
        id: 2,
        content: "Anonymous comment".to_string(),
        post_id: 1,
        author_id: None,
        created_at: DateTimeUtc::default(),
        updated_at: DateTimeUtc::default(),
    };

    assert_eq!(comment_with_author.author_id, Some(1));
    assert_eq!(comment_without_author.author_id, None);
}

#[test]
fn test_relation_enum_exists() {
    // Verify Relation enums are generated with expected variants
    use test_comment::Relation as CommentRelation;
    use test_post::Relation as PostRelation;
    use test_user::Relation as UserRelation;

    // User has Posts and Comments (has_many)
    let _ = UserRelation::Posts;
    let _ = UserRelation::Comments;

    // Post has Author (belongs_to) and Comments (has_many)
    let _ = PostRelation::Author;
    let _ = PostRelation::Comments;

    // Comment has Post (belongs_to) and Author (optional belongs_to)
    let _ = CommentRelation::Post;
    let _ = CommentRelation::Author;
}

#[test]
fn test_entity_traits_implemented() {
    // Verify Entity trait is implemented via EntityName
    let _ = test_user::Entity::table_name(&test_user::Entity);
    let _ = test_post::Entity::table_name(&test_post::Entity);
    let _ = test_comment::Entity::table_name(&test_comment::Entity);
}

// Regression test for issue #678: two `belongs_to` fields to the same
// target used to produce two conflicting `impl Related<account::Entity> for
// tx::Entity` blocks (E0119) — this whole file would fail to compile without
// the fix. `from` is canonical and keeps `Related`; `to` is reachable
// through the generated `Linked` (`regression_tx::ToLink`).
schema! {
    Account {
        name: String,
    }

    RegressionTx {
        from: Option<Account>,
        to: Option<Account>,
        amount: i64,
    }
}

#[test]
fn test_issue_678_related_resolves_to_canonical_from_column() {
    use rapina::sea_orm::{DbBackend, QueryTrait};

    // find_also_related only compiles at all because Related<Account> exists
    // for Tx — and it must resolve to `from`, the canonical field.
    let stmt = regression_tx::Entity::find()
        .find_also_related(regression_account::Entity)
        .build(DbBackend::Sqlite)
        .to_string();

    assert!(stmt.to_lowercase().contains("from_id"));
}

#[test]
fn test_issue_678_linked_resolves_to_losing_to_column() {
    use rapina::sea_orm::{DbBackend, ModelTrait, QueryTrait};

    let tx = regression_tx::Model {
        id: 1,
        from_id: Some(1),
        to_id: Some(2),
        amount: 500,
        created_at: DateTimeUtc::default(),
        updated_at: DateTimeUtc::default(),
    };

    // No Related<Account> exists for the `to` direction — find_linked is the
    // only route, and it must resolve to `to_id`, not `from_id`.
    let stmt = tx
        .find_linked(regression_tx::ToLink)
        .build(DbBackend::Sqlite)
        .to_string();

    assert!(stmt.to_lowercase().contains("to_id"));
}
