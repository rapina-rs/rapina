//! `State<T>`, the shared application state extractor.

use std::ops::Deref;
use std::sync::Arc;

use crate::error::Error;
use crate::extract::{FromRequestParts, PathParams};
use crate::state::AppState;

/// Extracts application state.
///
/// Provides access to shared application state that was registered
/// with [`Rapina::state`](crate::app::Rapina::state).
///
/// The inner value is wrapped in an `Arc<T>`, so extraction is always
/// a cheap atomic reference-count bump rather than a deep clone.
/// This also removes the `Clone` requirement on `T`.
///
/// # Examples
///
/// ```ignore
/// use rapina::prelude::*;
///
/// struct AppConfig {
///     db_url: String,
/// }
///
/// #[get("/config")]
/// async fn get_config(state: State<AppConfig>) -> String {
///     state.db_url.clone()
/// }
/// ```
#[derive(Debug, Clone)]
pub struct State<T>(pub Arc<T>);

impl<T> State<T> {
    /// Consumes the extractor and returns the inner `Arc<T>`.
    pub fn into_inner(self) -> Arc<T> {
        self.0
    }
}

impl<T> Deref for State<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Send + Sync + 'static> FromRequestParts for State<T> {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        _params: &PathParams,
        state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        let arc = state.get_arc::<T>().ok_or_else(|| {
            Error::internal(format!(
                "State not registered for type '{}'. Did you forget to call .state() or .state_arc()?",
                std::any::type_name::<T>()
            ))
        })?;
        Ok(State(arc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state};

    #[derive(Debug, Clone, PartialEq)]
    struct Data {
        name: String,
    }

    // State extractor tests
    #[tokio::test]
    async fn test_state_extractor_success() {
        struct AppConfig {
            name: String,
        }

        let state = crate::test::state_with(AppConfig {
            name: "test-app".to_string(),
        });
        let (parts, _) = TestRequest::get("/").into_parts();

        let result = State::<AppConfig>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "test-app");
    }

    #[tokio::test]
    async fn test_state_extractor_not_found() {
        #[derive(Debug)]
        struct MissingState;

        let state = empty_state();
        let (parts, _) = TestRequest::get("/").into_parts();

        let result =
            State::<MissingState>::from_request_parts(&parts, &empty_params(), &state).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 500);
    }

    #[tokio::test]
    async fn test_state_extractor_arc_trait_object() {
        trait Greeter: Send + Sync {
            fn greet(&self) -> &'static str;
        }

        struct Hello;
        impl Greeter for Hello {
            fn greet(&self) -> &'static str {
                "hello"
            }
        }

        let greeter: std::sync::Arc<dyn Greeter> = std::sync::Arc::new(Hello);
        let state = std::sync::Arc::new(crate::state::AppState::new().with_arc(greeter));
        let (parts, _) = TestRequest::get("/").into_parts();

        let result = State::<std::sync::Arc<dyn Greeter>>::from_request_parts(
            &parts,
            &empty_params(),
            &state,
        )
        .await;

        assert!(result.is_ok());
        // Deref chain: State<Arc<dyn Greeter>> -> Arc<dyn Greeter> -> dyn Greeter
        assert_eq!(result.unwrap().greet(), "hello");
    }

    #[test]
    fn test_state_into_inner() {
        let state = State(Arc::new("value".to_string()));
        let arc = state.into_inner();
        assert_eq!(*arc, "value");
    }

    #[test]
    fn test_state_deref() {
        let state = State(Arc::new("value".to_string()));
        assert_eq!(*state, "value");
    }

    #[test]
    fn test_state_autoderef() {
        let data = Data {
            name: "state test".to_string(),
        };

        let state = State(Arc::new(data.clone()));
        assert_eq!(state.name, data.name);
    }
}
