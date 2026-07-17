use proc_macro::TokenStream;
use proc_macro2::Ident;
use quote::{ToTokens, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Error, FnArg, ItemFn, LitStr, Pat, PatIdent, PatType, Token, Type, parenthesized,
};

/// Parsed route macro attribute: `"/path"`, `"/path", group = "/prefix"`,
/// `"/path", description = "..."`, or any combination thereof.
struct RouteAttr {
    path: LitStr,
    group: Option<LitStr>,
    description: Option<LitStr>,
}

impl syn::parse::Parse for RouteAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        let mut group: Option<LitStr> = None;
        let mut description: Option<LitStr> = None;

        while input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
            if input.is_empty() {
                break;
            }
            let ident: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            if ident == "group" {
                let value: LitStr = input.parse()?;
                group = Some(value);
            } else if ident == "description" {
                let value: LitStr = input.parse()?;
                description = Some(value);
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    "expected `group` or `description`",
                ));
            }
        }

        if !input.is_empty() {
            return Err(input.error("unexpected tokens after route attribute"));
        }
        Ok(RouteAttr {
            path,
            group,
            description,
        })
    }
}

/// Join a group prefix with a route path at compile time.
fn join_paths(prefix: &str, path: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    if path.is_empty() || path == "/" {
        if prefix.is_empty() {
            return "/".to_string();
        }
        return prefix.to_string();
    }
    let path = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{prefix}{path}")
}
use quote::quote;
use syn::ItemFn;

mod config;
mod job;
mod relay;
mod route;
mod schema;

use route::route_macro;

/// Parsed `#[authorize(...)]` arguments.
///
/// Supported forms:
/// - `#[authorize(auth_fn)]` for zero-dependency authorization
/// - `#[authorize(auth_fn(Dep1, Dep2, ...))]` for authorization with
///   explicitly declared dependency types
///
/// `auth_fn` is the path to the async authorization function to invoke before
/// the handler runs. `deps` lists the dependency types that should be extracted
/// and passed to that function.
struct AuthorizeArgs {
    auth_fn: syn::Path,
    deps: Vec<Type>,
}

/// Parses the arguments of `#[authorize(...)]` into an [`AuthorizeArgs`].
///
/// Supported forms:
/// - `auth_fn`
/// - `auth_fn(Dep1, Dep2, ...)`
///
/// The bare-path form represents a zero-dependency authorization function.
/// When dependencies are present, they must be provided as a parenthesized,
/// comma-separated list of types.
///
/// # Errors
///
/// Returns a parse error if trailing tokens are present after the function path
impl Parse for AuthorizeArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let auth_fn: syn::Path = input.parse()?;

        if input.is_empty() {
            return Ok(Self {
                auth_fn,
                deps: Vec::new(),
            });
        }

        let deps = if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            let parsed: Punctuated<Type, Token![,]> =
                content.parse_terminated(Type::parse, Token![,])?;

            if !input.is_empty() {
                return Err(input.error("unexpected tokens after authorization dependencies"));
            }

            parsed.into_iter().collect()
        } else {
            return Err(syn::Error::new(
                input.span(),
                "expected dependency list in parentheses, e.g. #[authorize(auth_fn(Dep1, Dep2))]",
            ));
        };

        Ok(Self { auth_fn, deps })
    }
}

/// Generated authorization code split into extraction and invocation phases.
///
/// The phases must remain separate because authorization-only dependencies need
/// request parts, while reused handler dependencies are not in scope until the
/// route's normal extractor bindings have been created.
struct AuthorizePlan {
    /// Dependencies not present in the handler signature. These are extracted
    /// from request parts before handler extraction consumes the request.
    extracts: proc_macro2::TokenStream,

    /// Invokes the authorization function after reusable handler bindings have
    /// been created.
    call: proc_macro2::TokenStream,

    /// Whether `extracts` needs access to `__rapina_parts`.
    needs_request_parts: bool,
}

