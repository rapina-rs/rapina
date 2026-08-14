//! `Path<T>` and the path parameter machinery behind it.

use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use smallvec::SmallVec;
use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use crate::error::Error;
use crate::extract::{FromRequestParts, impl_deref};
use crate::state::AppState;

/// Extracts path parameters from the URL.
///
/// Supports three extraction modes:
///
/// **Single parameter** — parses one `:param` into any type that implements `FromStr`:
/// ```ignore
/// #[get("/users/:id")]
/// async fn get_user(id: Path<u64>) -> String {
///     format!("User ID: {}", *id)
/// }
/// ```
///
/// **Tuple** — extracts multiple parameters in declaration order (left to right in the pattern):
/// ```ignore
/// #[get("/orgs/:org_id/teams/:team_id")]
/// async fn get_team(Path((org_id, team_id)): Path<(u64, u64)>) -> String {
///     format!("org={} team={}", org_id, team_id)
/// }
/// ```
///
/// **Three or more** — same tuple syntax:
/// ```ignore
/// #[get("/orgs/:org_id/teams/:team_id/members/:member_id")]
/// async fn get_member(Path((org_id, team_id, member_id)): Path<(u64, u64, u64)>) -> String {
///     format!("org={} team={} member={}", org_id, team_id, member_id)
/// }
/// ```
///
/// Returns 400 Bad Request if a parameter is missing or cannot be parsed.
#[derive(Debug)]
pub struct Path<T>(pub T);

impl<T> Path<T> {
    /// Consumes the extractor and returns the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl_deref!(Path);

impl<T: DeserializeOwned + Send> FromRequestParts for Path<T> {
    async fn from_request_parts(
        _parts: &http::request::Parts,
        params: &PathParams,
        _state: &Arc<AppState>,
    ) -> Result<Self, Error> {
        T::deserialize(PathParamsDeserializer(params))
            .map(Path)
            .map_err(|e| Error::bad_request(e.to_string()))
    }
}

/// Path parameters extracted from the URL during route matching.
///
/// Stores up to 4 parameters on the stack without heap allocation.
/// Uses linear search which outperforms HashMap for the small N
/// typical of REST APIs (1-3 path parameters).
#[derive(Debug, Clone, Default)]
pub struct PathParams {
    entries: SmallVec<[(Cow<'static, str>, String); 4]>,
}

impl PathParams {
    pub fn new() -> Self {
        Self {
            entries: SmallVec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&String> {
        self.entries
            .iter()
            .find(|(k, _)| k.as_ref() == key)
            .map(|(_, v)| v)
    }

    pub fn insert(&mut self, key: String, value: String) -> Option<String> {
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k.as_ref() == key) {
            let old = std::mem::replace(&mut entry.1, value);
            Some(old)
        } else {
            self.entries.push((Cow::Owned(key), value));
            None
        }
    }

    /// Push a static key without checking for duplicates.
    ///
    /// Used by the trie during route matching where param names are
    /// leaked to `&'static str` at freeze time and the same key is never
    /// inserted twice in a single lookup. Zero allocation for the key,
    /// and skips the linear scan that `insert()` does.
    pub(crate) fn push(&mut self, key: &'static str, value: String) {
        self.entries.push((Cow::Borrowed(key), value));
    }

    pub fn remove(&mut self, key: &str) -> Option<String> {
        if let Some(pos) = self.entries.iter().position(|(k, _)| k.as_ref() == key) {
            Some(self.entries.swap_remove(pos).1)
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &String)> {
        self.entries.iter().map(|(k, v)| (k.as_ref(), v))
    }
}

impl FromIterator<(String, String)> for PathParams {
    fn from_iter<I: IntoIterator<Item = (String, String)>>(iter: I) -> Self {
        Self {
            entries: iter.into_iter().map(|(k, v)| (Cow::Owned(k), v)).collect(),
        }
    }
}

pub fn extract_path_params(pattern: &str, path: &str) -> Option<PathParams> {
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    if pattern_parts.len() != path_parts.len() {
        return None;
    }

    let mut params = PathParams::new();

    for (pattern_part, path_part) in pattern_parts.iter().zip(path_parts.iter()) {
        if let Some(param_name) = pattern_part.strip_prefix(':') {
            params.insert(param_name.to_string(), path_part.to_string());
        } else if pattern_part != path_part {
            return None;
        }
    }

    Some(params)
}

// ── PathParamsDeserializer ────────────────────────────────────────────────────
//
// Teaches serde how to read from PathParams so a single
// `impl<T: DeserializeOwned> FromRequestParts for Path<T>` can handle:
//   - Path<u64> / Path<String>  → first param value
//   - Path<(u64, String)>       → params in insertion order (SeqAccess)
//   - Path<MyStruct>            → params by field name (MapAccess)

#[derive(Debug)]
struct PathDeError(String);

impl fmt::Display for PathDeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PathDeError {}

impl de::Error for PathDeError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        PathDeError(msg.to_string())
    }
}

struct PathParamsDeserializer<'de>(&'de PathParams);

impl<'de> PathParamsDeserializer<'de> {
    fn first_str(&self) -> Result<&'de str, PathDeError> {
        self.0
            .iter()
            .next()
            .map(|(_, v)| v.as_str())
            .ok_or_else(|| {
                de::Error::custom(
                    "Missing path parameter. Ensure your route pattern includes a parameter like /:id",
                )
            })
    }
}

// Generates scalar deserialize methods for PathParamsDeserializer by delegating
// to StrDeserializer, which holds the actual parse logic.
macro_rules! delegate_scalar {
    ($($method:ident),+ $(,)?) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
            StrDeserializer(self.first_str()?).$method(visitor)
        })+
    };
}

