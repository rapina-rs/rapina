use rapina::schemars::{self, JsonSchema};
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct CreateTodo {
    pub title: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub done: Option<bool>,
}