/// Builds the generated authorization plan for a route handler.
///
/// Authorization dependencies fall into two categories:
///
/// - **Reused handler dependencies**: if a dependency's type matches a handler parameter
///   type, the generated authorization handler call borrows the handler binding.
/// - **Authorization-only dependencies**: if no handler parameter matches, the
///   dependency is extracted separately through `rapina::extract::FromRequestParts` before it is invoked.
///
/// Extraction and policy invocation are deliberately returned as separate token
/// streams. Authorization-only dependencies must be extracted while request
/// parts are available, whereas the policy call must happen only after reusable
/// handler parameters have been extracted and bound. Keeping these phases
/// separate prevents generated references to handler bindings before those
/// bindings are in scope.
///
/// Type matching is syntactic and whitespace-insensitive; it does not resolve
/// aliases or determine semantic Rust type equality. For example,
/// `State<AppState>` and `rapina::extract::State<AppState>` are treated as
/// different types and result in separate extraction.
///
/// # Errors
///
/// Returns an error if a reused handler parameter does not use a simple
/// identifier pattern and therefore cannot be referenced from generated code.
fn build_authorize_plan(
    inputs: &Punctuated<FnArg, Token![,]>,
    auth: &AuthorizeArgs,
) -> syn::Result<AuthorizePlan> {
    let auth_fn = &auth.auth_fn;

    let mut extracts = Vec::new();
    let mut arguments = Vec::with_capacity(auth.deps.len());
    let mut needs_request_parts = false;

    for (index, dependency_type) in auth.deps.iter().enumerate() {
        let normalized_dependency = normalize_type(dependency_type);

        // Prefer an existing handler parameter over extracting the same syntactically
        // matching dependency a second time.
        let matching_handler_parameter = inputs.iter().find_map(|input| {
            let FnArg::Typed(PatType { pat, ty, .. }) = input else {
                return None;
            };

            if normalize_type(ty) == normalized_dependency {
                Some(pat)
            } else {
                None
            }
        });

        if let Some(pattern) = matching_handler_parameter {
            let identifier = extract_ident(pattern)?;
            arguments.push(quote!(&#identifier));
            continue;
        }

        // The parameter was not found in the handler parameters;
        // set flag to have it extracted from the Rapina request parts later
        needs_request_parts = true;

        let temporary = syn::Ident::new(
            &format!("__rapina_auth_dep_{index}"),
            proc_macro2::Span::call_site(),
        );

        // Authorization-only dependencies must implement FromRequestParts. Body-consuming
        // extractors cannot be used here because the request body must remain available to the route handler.
        extracts.push(quote! {
            let #temporary =
                match <#dependency_type as rapina::extract::FromRequestParts>::from_request_parts(
                    &__rapina_parts,
                    &__rapina_params,
                    &__rapina_state,
                )
                .await
                {
                    Ok(value) => value,
                    Err(error) => {
                        return rapina::response::IntoResponse::into_response(error);
                    }
                };
        });

        arguments.push(quote!(&#temporary));
    }

    let extracts = quote! {
        #(#extracts)*
    };

    // Policy failures short-circuit request handling, ensuring that the route
    // body is never executed after authorization has been denied.
    let call = quote! {
        match #auth_fn(#(#arguments),*).await {
            Ok(()) => {}
            Err(error) => {
                return rapina::response::IntoResponse::into_response(error);
            }
        }
    };

    Ok(AuthorizePlan {
        extracts,
        call,
        needs_request_parts,
    })
}

/// Extracts the identifier binding from a function parameter pattern.
///
/// `#[authorize]` only supports reusing handler parameters declared with simple
/// identifier patterns, such as `state: State<AppConfig>` or
/// `token: JsonWebToken<T>`.
///
/// Examples of unsupported patterns include destructuring bindings like
/// `State(state): State<AppConfig>`, tuple patterns like
/// `(a, b): (String, String)`, wildcard patterns like `_: State<AppConfig>`,
/// and other non-identifier parameter patterns.
///
/// # Errors
///
/// Returns a parse error if the pattern is not a simple identifier.
fn extract_ident(pat: &Pat) -> syn::Result<Ident> {
    match pat {
        Pat::Ident(PatIdent { ident, .. }) => Ok(ident.clone()),
        _ => Err(Error::new(
            pat.span(),
            "#[authorize] only supports simple identifier parameters, e.g. `state: State<AppConfig>`, `token: JsonWebToken<T>`",
        )),
    }
}

/// Normalizes a type into a whitespace-insensitive token string.
///
/// `syn::Type` stringification renders generics with spaces (e.g. `JsonWebToken < GoogleClaims >`),
/// so this makes `JsonWebToken<GoogleClaims>` compare equal textually.
///
/// This is syntactic normalization only, not semantic type equality.
fn normalize_type(ty: &Type) -> String {
    ty.to_token_stream().to_string().replace(' ', "")
}

/// Marks a route handler as requiring authorization before the handler body runs.
///
/// This attribute is only valid when used together with a Rapina route macro
/// such as `#[get]`, `#[post]`, or `#[put]`, and it must be placed **below**
/// that route macro so the route macro can read and process it during expansion.
///
/// Supported forms:
/// - `#[authorize(auth_fn)]` for zero-dependency authorization
/// - `#[authorize(auth_fn(Dep1, Dep2, ...))]` for authorization functions that
///   require extracted dependencies
///
/// Dependencies declared in the authorization function will reuse handler parameters
/// when the same extractor type is already present on the handler. They may also
/// be declared as Rapina extractors that are not present on the handler, in which case Rapina extracts them
/// before invoking the authorization function. Therefore, it is not required to put all dependencies
/// of the authorization function into the main handler parameter list. Rapina will handle it during compile-time.
///
/// The authorization function is invoked before the handler body. If it returns
/// an error, request handling stops and the error is converted into a response.
///
/// This attribute is a marker parsed by Rapina's route macros; using it on its
/// own always produces a compile error.
///
/// # Examples
///
/// ```ignore
/// #[get("/email")]
/// #[authorize(authz::authorize)]
/// async fn get_email() -> Result<Json<String>> {
///     // handler body
/// }
/// ```
///
/// ```ignore
/// #[get("/email")]
/// #[authorize(authz::authorize(JsonWebToken<Claims>, State<AppState>))]
/// async fn get_email(
///     token: JsonWebToken<Claims>
/// ) -> Result<Json<String>> {
///     // handler body
/// }
#[proc_macro_attribute]
pub fn authorize(_attr: TokenStream, _item: TokenStream) -> TokenStream {
    syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[authorize] must be used together with a route macro like #[get], #[post], #[put], etc., and placed below that route macro",
        )
        .to_compile_error()
        .into()
}

/// Extract #[authorize] attribute from function attributes, removing it if found.
fn extract_authorize_attr(attrs: &mut Vec<syn::Attribute>) -> Option<Attribute> {
    attrs
        .iter()
        .position(|attr| attr.path().is_ident("authorize"))
        .map(|idx| attrs.remove(idx))
}

/// Registers a GET route handler.
///
/// # Syntax
///
/// ```ignore
/// #[get("/users")]
/// async fn list_users() -> Json<Vec<User>> { /* ... */ }
///
/// // Single path parameter:
/// #[get("/users/:id")]
/// async fn get_user(id: Path<u64>) -> Json<User> { /* ... */ }
///
/// // Multiple path parameters — tuple, positional (left to right in pattern):
/// #[get("/orgs/:org_id/teams/:team_id")]
/// async fn get_team(Path((org_id, team_id)): Path<(u64, u64)>) -> Json<Team> { /* ... */ }
///
/// // With a group prefix (registers at /api/users):
/// #[get("/users", group = "/api")]
/// async fn list_users() -> Json<Vec<User>> { /* ... */ }
/// ```
///
/// The `group` parameter joins the prefix with the path at compile time,
/// so the handler is registered at the full path during auto-discovery.
#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("GET", attr, item)
}