impl<'de> de::Deserializer<'de> for PathParamsDeserializer<'de> {
    type Error = PathDeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        match self.0.len() {
            0 => Err(de::Error::custom("missing path parameter")),
            1 => visitor.visit_str(self.first_str()?),
            _ => self.deserialize_seq(visitor),
        }
    }

    delegate_scalar! {
        deserialize_bool,
        deserialize_i8, deserialize_i16, deserialize_i32, deserialize_i64, deserialize_i128,
        deserialize_u8, deserialize_u16, deserialize_u32, deserialize_u64, deserialize_u128,
        deserialize_f32, deserialize_f64,
        deserialize_str, deserialize_string, deserialize_identifier,
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        StrDeserializer(self.first_str()?).deserialize_option(visitor)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathDeError> {
        StrDeserializer(self.first_str()?).deserialize_newtype_struct(name, visitor)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_seq(PathParamsSeqAccess {
            iter: self.0.entries.iter(),
        })
    }
    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, PathDeError> {
        self.deserialize_seq(visitor)
    }
    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, PathDeError> {
        self.deserialize_seq(visitor)
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_map(PathParamsMapAccess {
            iter: self.0.entries.iter(),
            pending_value: None,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, PathDeError> {
        self.deserialize_map(visitor)
    }

    serde::forward_to_deserialize_any! {
        char bytes byte_buf unit unit_struct enum ignored_any
    }
}

// Iterates param values in insertion order (for tuples).
struct PathParamsSeqAccess<'de> {
    iter: std::slice::Iter<'de, (Cow<'static, str>, String)>,
}

impl<'de> SeqAccess<'de> for PathParamsSeqAccess<'de> {
    type Error = PathDeError;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, PathDeError> {
        match self.iter.next() {
            None => Ok(None),
            Some((_, val)) => seed.deserialize(StrDeserializer(val.as_str())).map(Some),
        }
    }
}

// Iterates param key-value pairs (for structs).
struct PathParamsMapAccess<'de> {
    iter: std::slice::Iter<'de, (Cow<'static, str>, String)>,
    pending_value: Option<&'de str>,
}

impl<'de> MapAccess<'de> for PathParamsMapAccess<'de> {
    type Error = PathDeError;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, PathDeError> {
        match self.iter.next() {
            None => Ok(None),
            Some((key, val)) => {
                self.pending_value = Some(val.as_str());
                seed.deserialize(StrDeserializer(key.as_ref())).map(Some)
            }
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, PathDeError> {
        let val = self
            .pending_value
            .take()
            .ok_or_else(|| de::Error::custom("next_value called before next_key"))?;
        seed.deserialize(StrDeserializer(val))
    }
}

// Deserializes a single &str into any primitive — used by SeqAccess and MapAccess.
struct StrDeserializer<'de>(&'de str);

// Generates parse-and-visit methods for StrDeserializer.
macro_rules! parse_scalar {
    ($($method:ident => $visit:ident: $ty:ty),+ $(,)?) => {
        $(fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
            visitor.$visit(self.0.parse::<$ty>().map_err(de::Error::custom)?)
        })+
    };
}

