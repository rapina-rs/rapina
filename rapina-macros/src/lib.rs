use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{FnArg, ItemFn};

mod relay;
mod route;
mod schema;

use route::route_macro;

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
    let func: ItemFn = syn::parse(item.clone()).expect("#[public] must be applied to a function");
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
    .into()
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
    job_macro_impl(attr.into(), item.into()).into()
}

struct JobAttr {
    queue: String,
    max_retries: i32,
    retry_policy: String,
    retry_delay_secs: f64,
}

impl Default for JobAttr {
    fn default() -> Self {
        Self {
            queue: "default".to_string(),
            max_retries: 3,
            retry_policy: "exponential".to_string(),
            retry_delay_secs: 1.0,
        }
    }
}

impl syn::parse::Parse for JobAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut attr = JobAttr::default();

        while !input.is_empty() {
            let ident: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;

            if ident == "queue" {
                let lit: syn::LitStr = input.parse()?;
                let q = lit.value();
                if q.is_empty() {
                    return Err(syn::Error::new(lit.span(), "queue name must not be empty"));
                }
                attr.queue = q;
            } else if ident == "max_retries" {
                let lit: syn::LitInt = input.parse()?;
                let val: i32 = lit.base10_parse()?;
                if val < 0 {
                    return Err(syn::Error::new(lit.span(), "max_retries must be >= 0"));
                }
                attr.max_retries = val;
            } else if ident == "retry_policy" {
                let lit: syn::LitStr = input.parse()?;
                let val = lit.value();
                if !matches!(val.as_str(), "exponential" | "fixed" | "none") {
                    return Err(syn::Error::new(
                        lit.span(),
                        "retry_policy must be \"exponential\", \"fixed\", or \"none\"",
                    ));
                }
                attr.retry_policy = val;
            } else if ident == "retry_delay_secs" {
                let val: f64 = if input.peek(syn::LitFloat) {
                    let lit: syn::LitFloat = input.parse()?;
                    lit.base10_parse()?
                } else {
                    let lit: syn::LitInt = input.parse()?;
                    let v: u64 = lit.base10_parse()?;
                    v as f64
                };
                if val < 0.0 {
                    return Err(syn::Error::new(
                        proc_macro2::Span::call_site(),
                        "retry_delay_secs must be >= 0",
                    ));
                }
                attr.retry_delay_secs = val;
            } else if ident == "timeout" {
                // Consume the value so the error points at the attribute name, not EOF.
                let _: syn::LitStr = input.parse()?;
                return Err(syn::Error::new(
                    ident.span(),
                    "#[job(timeout = ...)] is not yet supported — coming in a future release",
                ));
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!(
                        "unknown #[job] attribute `{ident}` — supported: `queue`, `max_retries`, `retry_policy`, `retry_delay_secs`"
                    ),
                ));
            }

            if input.peek(syn::Token![,]) {
                input.parse::<syn::Token![,]>()?;
            }
        }

        Ok(attr)
    }
}