/// Registers a POST route handler.
///
/// See [`get`] for syntax details including the optional `group` parameter.
#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("POST", attr, item)
}

/// Registers a PUT route handler.
///
/// See [`get`] for syntax details including the optional `group` parameter.
#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("PUT", attr, item)
}

/// Registers a PATCH route handler.
///
/// # Example
///
/// ```ignore
/// #[patch("/users/:id")]
/// async fn update_user(Path(id): Path<u64>) -> Json<User> { /* ... */ }
/// ```
///
/// See [`get`] for syntax details including the optional `group` parameter.
#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("PATCH", attr, item)
}

/// Registers a DELETE route handler.
///
/// See [`get`] for syntax details including the optional `group` parameter.
#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("DELETE", attr, item)
}

/// Marks a route as public (no authentication required).
///
/// When authentication is enabled via `Rapina::with_auth()`, all routes
/// require a valid JWT token by default. Use `#[public]` to allow
/// unauthenticated access to specific routes.
///
/// # Example
///
/// ```ignore
/// use rapina::prelude::*;
///
/// #[public]
/// #[get("/health")]
/// async fn health() -> &'static str {
///     "ok"
/// }
///
/// #[public]
/// #[post("/login")]
/// async fn login(body: Json<LoginRequest>) -> Result<Json<TokenResponse>> {
///     // ... authenticate and return token
/// }
/// ```
///
/// Note: Routes starting with `/__rapina` are automatically public.
#[proc_macro_attribute]
pub fn public(_attr: TokenStream, item: TokenStream) -> TokenStream {
    public_macro_impl(item).into()
}