impl<'de> de::Deserializer<'de> for StrDeserializer<'de> {
    type Error = PathDeError;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_str(self.0)
    }

    parse_scalar! {
        deserialize_bool  => visit_bool:  bool,
        deserialize_i8    => visit_i8:    i8,
        deserialize_i16   => visit_i16:   i16,
        deserialize_i32   => visit_i32:   i32,
        deserialize_i64   => visit_i64:   i64,
        deserialize_i128  => visit_i128:  i128,
        deserialize_u8    => visit_u8:    u8,
        deserialize_u16   => visit_u16:   u16,
        deserialize_u32   => visit_u32:   u32,
        deserialize_u64   => visit_u64:   u64,
        deserialize_u128  => visit_u128:  u128,
        deserialize_f32   => visit_f32:   f32,
        deserialize_f64   => visit_f64:   f64,
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_str(self.0)
    }
    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_string(self.0.to_owned())
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_str(self.0)
    }
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, PathDeError> {
        visitor.visit_some(self)
    }
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, PathDeError> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        char bytes byte_buf unit unit_struct seq tuple tuple_struct
        map struct enum ignored_any
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::{TestRequest, empty_params, empty_state, params};

    // Path params extraction tests
    #[test]
    fn test_extract_path_params_exact_match() {
        let result = extract_path_params("/users", "/users");
        assert!(result.is_some());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_extract_path_params_single_param() {
        let result = extract_path_params("/users/:id", "/users/123");
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }

    #[test]
    fn test_extract_path_params_multiple_params() {
        let result = extract_path_params("/users/:user_id/posts/:post_id", "/users/1/posts/42");
        assert!(result.is_some());
        let params = result.unwrap();
        assert_eq!(params.get("user_id"), Some(&"1".to_string()));
        assert_eq!(params.get("post_id"), Some(&"42".to_string()));
    }

    #[test]
    fn test_extract_path_params_no_match_different_length() {
        let result = extract_path_params("/users/:id", "/users/123/extra");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_path_params_no_match_different_static() {
        let result = extract_path_params("/users/:id", "/posts/123");
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_path_params_root() {
        let result = extract_path_params("/", "/");
        assert!(result.is_some());
    }

    // Path extractor tests
    #[tokio::test]
    async fn test_path_extractor_u64() {
        let (parts, _) = TestRequest::get("/users/123").into_parts();
        let params = params(&[("id", "123")]);

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, 123);
    }

    #[tokio::test]
    async fn test_path_extractor_string() {
        let (parts, _) = TestRequest::get("/users/john").into_parts();
        let params = params(&[("name", "john")]);

        let result = Path::<String>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, "john");
    }

    #[tokio::test]
    async fn test_path_extractor_invalid_type() {
        let (parts, _) = TestRequest::get("/users/notanumber").into_parts();
        let params = params(&[("id", "notanumber")]);

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }

    #[tokio::test]
    async fn test_path_extractor_missing_param() {
        let (parts, _) = TestRequest::get("/users").into_parts();
        let params = empty_params();

        let result = Path::<u64>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_path_extractor_uuid() {
        let id = uuid::Uuid::new_v4();
        let (parts, _) = TestRequest::get(&format!("/users/{id}")).into_parts();
        let params = params(&[("id", &id.to_string())]);

        let result = Path::<uuid::Uuid>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, id);
    }

    #[tokio::test]
    async fn test_path_extractor_uuid_invalid() {
        let (parts, _) = TestRequest::get("/users/not-a-uuid").into_parts();
        let params = params(&[("id", "not-a-uuid")]);

        let result = Path::<uuid::Uuid>::from_request_parts(&parts, &params, &empty_state()).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }

    #[test]
    fn test_path_into_inner() {
        let path = Path(42u64);
        assert_eq!(path.into_inner(), 42);
    }

    #[test]
    fn test_path_deref() {
        let path = Path(42u64);
        assert_eq!(*path, 42);
    }

    #[tokio::test]
    async fn test_path_tuple_two_params() {
        let p = params(&[("org_id", "10"), ("team_id", "42")]);
        let result = Path::<(u64, u64)>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, (10, 42));
    }

    #[tokio::test]
    async fn test_path_tuple_three_params() {
        let p = params(&[("org_id", "1"), ("team_id", "2"), ("member_id", "3")]);
        let result = Path::<(u64, u64, u64)>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().0, (1, 2, 3));
    }

    #[tokio::test]
    async fn test_path_tuple_missing_param() {
        let p = params(&[("org_id", "1")]);
        let result = Path::<(u64, u64)>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_path_tuple_bad_parse() {
        let p = params(&[("org_id", "not_a_number"), ("team_id", "2")]);
        let result = Path::<(u64, u64)>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_path_struct_params() {
        #[derive(serde::Deserialize)]
        struct OrgParams {
            org_id: u64,
            team_id: u64,
        }

        let p = params(&[("org_id", "10"), ("team_id", "42")]);
        let result = Path::<OrgParams>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_ok());
        let Path(v) = result.unwrap();
        assert_eq!(v.org_id, 10);
        assert_eq!(v.team_id, 42);
    }

    #[tokio::test]
    async fn test_path_struct_bad_parse() {
        #[derive(Debug, serde::Deserialize)]
        #[allow(dead_code)]
        struct OrgParams {
            org_id: u64,
            team_id: u64,
        }

        let p = params(&[("org_id", "not_a_number"), ("team_id", "42")]);
        let result = Path::<OrgParams>::from_request_parts(
            &TestRequest::get("/").into_parts().0,
            &p,
            &empty_state(),
        )
        .await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().status(), 400);
    }
}
