//! Arena-allocated radix trie for dynamic route matching.
//!
//! Built once at `freeze()` time, then immutable for all lookups.
//! One trie per HTTP method. Byte-level prefix compression with
//! separate param children for `:name` segments.
//!
//! # Invariant: params consume full segments
//!
//! The trie assumes every `:param` consumes an entire path segment
//! (all bytes between `/` delimiters). There is no support for
//! suffix patterns like `/:name.txt` or inline params like `/:year-:month`.
//!
//! The lookup uses limited backtracking: when a static child's prefix
//! matches but its subtree fails, the algorithm falls back to the param
//! child at the same node. This handles overlapping patterns like
//! `/api/v1/:resource` vs `/api/:version/users` correctly. At most one
//! retry per node (static → param), giving O(2^D) worst case where D
//! is path depth. For typical REST APIs (D = 5-8), this is effectively
//! O(D) and only triggers when routes have overlapping static/param
//! prefixes at the same depth.
//!
//! **WARNING:** This backtracking relies on the full-segment invariant.
//! If suffix patterns (e.g. `/:name.txt`) are ever added, the param
//! child can no longer consume a clean segment boundary, and the
//! backtracking logic would produce incorrect matches silently.
//! Suffix support requires a fundamentally different traversal strategy.

use http::Method;

use crate::extract::PathParams;

// ── Method indexing ─────────────────────────────────────────────────

const NUM_STANDARD_METHODS: usize = 9;

/// Map standard HTTP methods to array indices for zero-cost dispatch.
/// Returns `None` for extension methods.
fn method_index(m: &Method) -> Option<usize> {
    match m.as_str() {
        "GET" => Some(0),
        "POST" => Some(1),
        "PUT" => Some(2),
        "DELETE" => Some(3),
        "HEAD" => Some(4),
        "OPTIONS" => Some(5),
        "PATCH" => Some(6),
        "CONNECT" => Some(7),
        "TRACE" => Some(8),
        _ => None,
    }
}

// ── Arena node ───────────────────────────────────────────────────────

struct Node {
    /// Static byte prefix this edge matches (16 bytes vs 24 for Vec).
    prefix: Box<[u8]>,
    /// First byte of each static child's prefix (parallel to `children`).
    indices: Vec<u8>,
    /// Arena indices of static children (parallel to `indices`).
    children: Vec<usize>,
    /// Arena index of the `:param` child, if any. At most one per node.
    param_child: Option<usize>,
    /// If this node was reached via a param edge, the param name (without `:`).
    /// Leaked to `&'static str` at build time — the trie lives for the entire
    /// application lifetime so this is never reclaimed (and it's a handful of
    /// bytes total for any realistic route table).
    param_name: Option<&'static str>,
    /// Route index if this is a terminal node.
    value: Option<usize>,
    /// Number of routes reachable through this subtree.
    priority: u32,
}

impl Node {
    fn new() -> Self {
        Self {
            prefix: Box::default(),
            indices: Vec::new(),
            children: Vec::new(),
            param_child: None,
            param_name: None,
            value: None,
            priority: 0,
        }
    }

    fn with_prefix(prefix: Vec<u8>) -> Self {
        Self {
            prefix: prefix.into_boxed_slice(),
            ..Self::new()
        }
    }
}

// ── Pattern segments ─────────────────────────────────────────────────

enum Segment<'a> {
    Static(&'a [u8]),
    Param(&'a str),
}

/// Split a route pattern into alternating static/param segments.
///
/// `/users/:id/posts/:pid` → [Static("/users/"), Param("id"), Static("/posts/"), Param("pid")]
///
/// # Panics
///
/// Panics if a `:param` is not a full segment (e.g. `/files/:name.txt`).
fn split_pattern(pattern: &str) -> Vec<Segment<'_>> {
    let mut segments = Vec::new();
    let bytes = pattern.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b':' {
            // Param segment — consume until next '/' or end.
            let start = i + 1; // skip ':'
            let end = bytes[start..]
                .iter()
                .position(|&b| b == b'/')
                .map(|p| start + p)
                .unwrap_or(bytes.len());

            // Enforce full-segment invariant: the param must span the
            // entire segment between '/' delimiters. A colon mid-segment
            // (e.g. `/files/:name.txt`) would break the no-backtracking
            // guarantee in lookup.
            assert!(
                end == bytes.len() || bytes[end] == b'/',
                "param `{}` in pattern `{}` does not consume a full segment — \
                 suffix patterns like `:name.txt` are not supported",
                &pattern[start..end],
                pattern,
            );

            segments.push(Segment::Param(&pattern[start..end]));
            i = end;
        } else {
            // Static segment — consume until next ':' or end.
            let start = i;
            let end = bytes[start..]
                .iter()
                .position(|&b| b == b':')
                .map(|p| start + p)
                .unwrap_or(bytes.len());
            segments.push(Segment::Static(&bytes[start..end]));
            i = end;
        }
    }

    segments
}

