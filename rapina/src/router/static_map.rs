use std::collections::HashMap;

use http::Method;

/// O(1) lookup table for routes with no path parameters.
///
/// Built at `prepare()` time from the full route list. Routes whose
/// patterns contain `:param` segments are skipped — they go through
/// the linear scanner.
pub(super) struct StaticMap {
    map: HashMap<Method, HashMap<String, usize>>,
}

impl StaticMap {
    pub(super) fn build(routes: &[(Method, super::Route)]) -> Self {
        let mut map: HashMap<Method, HashMap<String, usize>> = HashMap::new();
        for (idx, (method, route)) in routes.iter().enumerate() {
            if !super::is_dynamic(&route.pattern) {
                map.entry(method.clone())
                    .or_default()
                    .insert(route.pattern.clone(), idx);
            }
        }
        Self { map }
    }

    pub(super) fn lookup(&self, method: &Method, path: &str) -> Option<usize> {
        self.map.get(method)?.get(path).copied()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map.values().map(|m| m.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_partitions_static_and_dynamic() {
        let router = crate::router::Router::new()
            .route(Method::GET, "/health", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::GET, "/users", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::GET, "/users/:id", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::POST, "/users", |_, _, _| async {
                http::StatusCode::CREATED
            });

        let static_map = StaticMap::build(&router.routes);

        // 3 static routes: GET /health, GET /users, POST /users
        assert_eq!(static_map.len(), 3);
        assert!(static_map.lookup(&Method::GET, "/health").is_some());
        assert!(static_map.lookup(&Method::GET, "/users").is_some());
        assert!(static_map.lookup(&Method::POST, "/users").is_some());
        // dynamic route not in static map
        assert!(static_map.lookup(&Method::GET, "/users/:id").is_none());
    }

    #[test]
    fn test_lookup_miss_wrong_method() {
        let router = crate::router::Router::new().route(Method::GET, "/health", |_, _, _| async {
            http::StatusCode::OK
        });

        let static_map = StaticMap::build(&router.routes);

        assert!(static_map.lookup(&Method::GET, "/health").is_some());
        assert!(static_map.lookup(&Method::POST, "/health").is_none());
    }

    #[test]
    fn test_lookup_miss_wrong_path() {
        let router = crate::router::Router::new().route(Method::GET, "/health", |_, _, _| async {
            http::StatusCode::OK
        });

        let static_map = StaticMap::build(&router.routes);

        assert!(static_map.lookup(&Method::GET, "/nonexistent").is_none());
    }

    #[test]
    fn test_empty_routes() {
        let router = crate::router::Router::new();
        let static_map = StaticMap::build(&router.routes);
        assert_eq!(static_map.len(), 0);
    }

    #[test]
    fn test_all_dynamic_routes() {
        let router = crate::router::Router::new()
            .route(Method::GET, "/users/:id", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::GET, "/posts/:id/comments/:cid", |_, _, _| async {
                http::StatusCode::OK
            });

        let static_map = StaticMap::build(&router.routes);
        assert_eq!(static_map.len(), 0);
    }

    #[test]
    fn test_returns_correct_index() {
        let router = crate::router::Router::new()
            .route(Method::GET, "/first", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::GET, "/second", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::GET, "/third", |_, _, _| async {
                http::StatusCode::OK
            });

        let static_map = StaticMap::build(&router.routes);

        assert_eq!(static_map.lookup(&Method::GET, "/first"), Some(0));
        assert_eq!(static_map.lookup(&Method::GET, "/second"), Some(1));
        assert_eq!(static_map.lookup(&Method::GET, "/third"), Some(2));
    }
}
