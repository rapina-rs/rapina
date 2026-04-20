use rapina::schemars::{self, JsonSchema};
use serde::Deserialize;
use validator::Validate;

#[derive(Deserialize, Validate, JsonSchema)]
pub struct CreateTodo {
    #[validate(length(min = 1, max = 100))]
    pub title: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct UpdateTodo {
    pub title: Option<String>,
    pub done: Option<bool>,
}