// ── Radix trie ───────────────────────────────────────────────────────

struct RadixTrie {
    arena: Vec<Node>,
}

impl RadixTrie {
    fn new() -> Self {
        Self {
            arena: vec![Node::new()],
        }
    }

    fn insert(&mut self, pattern: &str, route_index: usize) {
        let segments = split_pattern(pattern);
        let mut current = 0;

        for seg in &segments {
            match seg {
                Segment::Static(bytes) => {
                    current = self.insert_static(current, bytes);
                }
                Segment::Param(name) => {
                    if let Some(child_id) = self.arena[current].param_child {
                        // Validate that the existing param name matches.
                        if let Some(existing) = self.arena[child_id].param_name {
                            assert!(
                                existing == *name,
                                "conflicting param names at the same position: \
                                 `:{existing}` and `:{name}` in pattern `{pattern}` — \
                                 all routes sharing this param position must use the same name",
                            );
                        }
                    } else {
                        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
                        let id = self.alloc(Node {
                            param_name: Some(leaked),
                            ..Node::new()
                        });
                        self.arena[current].param_child = Some(id);
                    }
                    current = self.arena[current].param_child.unwrap();
                }
            }
        }

        self.arena[current].value = Some(route_index);
    }

    fn insert_static(&mut self, start: usize, key: &[u8]) -> usize {
        if key.is_empty() {
            return start;
        }

        let mut current = start;

        let mut remaining = key;

        loop {
            let prefix_len = common_prefix_len(&self.arena[current].prefix, remaining);

            // Need to split this node?
            if prefix_len < self.arena[current].prefix.len() {
                self.split_node(current, prefix_len);
            }

            remaining = &remaining[prefix_len..];

            if remaining.is_empty() {
                return current;
            }

            let next = remaining[0];

            // Existing child for this byte?
            if let Some(pos) = self.arena[current].indices.iter().position(|&b| b == next) {
                current = self.arena[current].children[pos];
                continue;
            }

            // Create new child with the remaining bytes as prefix.
            let child = self.alloc(Node::with_prefix(remaining.to_vec()));
            self.arena[current].indices.push(next);
            self.arena[current].children.push(child);
            return child;
        }
    }

    fn split_node(&mut self, node: usize, at: usize) {
        let suffix = self.arena[node].prefix[at..].into();
        let first_byte = self.arena[node].prefix[at];

        // Build the child node before calling alloc to avoid double
        // mutable borrow of self.arena.
        let child_node = Node {
            prefix: suffix,
            indices: std::mem::take(&mut self.arena[node].indices),
            children: std::mem::take(&mut self.arena[node].children),
            param_child: self.arena[node].param_child.take(),
            param_name: self.arena[node].param_name.take(),
            value: self.arena[node].value.take(),
            priority: self.arena[node].priority,
        };
        let child = self.alloc(child_node);

        self.arena[node].prefix = self.arena[node].prefix[..at].into();
        self.arena[node].indices = vec![first_byte];
        self.arena[node].children = vec![child];
    }

    /// Compute subtree route counts in a single DFS pass. Called once
    /// after all inserts, before `reorder_children`.
    fn compute_priorities(&mut self) {
        self.compute_priorities_dfs(0);
    }

    fn compute_priorities_dfs(&mut self, node_id: usize) -> u32 {
        let mut count = if self.arena[node_id].value.is_some() {
            1
        } else {
            0
        };

        // Recurse into static children (index-based to avoid cloning).
        let num_children = self.arena[node_id].children.len();
        for i in 0..num_children {
            let child = self.arena[node_id].children[i];
            count += self.compute_priorities_dfs(child);
        }

        // Recurse into param child.
        if let Some(param) = self.arena[node_id].param_child {
            count += self.compute_priorities_dfs(param);
        }

        self.arena[node_id].priority = count;
        count
    }

