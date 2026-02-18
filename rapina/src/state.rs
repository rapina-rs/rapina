//! Application state for dependency injection into handlers.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

type StateMap = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

/// A type-safe container for sharing state across request handlers.
///
/// `AppState` stores values indexed by their [`TypeId`], allowing
/// handlers to extract dependencies via [`State<T>`](crate::extract::State).
///
/// # Examples
///
/// ```
/// use rapina::state::AppState;
///
/// #[derive(Debug, Clone)]
/// struct DatabaseConfig {
///     url: String,
/// }
///
/// #[derive(Debug, Clone)]
/// struct CacheConfig {
///     ttl_secs: u64,
/// }
///
/// let state = AppState::new()
///     .with(DatabaseConfig {
///         url: "postgres://localhost/mydb".to_string(),
///     })
///     .with(CacheConfig { ttl_secs: 300 });
///
/// let db = state.get::<DatabaseConfig>().unwrap();
/// assert_eq!(db.url, "postgres://localhost/mydb");
///
/// let cache = state.get::<CacheConfig>().unwrap();
/// assert_eq!(cache.ttl_secs, 300);
///
/// // Returns None if the type was not registered
/// assert!(state.get::<String>().is_none());
/// ```
#[derive(Default, Clone)]
pub struct AppState {
    inner: StateMap,
}

impl AppState {
    /// Creates a new empty `AppState`.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Registers a value of type `T` in the state.
    ///
    /// If a value of the same type already exists, it will be overwritten.
    pub fn with<T: Send + Sync + 'static>(mut self, value: T) -> Self {
        self.inner.insert(TypeId::of::<T>(), Arc::new(value));
        self
    }

    /// Retrieves a reference to a value of type `T`, if registered.
    ///
    /// Returns `None` if no value of type `T` has been added.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|arc| arc.downcast_ref::<T>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_state_new() {
        let state = AppState::new();
        assert!(state.inner.is_empty());
    }

    #[test]
    fn test_app_state_default() {
        let state = AppState::default();
        assert!(state.inner.is_empty());
    }

    #[test]
    fn test_app_state_with_value() {
        #[derive(Debug, PartialEq)]
        struct Config {
            name: String,
        }

        let state = AppState::new().with(Config {
            name: "test".to_string(),
        });

        let config = state.get::<Config>().unwrap();
        assert_eq!(config.name, "test");
    }

    #[test]
    fn test_app_state_get_missing() {
        struct Missing;

        let state = AppState::new();
        assert!(state.get::<Missing>().is_none());
    }

    #[test]
    fn test_app_state_multiple_types() {
        #[derive(Debug)]
        struct Config {
            name: String,
        }

        #[derive(Debug)]
        struct Database {
            url: String,
        }

        let state = AppState::new()
            .with(Config {
                name: "app".to_string(),
            })
            .with(Database {
                url: "postgres://localhost".to_string(),
            });

        let config = state.get::<Config>().unwrap();
        let db = state.get::<Database>().unwrap();

        assert_eq!(config.name, "app");
        assert_eq!(db.url, "postgres://localhost");
    }

    #[test]
    fn test_app_state_overwrites_same_type() {
        let state = AppState::new()
            .with("first".to_string())
            .with("second".to_string());

        let value = state.get::<String>().unwrap();
        assert_eq!(value, "second");
    }

    #[test]
    fn test_app_state_clone() {
        let state = AppState::new().with(42i32);
        let cloned = state.clone();

        assert_eq!(state.get::<i32>(), Some(&42));
        assert_eq!(cloned.get::<i32>(), Some(&42));
    }

    #[test]
    fn test_app_state_with_chaining() {
        let state = AppState::new()
            .with(1i32)
            .with(2i64)
            .with(3.0f64)
            .with("test".to_string());

        assert_eq!(state.get::<i32>(), Some(&1));
        assert_eq!(state.get::<i64>(), Some(&2));
        assert_eq!(state.get::<f64>(), Some(&3.0));
        assert_eq!(state.get::<String>(), Some(&"test".to_string()));
    }
}
