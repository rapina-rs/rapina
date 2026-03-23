use rapina::prelude::*;

schema! {
    #[timestamps(none)]
    Urls {
        short_code: String,
        long_url: Text,
        created_at: DateTime,
        expires_at: DateTime,
        click_count: i64,
    }
}
