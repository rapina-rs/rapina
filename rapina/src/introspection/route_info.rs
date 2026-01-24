//! Route metadata for introspection.

use serde::Serialize;

/// Metadata about a registered route.
///
/// Contains information about a route's HTTP method, path pattern,
/// and handler name for introspection and documentation generation.
///
/// # Examples
///
/// ```
/// use rapina::introspection::RouteInfo;
///
/// let info = RouteInfo::new("GET", "/users/:id", "get_user", None);
/// assert_eq!(info.method, "GET");
/// assert_eq!(info.path, "/users/:id");
/// ```
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteInfo {
    /// The HTTP method (GET, POST, PUT, DELETE, etc.).
    pub method: String,
    /// The path pattern with parameters (e.g., "/users/:id").
    pub path: String,
    /// The name of the handler function.
    pub handler_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_schema: Option<serde_json::Value>,
}

impl RouteInfo {
    /// Creates a new RouteInfo with the given metadata.
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        handler_name: impl Into<String>,
        response_schema: Option<serde_json::Value>,
    ) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            handler_name: handler_name.into(),
            response_schema,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_info_new() {
        let info = RouteInfo::new("GET", "/users", "list_users", None);
        assert_eq!(info.method, "GET");
        assert_eq!(info.path, "/users");
        assert_eq!(info.handler_name, "list_users");
    }

    #[test]
    fn test_route_info_with_params() {
        let info = RouteInfo::new("GET", "/users/:id", "get_user", None);
        assert_eq!(info.path, "/users/:id");
    }

    #[test]
    fn test_route_info_clone() {
        let info = RouteInfo::new("POST", "/users", "create_user", None);
        let cloned = info.clone();
        assert_eq!(info, cloned);
    }

    #[test]
    fn test_route_info_serialize() {
        let info = RouteInfo::new("GET", "/health", "health_check", None);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"method\":\"GET\""));
        assert!(json.contains("\"path\":\"/health\""));
        assert!(json.contains("\"handler_name\":\"health_check\""));
    }

    #[test]
    fn test_route_info_debug() {
        let info = RouteInfo::new("DELETE", "/users/:id", "delete_user", None);
        let debug = format!("{:?}", info);
        assert!(debug.contains("DELETE"));
        assert!(debug.contains("/users/:id"));
    }
}
