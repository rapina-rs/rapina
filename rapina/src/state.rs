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
/// #[derive(Debug)]
/// struct DatabaseConfig {
///     url: String,
/// }
///
/// #[derive(Debug)]
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

    /// Registers a pre-existing `Arc<T>` as shared state.
    ///
    /// Use this when `T` is a trait object (e.g. `Arc<dyn MyTrait>`) and you
    /// want to access it via [`State<Arc<dyn MyTrait>>`](crate::extract::State)
    /// in handlers without needing a newtype wrapper.
    ///
    /// Internally the value is stored under `TypeId::of::<Arc<T>>()` wrapped in
    /// one additional `Arc` (as required by the state map). Handlers receive
    /// `State<Arc<dyn MyTrait>>` and can call methods directly via auto-deref,
    /// or clone the inner arc with `(*state).clone()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use rapina::state::AppState;
    /// use std::sync::Arc;
    ///
    /// trait Greeter: Send + Sync {
    ///     fn greet(&self) -> String;
    /// }
    ///
    /// struct HelloGreeter;
    /// impl Greeter for HelloGreeter {
    ///     fn greet(&self) -> String { "hello".to_string() }
    /// }
    ///
    /// let greeter: Arc<dyn Greeter> = Arc::new(HelloGreeter);
    /// let state = AppState::new().with_arc(greeter);
    ///
    /// // Access via State<Arc<dyn Greeter>> in handlers; deref gives Arc<dyn Greeter>
    /// ```
    pub fn with_arc<T: ?Sized + Send + Sync + 'static>(mut self, value: Arc<T>) -> Self {
        self.inner.insert(TypeId::of::<Arc<T>>(), Arc::new(value));
        self
    }

    /// Retrieves a shared `Arc<T>` for a value of type `T`, if registered.
    ///
    /// This is useful when you want to share state without cloning the
    /// inner value. The [`State`](crate::extract::State) extractor uses
    /// this internally so that extraction is an atomic reference-count
    /// bump rather than a deep clone.
    ///
    /// Returns `None` if no value of type `T` has been added.
    pub fn get_arc<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.inner
            .get(&TypeId::of::<T>())
            .and_then(|arc| Arc::clone(arc).downcast::<T>().ok())
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
    fn test_app_state_get_arc() {
        #[derive(Debug, PartialEq)]
        struct Config {
            name: String,
        }

        let state = AppState::new().with(Config {
            name: "test".to_string(),
        });

        let arc = state.get_arc::<Config>().unwrap();
        assert_eq!(arc.name, "test");

        // A second call returns an independent Arc pointing to the same data
        let arc2 = state.get_arc::<Config>().unwrap();
        assert!(Arc::ptr_eq(&arc, &arc2));
    }

    #[test]
    fn test_app_state_get_arc_missing() {
        struct Missing;

        let state = AppState::new();
        assert!(state.get_arc::<Missing>().is_none());
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

    #[test]
    fn test_with_arc_concrete_type() {
        #[derive(Debug, PartialEq)]
        struct Repo {
            name: &'static str,
        }

        let arc = Arc::new(Repo { name: "pg" });
        let state = AppState::new().with_arc(Arc::clone(&arc));

        // Extracted via get_arc::<Arc<Repo>>()
        let extracted = state.get_arc::<Arc<Repo>>().unwrap();
        assert_eq!(extracted.name, "pg");
    }

    #[test]
    fn test_with_arc_trait_object() {
        trait Greeter: Send + Sync {
            fn greet(&self) -> &'static str;
        }

        struct Hello;
        impl Greeter for Hello {
            fn greet(&self) -> &'static str {
                "hello"
            }
        }

        let greeter: Arc<dyn Greeter> = Arc::new(Hello);
        let state = AppState::new().with_arc(greeter);

        let extracted = state.get_arc::<Arc<dyn Greeter>>().unwrap();
        assert_eq!(extracted.greet(), "hello");
    }

    #[test]
    fn test_with_arc_does_not_conflict_with_with() {
        // with() and with_arc() on same logical type use different TypeIds
        // (TypeId::of::<T>() vs TypeId::of::<Arc<T>>()) so they coexist.
        #[derive(Debug, PartialEq)]
        struct Config {
            val: i32,
        }

        let concrete = Config { val: 1 };
        let arc = Arc::new(Config { val: 2 });

        let state = AppState::new().with(concrete).with_arc(Arc::clone(&arc));

        assert_eq!(state.get::<Config>().unwrap().val, 1);
        assert_eq!(state.get_arc::<Arc<Config>>().unwrap().val, 2);
    }

    #[test]
    fn test_with_arc_missing_returns_none() {
        trait Repo: Send + Sync {}

        let state = AppState::new();
        assert!(state.get_arc::<Arc<dyn Repo>>().is_none());
    }

    #[test]
    fn test_with_arc_overwrites_same_arc_type() {
        trait Counter: Send + Sync {
            fn count(&self) -> u32;
        }

        struct CounterImpl(u32);
        impl Counter for CounterImpl {
            fn count(&self) -> u32 {
                self.0
            }
        }

        let first: Arc<dyn Counter> = Arc::new(CounterImpl(1));
        let second: Arc<dyn Counter> = Arc::new(CounterImpl(2));

        let state = AppState::new().with_arc(first).with_arc(second);

        let extracted = state.get_arc::<Arc<dyn Counter>>().unwrap();
        assert_eq!(extracted.count(), 2);
    }
}
