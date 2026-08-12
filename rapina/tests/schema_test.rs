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

    #[table_name = "test_tx_labels"]
    #[timestamps(none)]
    #[primary_key(tx_id, label_id)]
    TestTxLabel {
        #[column = "transaction_id"]
        tx_id: i32,
        label_id: i32,
    }

    #[table_name = "test_regions"]
    #[timestamps(none)]
    #[primary_key(code)]
    TestRegion {
        #[column = "region_code"]
        code: Text,
    }

}

// Join table whose composite primary key is made of entity-typed fields (#619).
// Each PK column is a belongs_to relationship; the generated FK columns must
// keep their verbatim names (tx_id / label_id, no extra _id suffix) and adopt
// the target entities' primary-key scalar types.
schema! {
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

#[test]
fn test_composite_pk_respects_column_rename() {
    assert_eq!(test_tx_label::Column::TxId.as_str(), "transaction_id");
    assert_eq!(test_tx_label::Column::LabelId.as_str(), "label_id");
    assert_eq!(test_region::Column::Code.as_str(), "region_code");
}

#[test]
fn test_join_table_entity_typed_primary_key_compiles() {
    use tx_label::Model;

    // The composite PK columns are the verbatim field names (no _id suffix)
    // and resolve to the target entities' i32 primary keys. A join table has
    // no auto-generated `id` column.
    let link = Model {
        tx_id: 1,
        label_id: 2,
    };

    assert_eq!(link.tx_id, 1);
    assert_eq!(link.label_id, 2);

    // Relations to both parent entities are generated (named after the fields).
    let _ = tx_label::Relation::TxId;
    let _ = tx_label::Relation::LabelId;

    let _ = tx_label::Entity::table_name(&tx_label::Entity);
}
