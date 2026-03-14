use rapina::database::DbError;
use rapina::prelude::*;

pub enum UrlsError {
    DbError(DbError),
}

impl IntoApiError for UrlsError {
    fn into_api_error(self) -> Error {
        match self {
            UrlsError::DbError(e) => e.into_api_error(),
        }
    }
}

impl DocumentedError for UrlsError {
    fn error_variants() -> Vec<ErrorVariant> {
        vec![
            ErrorVariant {
                status: 404,
                code: "NOT_FOUND",
                description: "Urls not found",
            },
            ErrorVariant {
                status: 500,
                code: "DATABASE_ERROR",
                description: "Database operation failed",
            },
        ]
    }
}

impl From<DbError> for UrlsError {
    fn from(e: DbError) -> Self {
        UrlsError::DbError(e)
    }
}