fn public_macro_impl(item: TokenStream) -> proc_macro2::TokenStream {
    let mut func: ItemFn =
        syn::parse(item.clone()).expect("#[public] must be applied to a function");

    // Throw compilation error if the contradicting #[authorize] attribute is placed below the #[public] macro
    let authorize_attr = extract_authorize_attr(&mut func.attrs);
    if authorize_attr.is_some() {
        return Error::new(
            authorize_attr.span(),
            "#[authorize] contradicts #[public]. A public handler must not include authorization.",
        )
        .to_compile_error();
    }

    let func_name_str = func.sig.ident.to_string();
    let item2: proc_macro2::TokenStream = item.into();
    quote! {
        #item2
        rapina::inventory::submit! {
            rapina::discovery::PublicMarker {
                handler_name: #func_name_str,
            }
        }
    }
}

/// Registers a channel handler for the relay system.
///
/// Channel handlers receive [`RelayEvent`](rapina::relay::RelayEvent) events
/// when clients subscribe, send messages, or disconnect from matching topics.
///
/// The pattern supports exact matches and prefix matches (trailing `*`):
///
/// - `"chat:lobby"` — matches only the exact topic `"chat:lobby"`
/// - `"room:*"` — matches any topic starting with `"room:"`
///
/// The first parameter must be `RelayEvent`. Remaining parameters are
/// extracted via `FromRequestParts` with synthetic request parts (same
/// extractors as HTTP handlers, minus body extractors).
///
/// # Example
///
/// ```ignore
/// use rapina::prelude::*;
/// use rapina::relay::{Relay, RelayEvent};
///
/// #[relay("room:*")]
/// async fn room(event: RelayEvent, relay: Relay) -> Result<()> {
///     match &event {
///         RelayEvent::Join { topic, conn_id } => {
///             relay.track(topic, *conn_id, serde_json::json!({}));
///         }
///         RelayEvent::Message { topic, event: ev, payload, .. } => {
///             relay.push(topic, ev, payload).await?;
///         }
///         RelayEvent::Leave { topic, conn_id } => {
///             relay.untrack(topic, *conn_id);
///         }
///     }
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn relay(attr: TokenStream, item: TokenStream) -> TokenStream {
    relay::relay_macro_impl(attr.into(), item.into()).into()
}