    /// Sort each node's static children by descending priority so the
    /// most popular subtrees are tried first during lookup.
    fn reorder_children(&mut self) {
        for i in 0..self.arena.len() {
            if self.arena[i].children.len() <= 1 {
                continue;
            }

            // Build (priority, index, byte) tuples, sort descending by priority.
            let mut order: Vec<(u32, usize, u8)> = self.arena[i]
                .children
                .iter()
                .zip(self.arena[i].indices.iter())
                .map(|(&child, &byte)| (self.arena[child].priority, child, byte))
                .collect();
            order.sort_by_key(|t| std::cmp::Reverse(t.0));

            self.arena[i].children = order.iter().map(|&(_, c, _)| c).collect();
            self.arena[i].indices = order.iter().map(|&(_, _, b)| b).collect();
        }
    }

    fn lookup(&self, path: &str, params: &mut PathParams) -> Option<usize> {
        self.lookup_recursive(0, path, params)
    }

    /// Recursive lookup with backtracking.
    ///
    /// `path` is the remaining portion of the original request path (always
    /// valid UTF-8 since it comes from `uri.path()`). We index into bytes
    /// for prefix matching but slice the `&str` directly for param values,
    /// avoiding any UTF-8 re-validation.
    ///
    /// Worst-case complexity is O(2^D) where D is path depth, because at
    /// each node we may try a static child then fall back to the param
    /// child. In practice D is 5-8 for typical REST APIs, making this
    /// effectively O(D).
    fn lookup_recursive(
        &self,
        node_id: usize,
        path: &str,
        params: &mut PathParams,
    ) -> Option<usize> {
        let node = &self.arena[node_id];
        let path_bytes = path.as_bytes();

        // Check prefix.
        if !path_bytes.starts_with(&node.prefix) {
            return None;
        }
        let consumed = node.prefix.len();
        let remaining = &path[consumed..];

        // Reached the end of the path?
        if remaining.is_empty() {
            return node.value;
        }

        let remaining_bytes = remaining.as_bytes();

        // Try static children first (priority-ordered).
        let next = remaining_bytes[0];
        for (i, &byte) in node.indices.iter().enumerate() {
            if byte == next {
                let saved_len = params.len();
                if let Some(result) = self.lookup_recursive(node.children[i], remaining, params) {
                    return Some(result);
                }
                // Static child failed — params should be unchanged because
                // the recursive call cleans up after itself (param nodes
                // remove their key on failure). If this assert fires,
                // there's a bug in the trie's backtracking cleanup.
                debug_assert_eq!(
                    params.len(),
                    saved_len,
                    "params leaked during static child backtracking"
                );
                break; // Only one static child can match the first byte.
            }
        }

        // Try param child — consumes bytes up to next '/' or end.
        if let Some(param_idx) = node.param_child {
            let end = remaining_bytes
                .iter()
                .position(|&b| b == b'/')
                .unwrap_or(remaining_bytes.len());

            // Slice the &str directly — no from_utf8 needed since `path`
            // is already valid UTF-8 from uri.path().
            let value = &remaining[..end];
            let param_node = &self.arena[param_idx];
            if let Some(name) = param_node.param_name {
                params.push(name, value.to_string());
            }

            if let Some(result) = self.lookup_recursive(param_idx, &remaining[end..], params) {
                return Some(result);
            }

            // Param child also failed — remove the param we just inserted.
            if let Some(name) = param_node.param_name {
                params.remove(name);
            }
        }

        None
    }

    fn alloc(&mut self, node: Node) -> usize {
        let id = self.arena.len();
        self.arena.push(node);
        id
    }
}

fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

// ── Method-dispatched wrapper ────────────────────────────────────────

pub(super) struct TrieRouter {
    /// Fixed-size array for the 9 standard HTTP methods (zero-cost index).
    methods: [Option<RadixTrie>; NUM_STANDARD_METHODS],
}

