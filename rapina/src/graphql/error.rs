//! Bridges `rapina::Error` into async-graphql errors.
//! `ErrorExtensions` populates the `code` extension; `graphql_error` builds
//! errors explicitly with `code` + `trace_id`.

use crate::error::Error as RapinaError;
use async_graphql::{Error as GraphQLError, ErrorExtensions};

impl ErrorExtensions for RapinaError {
    fn extend(&self) -> GraphQLError {
        GraphQLError::new(self.message()).extend_with(|_, e| {
            e.set("code", self.code());
            if let Some(tid) = self.trace_id() {
                e.set("trace_id", tid);
            }
            if let Some(d) = self.details() {
                if let Ok(json) = async_graphql::Value::from_json(d.clone()) {
                    e.set("details", json);
                }
            }
        })
    }
}
/// Construct an async-graphql error with Rapina's error code vocabulary
/// and the current trace_id populated in extensions.
pub fn graphql_error(code: &str, message: impl Into<String>, trace_id: &str) -> GraphQLError {
    GraphQLError::new(message).extend_with(|_, e| {
        e.set("code", code);
        e.set("trace_id", trace_id);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error as RapinaError;
    use async_graphql::Value;

    #[test]
    fn extend_populates_code_and_trace_id() {
        let err = RapinaError::bad_request("error").with_trace_id("trace-123");
        let gql = err.extend();

        assert_eq!(gql.message, "error");
        let ext = gql.extensions.expect("Should have extensions");
        assert_eq!(ext.get("code"), Some(&Value::from("BAD_REQUEST")));
        assert_eq!(ext.get("trace_id"), Some(&Value::from("trace-123")));
    }

    #[test]
    fn extend_omits_trace_id_when_absent() {
        let err = RapinaError::bad_request("error");
        let gql = err.extend();

        let ext = gql.extensions.expect("should have extensions");
        assert!(ext.get("trace_id").is_none());
    }

    #[test]
    fn extend_populates_code_and_details() {
        let err =
            RapinaError::bad_request("error").with_details(serde_json::json!({"field": "email"}));
        let gql = err.extend();

        assert_eq!(gql.message, "error");
        let ext = gql.extensions.expect("Should have extensions");
        assert_eq!(ext.get("code"), Some(&Value::from("BAD_REQUEST")));
        assert_eq!(
            ext.get("details"),
            Some(&Value::from_json(serde_json::json!({"field": "email"})).unwrap())
        )
    }

    #[test]
    fn extend_omits_details_when_absent() {
        let err = RapinaError::bad_request("error");
        let gql = err.extend();

        let ext = gql.extensions.expect("Should have extensions");
        assert!(ext.get("details").is_none());
    }

    #[test]
    fn extend_populates_code_with_trace_id_and_details() {
        let err = RapinaError::bad_request("error")
            .with_trace_id("trace-123")
            .with_details(serde_json::json!({"field": "email"}));
        let gql = err.extend();

        assert_eq!(gql.message, "error");
        let ext = gql.extensions.expect("Should have extentions");
        assert_eq!(ext.get("code"), Some(&Value::from("BAD_REQUEST")));
        assert_eq!(ext.get("trace_id"), Some(&Value::from("trace-123")));
        assert_eq!(
            ext.get("details"),
            Some(&Value::from_json(serde_json::json!({"field": "email"})).unwrap())
        )
    }

    #[test]
    fn graphql_error_builds_code_and_trace_id() {
        let gql = graphql_error("INVALID_INPUT", "qty too large", "trace-xyz");

        assert_eq!(gql.message, "qty too large");
        let ext = gql.extensions.expect("Should have extentions");
        assert_eq!(ext.get("code"), Some(&Value::from("INVALID_INPUT")));
        assert_eq!(ext.get("trace_id"), Some(&Value::from("trace-xyz")));
    }
}