/// Marks a static Prometheus collector for auto-discovery.
///
/// Annotate a module-level `static` holding a collector and `.discover()`
/// registers it with the `/metrics` endpoint, so you don't have to thread it
/// through `add_metric()`. Requires the `metrics` feature plus both
/// `.enable_metrics()` and `.discover()` on the app builder.
///
/// The collector type must be `Clone` (all built-in prometheus types are;
/// clones share the same underlying values). Wrap the collector in
/// `std::sync::LazyLock` or `once_cell::sync::Lazy`; no built-in prometheus
/// type can be constructed in a const context, so a bare static won't
/// compile. `OnceLock`-style cells are not supported, and the static must
/// live at module scope, not inside a function body.
///
/// This is the only Rapina attribute applied to a `static` rather than a
/// function.
///
/// # Example
///
/// ```ignore
/// use std::sync::LazyLock;
/// use rapina::metric;
/// use rapina::prometheus::IntCounter;
///
/// #[metric]
/// static ORDERS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
///     IntCounter::new("orders_total", "Total orders placed").unwrap()
/// });
/// ```
#[proc_macro_attribute]
pub fn metric(attr: TokenStream, item: TokenStream) -> TokenStream {
    metric_macro_impl(attr.into(), item.into()).into()
}

fn metric_macro_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if !attr.is_empty() {
        return syn::Error::new_spanned(attr, "#[metric] does not take arguments")
            .to_compile_error();
    }
    let item = match syn::parse2::<syn::ItemStatic>(item) {
        Ok(item) => item,
        Err(err) => {
            return syn::Error::new(
                err.span(),
                "#[metric] can only be applied to a `static` item",
            )
            .to_compile_error();
        }
    };
    if let syn::StaticMutability::Mut(m) = &item.mutability {
        return syn::Error::new_spanned(m, "#[metric] cannot be applied to a `static mut`")
            .to_compile_error();
    }

    let ident = &item.ident;
    let collector_fn = quote::format_ident!("__rapina_metric_{}", ident);

    quote! {
        #item

        #[doc(hidden)]
        #[allow(non_snake_case)]
        fn #collector_fn() -> Box<dyn rapina::prometheus::core::Collector> {
            Box::new(#ident.clone())
        }

        rapina::inventory::submit! {
            rapina::discovery::MetricDescriptor {
                collector: #collector_fn,
            }
        }
    }
}

/// Defines a background job handler.
///
/// Annotate an `async fn` to register it as a background job. The first
/// argument is always the payload type (must implement `Serialize +
/// DeserializeOwned`). Remaining arguments are dependency-injected from
/// `AppState` — `State<T>` and `Db` are the supported extractors.
///
/// Optionally configure the queue and retry limit:
///
/// ```text
/// #[job(queue = "emails", max_retries = 5)]
/// ```
///
/// Defaults: `queue = "default"`, `max_retries = 3`.
///
/// # What the macro generates
///
/// Given:
///
/// ```rust,ignore
/// #[job(queue = "emails")]
/// async fn send_welcome_email(
///     payload: WelcomeEmailPayload,
///     mailer: State<Mailer>,
/// ) -> JobResult { ... }
/// ```
///
/// The macro generates a helper function with the same name and visibility:
///
/// ```rust,ignore
/// fn send_welcome_email(payload: WelcomeEmailPayload) -> JobRequest {
///     JobRequest { job_type: "send_welcome_email", queue: "emails", ... }
/// }
/// ```
///
/// The `Jobs` extractor and `enqueue()` API for dispatching jobs from handlers
/// are planned for a follow-up release.
///
/// The handler is also registered via `inventory` for runtime dispatch —
/// no manual registration needed.
///
/// # Feature requirement
///
/// Requires the `database` feature. The generated types (`JobRequest`,
/// `JobDescriptor`) live in `rapina::jobs`, which is gated behind that feature.
///
/// # DI limitations
///
/// Only `State<T>` and `Db` work in job handlers. Request-bound extractors
/// (`Context`, `Headers`, `Path`, `CurrentUser`) will fail at runtime.
#[proc_macro_attribute]
pub fn job(attr: TokenStream, item: TokenStream) -> TokenStream {
    job::job_macro_impl(attr.into(), item.into()).into()
}

/// Derive macro for type-safe configuration
///
/// Generates a `from_env()` method that loads configuration from environment variables.
#[proc_macro_derive(Config, attributes(env, default))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    config::derive_config_impl(input.into()).into()
}