impl TrieRouter {
    pub(super) fn build(routes: &[(Method, super::Route)]) -> Self {
        const INIT: Option<RadixTrie> = None;
        let mut methods = [INIT; NUM_STANDARD_METHODS];

        for (idx, (method, route)) in routes.iter().enumerate() {
            if super::is_dynamic(&route.pattern) {
                let mi = method_index(method).unwrap_or_else(|| {
                    panic!(
                        "unsupported HTTP method `{}` for route `{}` — \
                         only standard methods (GET, POST, PUT, DELETE, HEAD, \
                         OPTIONS, PATCH, CONNECT, TRACE) are supported",
                        method, route.pattern,
                    )
                });
                methods[mi]
                    .get_or_insert_with(RadixTrie::new)
                    .insert(&route.pattern, idx);
            }
        }

        for slot in &mut methods {
            if let Some(trie) = slot.as_mut() {
                trie.compute_priorities();
                trie.reorder_children();
            }
        }

        Self { methods }
    }

    pub(super) fn lookup(
        &self,
        method: &Method,
        path: &str,
        params: &mut PathParams,
    ) -> Option<usize> {
        let idx = method_index(method)?;
        self.methods[idx].as_ref()?.lookup(path, params)
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup_params(trie: &RadixTrie, path: &str) -> (Option<usize>, PathParams) {
        let mut params = PathParams::new();
        let result = trie.lookup(path, &mut params);
        (result, params)
    }

    #[test]
    fn test_single_param_route() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id", 0);

        let (result, params) = lookup_params(&trie, "/users/42");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_multiple_param_routes() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id", 0);
        trie.insert("/posts/:id", 1);

        let (result, params) = lookup_params(&trie, "/users/42");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("id").unwrap(), "42");