fn job_macro_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let job_attr: JobAttr = match syn::parse2(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error(),
    };

    let func: ItemFn = match syn::parse2(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };

    // Must be async — the handle wrapper calls the impl with .await.
    if func.sig.asyncness.is_none() {
        return syn::Error::new(
            func.sig.fn_token.span,
            "#[job] must be applied to an async function",
        )
        .to_compile_error();
    }

    // Generic parameters can't be monomorphized into a fn pointer for inventory.
    if !func.sig.generics.params.is_empty() {
        return syn::Error::new(
            func.sig.generics.params.first().unwrap().span(),
            "#[job] does not support generic type parameters — the payload type must be concrete",
        )
        .to_compile_error();
    }

    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();
    let func_vis = &func.vis;

    let impl_fn_name = syn::Ident::new(
        &format!("__rapina_job_impl_{}", func_name_str),
        proc_macro2::Span::call_site(),
    );
    let handle_fn_name = syn::Ident::new(
        &format!("__rapina_job_handle_{}", func_name_str),
        proc_macro2::Span::call_site(),
    );

    let queue_str = &job_attr.queue;
    let max_retries = job_attr.max_retries;
    let retry_policy_str = &job_attr.retry_policy;
    let retry_delay_secs = job_attr.retry_delay_secs;

    let args: Vec<_> = func.sig.inputs.iter().collect();

    if args.is_empty() {
        return syn::Error::new(
            func.sig.ident.span(),
            "#[job] requires at least one argument (the payload type)",
        )
        .to_compile_error();
    }

    // First arg is the payload — extract its type for the helper signature and
    // for the serde_json::from_value call in the handle wrapper.
    let payload_type = match &args[0] {
        FnArg::Typed(pat_type) => &pat_type.ty,
        FnArg::Receiver(r) => {
            return syn::Error::new(
                r.self_token.span,
                "#[job] cannot be applied to a method — use a free function",
            )
            .to_compile_error();
        }
    };

    // Remaining args are DI extractors (State<T>, Db, etc.).
    let mut extractor_extractions = Vec::new();
    let mut di_call_args = Vec::new();

    for (i, arg) in args[1..].iter().enumerate() {
        if let FnArg::Typed(pat_type) = arg {
            let arg_type = &pat_type.ty;
            let tmp = syn::Ident::new(
                &format!("__rapina_di_{}", i),
                proc_macro2::Span::call_site(),
            );
            extractor_extractions.push(quote! {
                let #tmp = <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(
                    &__rapina_parts, &__rapina_params, &__rapina_state
                ).await?;
            });
            di_call_args.push(quote! { #tmp });
        }
    }

    let impl_inputs = &func.sig.inputs;
    let impl_output = &func.sig.output;
    let func_block = &func.block;
    let func_attrs = &func.attrs;

    quote! {
        // Original handler body, renamed to an internal function. Only called
        // by the handle wrapper below — never exposed directly.
        #(#func_attrs)*
        #[doc(hidden)]
        async fn #impl_fn_name(#impl_inputs) #impl_output
        #func_block

        // DI wrapper registered in inventory. Deserializes the JSON payload,
        // creates synthetic request parts for extractor compatibility, injects
        // dependencies from AppState, then calls the impl function.
        //
        // Only State<T> and Db work here — they source data from AppState
        // directly and ignore the synthetic parts.
        #[doc(hidden)]
        fn #handle_fn_name(
            __rapina_payload_raw: rapina::serde_json::Value,
            __rapina_state: std::sync::Arc<rapina::state::AppState>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = rapina::jobs::JobResult> + Send>>
        {
            Box::pin(async move {
                let __rapina_payload_typed: #payload_type =
                    match rapina::serde_json::from_value(__rapina_payload_raw) {
                        Ok(v) => v,
                        Err(e) => {
                            return Err(rapina::error::Error::internal(format!(
                                "failed to deserialize job payload for '{}': {e}",
                                #func_name_str
                            )));
                        }
                    };
                let (__rapina_parts, _) = rapina::http::Request::new(()).into_parts();
                let __rapina_params = rapina::extract::PathParams::new();
                #(#extractor_extractions)*
                #impl_fn_name(__rapina_payload_typed, #(#di_call_args),*).await
            })
        }

        // Helper function with the same name and visibility as the original.
        // Call this to build a JobRequest for jobs.enqueue().
        #func_vis fn #func_name(payload: #payload_type) -> rapina::jobs::JobRequest {
            rapina::jobs::JobRequest {
                job_type: #func_name_str,
                payload: rapina::serde_json::to_value(payload).expect(
                    "job payload serialization failed — ensure all fields are JSON-compatible",
                ),
                queue: #queue_str,
                max_retries: #max_retries,
            }
        }

        rapina::inventory::submit! {
            rapina::jobs::JobDescriptor {
                job_type: #func_name_str,
                handle: #handle_fn_name,
                retry_policy: #retry_policy_str,
                retry_delay_secs: #retry_delay_secs,
            }
        }
    }
}