/// Define database entities with Prisma-like syntax.
///
/// This macro generates SeaORM entity definitions from a declarative syntax
/// where types indicate relationships. Each entity automatically gets `id`,
/// `created_at`, and `updated_at` fields.
///
/// # Syntax
///
/// ```ignore
/// rapina::schema! {
///     User {
///         email: String,
///         name: String,
///         posts: Vec<Post>,        // has_many relationship
///     }
///
///     Post {
///         title: String,
///         content: Text,           // TEXT column type
///         author: User,            // belongs_to -> generates author_id
///         comments: Vec<Comment>,
///     }
///
///     Comment {
///         content: Text,
///         post: Post,
///         author: Option<User>,    // optional belongs_to
///     }
/// }
/// ```
///
/// # Generated Code
///
/// For each entity, the macro generates a SeaORM module with:
/// - `Model` struct with auto `id`, `created_at`, `updated_at`
/// - `Relation` enum with proper SeaORM attributes
/// - `Related<T>` trait implementations
/// - `ActiveModelBehavior` implementation
///
/// # Supported Types
///
/// | Schema Type | Rust Type | Notes |
/// |-------------|-----------|-------|
/// | `String` | `String` | Default varchar |
/// | `Text` | `String` | TEXT column |
/// | `i32` | `i32` | |
/// | `i64` | `i64` | |
/// | `f32` | `f32` | |
/// | `f64` | `f64` | |
/// | `bool` | `bool` | |
/// | `Uuid` | `Uuid` | |
/// | `DateTime` | `DateTimeUtc` | |
/// | `Date` | `Date` | |
/// | `Decimal` | `Decimal` | |
/// | `Json` | `Json` | |
/// | `Option<T>` | `Option<T>` | Nullable |
/// | `Vec<Entity>` | - | has_many relationship |
/// | `Entity` | - | belongs_to (generates FK) |
#[proc_macro]
pub fn schema(input: TokenStream) -> TokenStream {
    schema::schema_impl(input.into()).into()
}

#[cfg(test)]
mod tests {
    use super::metric_macro_impl;
    use super::{AuthorizeArgs, job_macro_impl, join_paths, metric_macro_impl, route_macro_core};
    use quote::quote;

    #[test]
    fn test_metric_macro_generates_collector_fn_and_inventory() {
        let input = quote! {
            static ORDERS_TOTAL: LazyLock<IntCounter> = LazyLock::new(|| {
                IntCounter::new("orders_total", "Total orders placed").unwrap()
            });
        };

        let output = metric_macro_impl(quote!(), input);
        let output_str = output.to_string();

        assert!(output_str.contains("static ORDERS_TOTAL"));
        assert!(output_str.contains("__rapina_metric_ORDERS_TOTAL"));
        assert!(output_str.contains("inventory :: submit !"));
        assert!(output_str.contains("MetricDescriptor"));
    }

    #[test]
    fn test_metric_macro_rejects_args() {
        let input = quote! {
            static ORDERS_TOTAL: LazyLock<IntCounter> = LazyLock::new(make_counter);
        };

        let output_str = metric_macro_impl(quote!(name = "orders"), input).to_string();

        assert!(output_str.contains("compile_error !"));
        assert!(output_str.contains("does not take arguments"));
    }

    #[test]
    fn test_metric_macro_rejects_fn() {
        let input = quote! {
            fn not_a_static() {}
        };

        let output_str = metric_macro_impl(quote!(), input).to_string();

        assert!(output_str.contains("compile_error !"));
        assert!(output_str.contains("can only be applied to a `static` item"));
    }

    #[test]
    fn test_metric_macro_rejects_static_mut() {
        let input = quote! {
            static mut ORDERS_TOTAL: IntCounter = make_counter();
        };

        let output_str = metric_macro_impl(quote!(), input).to_string();

        assert!(output_str.contains("compile_error !"));
        assert!(output_str.contains("cannot be applied to a `static mut`"));
    }
}