        let (result, params) = lookup_params(&trie, "/posts/99");
        assert_eq!(result, Some(1));
        assert_eq!(params.get("id").unwrap(), "99");
    }

    #[test]
    fn test_nested_params() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:uid/posts/:pid", 0);

        let (result, params) = lookup_params(&trie, "/users/5/posts/10");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("uid").unwrap(), "5");
        assert_eq!(params.get("pid").unwrap(), "10");
    }

    #[test]
    fn test_param_with_deeper_static() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id", 0);
        trie.insert("/users/:id/posts", 1);

        let (result, _) = lookup_params(&trie, "/users/42");
        assert_eq!(result, Some(0));

        let (result, params) = lookup_params(&trie, "/users/42/posts");
        assert_eq!(result, Some(1));
        assert_eq!(params.get("id").unwrap(), "42");
    }

    #[test]
    fn test_shared_prefix_divergence() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id/posts", 0);
        trie.insert("/users/:id/comments", 1);

        let (result, _) = lookup_params(&trie, "/users/1/posts");
        assert_eq!(result, Some(0));

        let (result, _) = lookup_params(&trie, "/users/1/comments");
        assert_eq!(result, Some(1));
    }

    #[test]
    fn test_no_match() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id", 0);

        let (result, _) = lookup_params(&trie, "/posts/42");
        assert_eq!(result, None);

        let (result, _) = lookup_params(&trie, "/users");
        assert_eq!(result, None);

        let (result, _) = lookup_params(&trie, "/users/42/extra");
        assert_eq!(result, None);
    }

    #[test]
    fn test_different_param_names_same_structure() {
        let mut trie = RadixTrie::new();
        trie.insert("/api/:version/users/:id", 0);

        let (result, params) = lookup_params(&trie, "/api/v2/users/99");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("version").unwrap(), "v2");
        assert_eq!(params.get("id").unwrap(), "99");
    }

    #[test]
    fn test_param_at_root() {
        let mut trie = RadixTrie::new();
        trie.insert("/:slug", 0);

        let (result, params) = lookup_params(&trie, "/hello");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("slug").unwrap(), "hello");
    }

    #[test]
    fn test_priority_ordering() {
        let mut trie = RadixTrie::new();
        // Insert routes where /api/v1/... subtree has more routes
        trie.insert("/api/v1/users/:id", 0);
        trie.insert("/api/v1/posts/:id", 1);
        trie.insert("/api/v1/comments/:id", 2);
        trie.insert("/api/v2/users/:id", 3);
        trie.compute_priorities();
        trie.reorder_children();

        // After priority reordering, v1 subtree (3 routes) should be
        // tried before v2 subtree (1 route). Verify by checking that
        // the v1 child comes first in the indices array.
        // Root → "/" → "api/v" → then children "1/" and "2/"
        // Find the node whose children are "1/" and "2/".
        let v_node = trie.arena.iter().find(|n| n.children.len() == 2).unwrap();
        assert_eq!(
            v_node.indices[0], b'1',
            "v1 subtree should be first (higher priority)"
        );
        assert_eq!(
            v_node.indices[1], b'2',
            "v2 subtree should be second (lower priority)"
        );

        // All should still resolve correctly after reordering
        let (result, _) = lookup_params(&trie, "/api/v1/users/1");
        assert_eq!(result, Some(0));

        let (result, _) = lookup_params(&trie, "/api/v1/posts/1");
        assert_eq!(result, Some(1));

        let (result, _) = lookup_params(&trie, "/api/v2/users/1");
        assert_eq!(result, Some(3));
    }

    #[test]
    fn test_trie_router_method_isolation() {
        let router = crate::router::Router::new()
            .route(Method::GET, "/users/:id", |_, _, _| async {
                http::StatusCode::OK
            })
            .route(Method::DELETE, "/users/:id", |_, _, _| async {
                http::StatusCode::NO_CONTENT
            });

        let trie_router = TrieRouter::build(&router.routes);
        let mut params = PathParams::new();

        assert!(
            trie_router
                .lookup(&Method::GET, "/users/1", &mut params)
                .is_some()
        );
        params.clear();
        assert!(
            trie_router
                .lookup(&Method::DELETE, "/users/1", &mut params)
                .is_some()
        );
        params.clear();
        assert!(
            trie_router
                .lookup(&Method::POST, "/users/1", &mut params)
                .is_none()
        );
    }

    #[test]
    fn test_empty_trie() {
        let trie = RadixTrie::new();
        let (result, _) = lookup_params(&trie, "/anything");
        assert_eq!(result, None);
    }

    #[test]
    #[should_panic(expected = "conflicting param names")]
    fn test_conflicting_param_names_panics() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id/posts", 0);
        trie.insert("/users/:name/comments", 1); // :name conflicts with :id
    }

    #[test]
    fn test_same_param_name_no_conflict() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id/posts", 0);
        trie.insert("/users/:id/comments", 1); // same name, no conflict
        let (result, _) = lookup_params(&trie, "/users/1/posts");
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_split_pattern_invariant() {
        // Valid patterns should not panic
        split_pattern("/users/:id");
        split_pattern("/users/:id/posts/:pid");
        split_pattern("/:slug");
        split_pattern("/api/:version/users");
    }

    #[test]
    fn test_backtracking_static_to_param() {
        // Static child "bb/" matches first byte 'b' but full prefix
        // diverges for input "bc" — must backtrack to param child.
        let mut trie = RadixTrie::new();
        trie.insert("/a/bb/:x", 0);
        trie.insert("/a/:y", 1);
        trie.compute_priorities();
        trie.reorder_children();

        // "bc" starts with 'b' (matching "bb/" child) but isn't "bb/..."
        // so the trie must fall back to param ":y"
        let (result, params) = lookup_params(&trie, "/a/bc");
        assert_eq!(result, Some(1));
        assert_eq!(params.get("y").unwrap(), "bc");

        // "bb/42" fully matches the static child path
        let (result, params) = lookup_params(&trie, "/a/bb/42");
        assert_eq!(result, Some(0));
        assert_eq!(params.get("x").unwrap(), "42");

        // "z" doesn't match 'b' at all — goes straight to param
        let (result, params) = lookup_params(&trie, "/a/z");
        assert_eq!(result, Some(1));
        assert_eq!(params.get("y").unwrap(), "z");
    }

    #[test]
    fn test_param_with_multiple_static_children() {
        let mut trie = RadixTrie::new();
        trie.insert("/users/:id/posts", 0);
        trie.insert("/users/:id/comments", 1);
        trie.insert("/users/:id/likes", 2);

        let (result, _) = lookup_params(&trie, "/users/1/posts");
        assert_eq!(result, Some(0));

        let (result, _) = lookup_params(&trie, "/users/1/comments");
        assert_eq!(result, Some(1));

        let (result, _) = lookup_params(&trie, "/users/1/likes");
        assert_eq!(result, Some(2));

        let (result, _) = lookup_params(&trie, "/users/1/other");
        assert_eq!(result, None);
    }
}