/// Derive macro for type-safe configuration
///
/// Generates a `from_env()` method that loads configuration from environment variables.
#[proc_macro_derive(Config, attributes(env, default))]
pub fn derive_config(input: TokenStream) -> TokenStream {
    derive_config_impl(input.into()).into()
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

fn derive_config_impl(input: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    let input: syn::DeriveInput = syn::parse2(input).expect("expected struct");
    let name = &input.ident;

    let fields = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            syn::Fields::Named(fields) => &fields.named,
            _ => panic!("Config derive only supports structs with named fields"),
        },
        _ => panic!("Config derive only supports structs"),
    };

    let mut field_inits = Vec::new();
    let mut missing_checks = Vec::new();

    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        let field_type = &field.ty;

        // Find #[env = "VAR_NAME"] attribute
        let env_var = field
            .attrs
            .iter()
            .find_map(|attr| {
                if attr.path().is_ident("env")
                    && let syn::Meta::NameValue(nv) = &attr.meta
                    && let syn::Expr::Lit(expr_lit) = &nv.value
                    && let syn::Lit::Str(lit_str) = &expr_lit.lit
                {
                    return Some(lit_str.value());
                }
                None
            })
            .unwrap_or_else(|| field_name.to_string().to_uppercase());

        // Find #[default = "value"] attribute
        let default_value = field.attrs.iter().find_map(|attr| {
            if attr.path().is_ident("default")
                && let syn::Meta::NameValue(nv) = &attr.meta
                && let syn::Expr::Lit(expr_lit) = &nv.value
                && let syn::Lit::Str(lit_str) = &expr_lit.lit
            {
                return Some(lit_str.value());
            }
            None
        });

        let env_var_lit = syn::LitStr::new(&env_var, proc_macro2::Span::call_site());

        if let Some(default) = default_value {
            let default_lit = syn::LitStr::new(&default, proc_macro2::Span::call_site());
            field_inits.push(quote! {
                #field_name: rapina::config::get_env_or(#env_var_lit, #default_lit).parse().unwrap_or_else(|_| #default_lit.parse().unwrap())
            });
        } else {
            field_inits.push(quote! {
                #field_name: rapina::config::get_env_parsed::<#field_type>(#env_var_lit)?
            });
            missing_checks.push(quote! {
                if std::env::var(#env_var_lit).is_err() {
                    missing.push(#env_var_lit);
                }
            });
        }
    }

    quote! {
        impl #name {
            pub fn from_env() -> std::result::Result<Self, rapina::config::ConfigError> {
                let mut missing: Vec<&str> = Vec::new();
                #(#missing_checks)*

                if !missing.is_empty() {
                    return Err(rapina::config::ConfigError::MissingMultiple(
                        missing.into_iter().map(String::from).collect()
                    ));
                }

                Ok(Self {
                    #(#field_inits),*
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{job_macro_impl, metric_macro_impl};
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

    // -- #[job] retry policy attributes --

    fn minimal_job_fn() -> proc_macro2::TokenStream {
        quote! {
            async fn my_job(payload: String) {}
        }
    }

    #[test]
    fn job_macro_defaults_retry_policy_and_delay() {
        let output = job_macro_impl(quote! {}, minimal_job_fn()).to_string();
        assert!(
            output.contains("retry_policy : \"exponential\""),
            "default retry_policy should be exponential"
        );
        assert!(
            output.contains("retry_delay_secs : 1f64"),
            "default retry_delay_secs should be 1.0"
        );
    }

    #[test]
    fn job_macro_fixed_retry_policy() {
        let output =
            job_macro_impl(quote! { retry_policy = "fixed" }, minimal_job_fn()).to_string();
        assert!(output.contains("retry_policy : \"fixed\""));
    }

    #[test]
    fn job_macro_none_retry_policy() {
        let output = job_macro_impl(quote! { retry_policy = "none" }, minimal_job_fn()).to_string();
        assert!(output.contains("retry_policy : \"none\""));
    }

    #[test]
    fn job_macro_retry_delay_float_literal() {
        let output =
            job_macro_impl(quote! { retry_delay_secs = 30.0 }, minimal_job_fn()).to_string();
        assert!(output.contains("retry_delay_secs : 30f64"));
    }

    #[test]
    fn job_macro_retry_delay_integer_literal() {
        let output = job_macro_impl(quote! { retry_delay_secs = 30 }, minimal_job_fn()).to_string();
        assert!(output.contains("retry_delay_secs : 30f64"));
    }

    #[test]
    fn job_macro_invalid_retry_policy_is_compile_error() {
        let output =
            job_macro_impl(quote! { retry_policy = "random" }, minimal_job_fn()).to_string();
        assert!(output.contains("compile_error"));
        assert!(
            output.contains("exponential") || output.contains("fixed") || output.contains("none")
        );
    }

    #[test]
    fn job_macro_unknown_attr_error_mentions_retry_attrs() {
        let output = job_macro_impl(quote! { retries = 3 }, minimal_job_fn()).to_string();
        assert!(output.contains("compile_error"));
        assert!(output.contains("retry_policy"));
        assert!(output.contains("retry_delay_secs"));
    }

    #[test]
    fn job_macro_all_retry_attrs_combined() {
        let output = job_macro_impl(
            quote! { retry_policy = "fixed", retry_delay_secs = 15, max_retries = 5 },
            minimal_job_fn(),
        )
        .to_string();
        assert!(output.contains("retry_policy : \"fixed\""));
        assert!(output.contains("retry_delay_secs : 15f64"));
    }
}
