use rapina::prelude::*;

schema! {
    #[timestamps(none)]
    Todo {
        title: String,
        done: bool,
    }
}
