use proc_macro::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{FnArg, ItemFn, LitStr, Pat};

/// Parsed route macro attribute: `"/path"` or `"/path", group = "/prefix"`.
struct RouteAttr {
    path: LitStr,
    group: Option<LitStr>,
}

impl syn::parse::Parse for RouteAttr {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let path: LitStr = input.parse()?;
        let group = if input.peek(syn::Token![,]) {
            input.parse::<syn::Token![,]>()?;
            let ident: syn::Ident = input.parse()?;
            if ident != "group" {
                return Err(syn::Error::new(ident.span(), "expected `group`"));
            }
            input.parse::<syn::Token![=]>()?;
            let value: LitStr = input.parse()?;
            Some(value)
        } else {
            None
        };
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after route attribute"));
        }
        Ok(RouteAttr { path, group })
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

mod schema;

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

fn route_macro_core(
    method: &str,
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let route_attr: RouteAttr = syn::parse2(attr).expect("expected path as string literal");
    let path_str = if let Some(ref group) = route_attr.group {
        let g = group.value();
        assert!(
            g.starts_with('/'),
            "group prefix must start with `/`, got: {g:?}"
        );
        join_paths(&g, &route_attr.path.value())
    } else {
        route_attr.path.value()
    };
    let mut func: ItemFn = syn::parse2(item).expect("expected function");

    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();
    let func_vis = &func.vis;

    // Extract #[public] attribute if present (when #[public] is below the route macro)
    let is_public = extract_public_attr(&mut func.attrs);

    // Extract #[errors(ErrorType)] attribute if present
    let error_type = extract_errors_attr(&mut func.attrs);

    // Extract #[cache(ttl = N)] attribute if present
    let cache_ttl = extract_cache_attr(&mut func.attrs);

    let error_responses_impl = if let Some(err_type) = &error_type {
        quote! {
            fn error_responses() -> Vec<rapina::error::ErrorVariant> {
                <#err_type as rapina::error::DocumentedError>::error_variants()
            }
        }
    } else {
        quote! {}
    };

    // Extract return type for schema generation
    let response_schema_impl = if let syn::ReturnType::Type(_, return_type) = &func.sig.output {
        if let Some(inner_type) = extract_json_inner_type(return_type) {
            quote! {
                fn response_schema() -> Option<serde_json::Value> {
                    Some(rapina::openapi_schema_for::<#inner_type>())
                }
            }
        } else {
            quote! {}
        }
    } else {
        quote! {}
    };

    // Extract request body type and content type for schema generation.
    // Only generate requestBody for POST, PUT, and PATCH methods per OpenAPI spec.
    let (request_schema_impl, request_content_type_impl, request_body_required_impl) =
        if matches!(method, "POST" | "PUT" | "PATCH") {
            if let Some(meta) = extract_request_body_meta(&func.sig.inputs) {
                let inner_type = meta.inner_type;
                let content_type = meta.content_type;
                let required = meta.required;
                (
                    quote! {
                        fn request_schema() -> Option<serde_json::Value> {
                            Some(rapina::openapi_schema_for::<#inner_type>())
                        }
                    },
                    quote! {
                        fn request_content_type() -> Option<&'static str> {
                            Some(#content_type)
                        }
                    },
                    quote! {
                        fn request_body_required() -> Option<bool> {
                            Some(#required)
                        }
                    },
                )
            } else {
                (quote! {}, quote! {}, quote! {})
            }
        } else {
            (quote! {}, quote! {}, quote! {})
        };

    let args: Vec<_> = func.sig.inputs.iter().collect();

    // Extract return type for type annotation (helps with type inference in async blocks)
    let return_type_annotation = match &func.sig.output {
        syn::ReturnType::Type(_, ty) => quote! { : #ty },
        syn::ReturnType::Default => quote! {},
    };

    // Optional cache TTL header injection
    let cache_header_injection = if let Some(ttl) = cache_ttl {
        let ttl_str = ttl.to_string();
        quote! {
            let mut __rapina_response = __rapina_response;
            __rapina_response.headers_mut().insert(
                "x-rapina-cache-ttl",
                rapina::http::HeaderValue::from_static(#ttl_str),
            );
        }
    } else {
        quote! {}
    };

    // Build the handler body
    // Use __rapina_ prefix for internal variables to avoid shadowing user's variables
    let handler_body = if args.is_empty() {
        let inner_block = &func.block;
        quote! {
            let __rapina_result #return_type_annotation = (async #inner_block).await;
            let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
            #cache_header_injection
            __rapina_response
        }
    } else {
        let inner_block = &func.block;

        if args.len() == 1 {
            // Single arg: pass request directly to FromRequest
            let arg = &args[0];
            if let FnArg::Typed(pat_type) = arg {
                let pat = &pat_type.pat;
                let arg_type = &pat_type.ty;
                let tmp = syn::Ident::new("__rapina_arg_0", proc_macro2::Span::call_site());
                quote! {
                    let #tmp = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                        Ok(v) => v,
                        Err(e) => return rapina::response::IntoResponse::into_response(e),
                    };
                    let #pat = #tmp;
                    let __rapina_result #return_type_annotation = (async #inner_block).await;
                    let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
                    #cache_header_injection
                    __rapina_response
                }
            } else {
                unreachable!("handler argument must be a typed pattern")
            }
        } else {
            // Multiple args: all but last use FromRequestParts, last uses FromRequest
            let mut parts_extractions = Vec::new();

            for (i, arg) in args[..args.len() - 1].iter().enumerate() {
                if let FnArg::Typed(pat_type) = arg {
                    let pat = &pat_type.pat;
                    let arg_type = &pat_type.ty;
                    let tmp = syn::Ident::new(
                        &format!("__rapina_arg_{}", i),
                        proc_macro2::Span::call_site(),
                    );
                    parts_extractions.push(quote! {
                        let #tmp = match <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(&__rapina_parts, &__rapina_params, &__rapina_state).await {
                            Ok(v) => v,
                            Err(e) => return rapina::response::IntoResponse::into_response(e),
                        };
                        let #pat = #tmp;
                    });
                }
            }

            let last_arg = args.last().unwrap();
            let last_extraction = if let FnArg::Typed(pat_type) = last_arg {
                let pat = &pat_type.pat;
                let arg_type = &pat_type.ty;
                let tmp = syn::Ident::new(
                    &format!("__rapina_arg_{}", args.len() - 1),
                    proc_macro2::Span::call_site(),
                );
                quote! {
                    let __rapina_req = rapina::http::Request::from_parts(__rapina_parts, __rapina_body);
                    let #tmp = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                        Ok(v) => v,
                        Err(e) => return rapina::response::IntoResponse::into_response(e),
                    };
                    let #pat = #tmp;
                }
            } else {
                unreachable!("handler argument must be a typed pattern")
            };

            quote! {
                let (__rapina_parts, __rapina_body) = __rapina_req.into_parts();
                #(#parts_extractions)*
                #last_extraction
                let __rapina_result #return_type_annotation = (async #inner_block).await;
                let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
                #cache_header_injection
                __rapina_response
            }
        }
    };

    // Build the router method call for the register function
    let router_method = syn::Ident::new(&method.to_lowercase(), proc_macro2::Span::call_site());
    let register_fn_name = syn::Ident::new(
        &format!("__rapina_register_{}", func_name_str),
        proc_macro2::Span::call_site(),
    );

    // Generate the struct, Handler impl, and inventory submission
    quote! {
        #[derive(Clone, Copy)]
        #[allow(non_camel_case_types)]
        #func_vis struct #func_name;

        impl rapina::handler::Handler for #func_name {
            const NAME: &'static str = #func_name_str;

            #response_schema_impl
            #request_schema_impl
            #request_content_type_impl
            #request_body_required_impl
            #error_responses_impl

            fn call(
                &self,
                __rapina_req: rapina::hyper::Request<rapina::hyper::body::Incoming>,
                __rapina_params: rapina::extract::PathParams,
                __rapina_state: std::sync::Arc<rapina::state::AppState>,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = rapina::hyper::Response<rapina::response::BoxBody>> + Send>> {
                Box::pin(async move {
                    #handler_body
                })
            }
        }

        #[doc(hidden)]
        fn #register_fn_name(__rapina_router: rapina::router::Router) -> rapina::router::Router {
            __rapina_router.#router_method(#path_str, #func_name)
        }

        rapina::inventory::submit! {
            rapina::discovery::RouteDescriptor {
                method: #method,
                path: #path_str,
                handler_name: #func_name_str,
                is_public: #is_public,
                response_schema: <#func_name as rapina::handler::Handler>::response_schema,
                request_schema: <#func_name as rapina::handler::Handler>::request_schema,
                request_content_type: <#func_name as rapina::handler::Handler>::request_content_type,
                request_body_required: <#func_name as rapina::handler::Handler>::request_body_required,
                error_responses: <#func_name as rapina::handler::Handler>::error_responses,
                register: #register_fn_name,
            }
        }
    }
}

/// Extracts the inner type from Json<T> wrapper for schema generation
fn extract_json_inner_type(return_type: &syn::Type) -> Option<proc_macro2::TokenStream> {
    if let syn::Type::Path(type_path) = return_type
        && let Some(last_segment) = type_path.path.segments.last()
    {
        // Direct Json<T>
        if last_segment.ident == "Json"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
        {
            return Some(quote!(#inner_type));
        }

        // Result<Json<T>> or Result<Json<T>, E>
        if last_segment.ident == "Result"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(ok_type)) = args.args.first()
        {
            return extract_json_inner_type(ok_type);
        }
    }
    None
}

/// Extracts the request body metadata from handler function arguments.
/// Supports Json<T>, Form<T>, Validated<Json<T>>, and Validated<Form<T>>.
fn extract_request_body_meta(
    inputs: &syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> Option<RequestBodyMeta> {
    for arg in inputs.iter() {
        if let syn::FnArg::Typed(pat_type) = arg {
            if let Some(meta) = extract_body_inner_type(&pat_type.ty) {
                return Some(meta);
            }
        }
    }
    None
}

/// Information about a request body extractor.
struct RequestBodyMeta {
    inner_type: proc_macro2::TokenStream,
    content_type: &'static str,
    required: bool,
}

/// Extracts the inner type and content type from Json<T>, Form<T>, Validated<Json<T>>/Validated<Form<T>>,
/// or Option<Json<T>>/Option<Form<T>>.
fn extract_body_inner_type(ty: &syn::Type) -> Option<RequestBodyMeta> {
    if let syn::Type::Path(type_path) = ty
        && let Some(last_segment) = type_path.path.segments.last()
    {
        // Direct Json<T>
        if last_segment.ident == "Json"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
        {
            return Some(RequestBodyMeta {
                inner_type: quote!(#inner_type),
                content_type: "application/json",
                required: true,
            });
        }
        // Direct Form<T>
        if last_segment.ident == "Form"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
        {
            return Some(RequestBodyMeta {
                inner_type: quote!(#inner_type),
                content_type: "application/x-www-form-urlencoded",
                required: true,
            });
        }
        // Validated<Json<T>> or Validated<Form<T>>
        if last_segment.ident == "Validated"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(inner_extractor)) = args.args.first()
        {
            return extract_body_inner_type(inner_extractor);
        }
        // Option<Json<T>> or Option<Form<T>> - optional request body
        if last_segment.ident == "Option"
            && let syn::PathArguments::AngleBracketed(args) = &last_segment.arguments
            && let Some(syn::GenericArgument::Type(inner_extractor)) = args.args.first()
        {
            if let Some(mut meta) = extract_body_inner_type(inner_extractor) {
                meta.required = false;
                return Some(meta);
            }
        }
    }
    None
}

/// Extract #[errors(ErrorType)] attribute from function attributes, removing it if found.
fn extract_errors_attr(attrs: &mut Vec<syn::Attribute>) -> Option<syn::Type> {
    let idx = attrs
        .iter()
        .position(|attr| attr.path().is_ident("errors"))?;
    let attr = attrs.remove(idx);
    let err_type: syn::Type = attr.parse_args().expect("expected #[errors(ErrorType)]");
    Some(err_type)
}

/// Extract #[cache(ttl = N)] attribute from function attributes, removing it if found.
fn extract_cache_attr(attrs: &mut Vec<syn::Attribute>) -> Option<u64> {
    let idx = attrs
        .iter()
        .position(|attr| attr.path().is_ident("cache"))?;
    let attr = attrs.remove(idx);

    let mut ttl: Option<u64> = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("ttl") {
            let value = meta.value()?;
            let lit: syn::LitInt = value.parse()?;
            ttl = Some(lit.base10_parse()?);
            Ok(())
        } else {
            Err(meta.error("expected `ttl`"))
        }
    })
    .expect("expected #[cache(ttl = N)]");

    ttl
}

/// Extract #[public] attribute from function attributes, removing it if found.
fn extract_public_attr(attrs: &mut Vec<syn::Attribute>) -> bool {
    if let Some(idx) = attrs.iter().position(|attr| attr.path().is_ident("public")) {
        attrs.remove(idx);
        true
    } else {
        false
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
    relay_macro_impl(attr.into(), item.into()).into()
}

fn relay_macro_impl(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let pattern: LitStr = syn::parse2(attr).expect("expected pattern as string literal");
    let pattern_str = pattern.value();
    let func: ItemFn = syn::parse2(item).expect("#[relay] must be applied to an async function");

    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();

    let is_prefix = pattern_str.ends_with('*');
    let match_prefix_str = if is_prefix {
        &pattern_str[..pattern_str.len() - 1]
    } else {
        &pattern_str
    };

    let wrapper_name = syn::Ident::new(
        &format!("__rapina_channel_{}", func_name_str),
        proc_macro2::Span::call_site(),
    );

    // First arg is RelayEvent (passed directly). Remaining args are extractors.
    let args: Vec<_> = func.sig.inputs.iter().collect();

    let mut extractor_extractions = Vec::new();
    let mut call_args = vec![quote! { __rapina_event }];

    for (i, arg) in args.iter().enumerate() {
        if i == 0 {
            // First arg is RelayEvent — passed directly, not extracted
            continue;
        }
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;

                extractor_extractions.push(quote! {
                    let #arg_name = <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(
                        &__rapina_parts, &__rapina_params, &__rapina_state
                    ).await?;
                });

                call_args.push(quote! { #arg_name });
            }
        }
    }

    quote! {
        #func

        // Generated by #[relay] — not user-facing API
        #[doc(hidden)]
        fn #wrapper_name(
            __rapina_event: rapina::relay::RelayEvent,
            __rapina_state: std::sync::Arc<rapina::state::AppState>,
            __rapina_current_user: Option<rapina::auth::CurrentUser>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = std::result::Result<(), rapina::error::Error>> + Send>> {
            Box::pin(async move {
                let (mut __rapina_parts, _) = rapina::http::Request::new(()).into_parts();
                if let Some(u) = __rapina_current_user {
                    __rapina_parts.extensions.insert(u);
                }
                let __rapina_params = rapina::extract::PathParams::new();
                #(#extractor_extractions)*
                #func_name(#(#call_args),*).await
            })
        }

        rapina::inventory::submit! {
            rapina::relay::ChannelDescriptor {
                pattern: #pattern_str,
                is_prefix: #is_prefix,
                match_prefix: #match_prefix_str,
                handler_name: #func_name_str,
                handle: #wrapper_name,
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
}

impl Default for JobAttr {
    fn default() -> Self {
        Self {
            queue: "default".to_string(),
            max_retries: 3,
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
                        "unknown #[job] attribute `{ident}` — supported: `queue`, `max_retries`"
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
            }
        }
    }
}

fn route_macro(method: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro_core(method, attr.into(), item.into()).into()
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
    use super::{join_paths, relay_macro_impl, route_macro_core};
    use quote::quote;

    #[test]
    fn test_generates_struct_with_handler_impl() {
        let path = quote!("/");
        let input = quote! {
            async fn hello() -> &'static str {
                "Hello, Rapina!"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Check struct is generated
        assert!(output_str.contains("struct hello"));
        // Check Handler impl is generated
        assert!(output_str.contains("impl rapina :: handler :: Handler for hello"));
        // Check NAME constant
        assert!(output_str.contains("const NAME"));
        assert!(output_str.contains("\"hello\""));
    }

    #[test]
    fn test_generates_handler_with_extractors() {
        let path = quote!("/users/:id");
        let input = quote! {
            async fn get_user(id: rapina::extract::Path<u64>) -> String {
                format!("{}", id.into_inner())
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("struct get_user"));
        // Single arg is last arg — uses FromRequest (blanket impl handles parts-only)
        assert!(output_str.contains("FromRequest"));
        // Single arg should NOT destructure request into parts
        assert!(!output_str.contains("into_parts"));
    }

    #[test]
    fn test_function_with_multiple_extractors() {
        let path = quote!("/users");
        let input = quote! {
            async fn create_user(
                id: rapina::extract::Path<u64>,
                body: rapina::extract::Json<String>
            ) -> String {
                "created".to_string()
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // Check struct is generated
        assert!(output_str.contains("struct create_user"));
        // Check both extractors are handled
        assert!(output_str.contains("FromRequestParts"));
        assert!(output_str.contains("FromRequest"));
    }

    #[test]
    fn test_two_body_extractors_no_macro_panic() {
        // With positional convention, the macro does NOT panic for multiple body consumers.
        // Instead, it generates code where the first Json is bounded by FromRequestParts
        // (which it doesn't implement), so the compiler catches it at type-check time.
        let path = quote!("/users");
        let input = quote! {
            async fn handler(
                body1: rapina::extract::Json<String>,
                body2: rapina::extract::Json<String>
            ) -> String {
                "ok".to_string()
            }
        };

        // Should NOT panic — macro expansion succeeds, compiler catches the error later
        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // First arg gets FromRequestParts (will fail at compile time since Json doesn't impl it)
        assert!(output_str.contains("FromRequestParts"));
        // Last arg gets FromRequest
        assert!(output_str.contains("FromRequest"));
    }

    #[test]
    fn test_custom_type_name_not_misclassified() {
        // UserPathInfo contains "Path" but should NOT be routed to FromRequestParts
        // Positional convention: single (last) arg always uses FromRequest
        let path = quote!("/users");
        let input = quote! {
            async fn handler(info: UserPathInfo) -> String {
                "ok".to_string()
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("FromRequest"));
        assert!(!output_str.contains("FromRequestParts"));
    }

    #[test]
    fn test_multiple_parts_only_extractors_positional() {
        // All parts-only extractors: first N-1 use FromRequestParts, last uses FromRequest
        let path = quote!("/users/:id");
        let input = quote! {
            async fn handler(
                id: rapina::extract::Path<u64>,
                query: rapina::extract::Query<Params>,
                headers: rapina::extract::Headers,
            ) -> String {
                "ok".to_string()
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // First two args use FromRequestParts
        assert!(output_str.contains("FromRequestParts"));
        // Last arg uses FromRequest (via blanket impl at runtime)
        assert!(output_str.contains("FromRequest"));
        // Request is destructured for multi-arg case
        assert!(output_str.contains("into_parts"));
        // Request is reassembled for last arg
        assert!(output_str.contains("from_parts"));
    }

    #[test]
    #[should_panic(expected = "expected function")]
    fn test_invalid_input_panics() {
        let path = quote!("/");
        let invalid_input = quote! { not_a_function };

        route_macro_core("GET", path, invalid_input);
    }

    #[test]
    fn test_json_return_type_generates_response_schema() {
        let path = quote!("/users");
        let input = quote! {
            async fn get_user() -> Json<UserResponse> {
                Json(UserResponse { id: 1 })
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Check response_schema method is generated with openapi_schema_for
        assert!(output_str.contains("fn response_schema"));
        assert!(output_str.contains("rapina :: openapi_schema_for"));
        assert!(output_str.contains("UserResponse"));
    }

    #[test]
    fn test_result_json_return_type_generates_response_schema() {
        let path = quote!("/users");
        let input = quote! {
            async fn get_user() -> Result<Json<UserResponse>> {
                Ok(Json(UserResponse { id: 1 }))
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("fn response_schema"));
        assert!(output_str.contains("rapina :: openapi_schema_for"));
        assert!(output_str.contains("UserResponse"));
    }

    #[test]
    fn test_errors_attr_generates_error_responses() {
        let path = quote!("/users");
        let input = quote! {
            #[errors(UserError)]
            async fn get_user() -> Result<Json<UserResponse>> {
                Ok(Json(UserResponse { id: 1 }))
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("fn error_responses"));
        assert!(output_str.contains("DocumentedError"));
        assert!(output_str.contains("UserError"));
    }

    #[test]
    fn test_json_body_generates_request_schema_and_content_type() {
        let path = quote!("/users");
        let input = quote! {
            async fn create_user(body: Json<CreateUserRequest>) -> Json<UserResponse> {
                Json(UserResponse { id: 1 })
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // Check request_schema method is generated
        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("CreateUserRequest"));
        // Check request_content_type method is generated with JSON content type
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/json"));
    }

    #[test]
    fn test_form_body_generates_request_schema_and_content_type() {
        let path = quote!("/users");
        let input = quote! {
            async fn create_user(body: Form<CreateUserForm>) -> Json<UserResponse> {
                Json(UserResponse { id: 1 })
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("CreateUserForm"));
        // Check request_content_type method is generated with form content type
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/x-www-form-urlencoded"));
    }

    #[test]
    fn test_validated_json_generates_request_schema_and_content_type() {
        let path = quote!("/users");
        let input = quote! {
            async fn create_user(body: Validated<Json<CreateUserRequest>>) -> Json<UserResponse> {
                Json(UserResponse { id: 1 })
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // Should extract CreateUserRequest from Validated<Json<CreateUserRequest>>
        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("CreateUserRequest"));
        // Should inherit JSON content type from inner Json extractor
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/json"));
    }

    #[test]
    fn test_validated_form_generates_request_schema_and_content_type() {
        let path = quote!("/login");
        let input = quote! {
            async fn login(body: Validated<Form<LoginForm>>) -> Json<TokenResponse> {
                Json(TokenResponse { token: "abc".into() })
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // Should extract LoginForm from Validated<Form<LoginForm>>
        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("LoginForm"));
        // Should inherit form content type from inner Form extractor
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/x-www-form-urlencoded"));
    }

    #[test]
    fn test_option_json_generates_optional_request_body() {
        let path = quote!("/users");
        let input = quote! {
            async fn update_user(body: Option<Json<UpdateUserRequest>>) -> Json<UserResponse> {
                Json(UserResponse { id: 1 })
            }
        };

        let output = route_macro_core("PATCH", path, input);
        let output_str = output.to_string();

        // Should extract UpdateUserRequest from Option<Json<UpdateUserRequest>>
        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("UpdateUserRequest"));
        // Should have JSON content type
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/json"));
        // Should have request_body_required returning false
        assert!(output_str.contains("fn request_body_required"));
        assert!(output_str.contains("Some (false)"));
    }

    #[test]
    fn test_option_form_generates_optional_request_body() {
        let path = quote!("/login");
        let input = quote! {
            async fn login(body: Option<Form<LoginForm>>) -> Json<TokenResponse> {
                Json(TokenResponse { token: "abc".into() })
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        // Should extract LoginForm from Option<Form<LoginForm>>
        assert!(output_str.contains("fn request_schema"));
        assert!(output_str.contains("LoginForm"));
        // Should have form content type
        assert!(output_str.contains("fn request_content_type"));
        assert!(output_str.contains("application/x-www-form-urlencoded"));
        // Should have request_body_required returning false
        assert!(output_str.contains("fn request_body_required"));
        assert!(output_str.contains("Some (false)"));
    }

    #[test]
    fn test_get_with_json_body_no_request_schema() {
        // GET handlers should not generate requestBody even if they have Json<T> parameter
        let path = quote!("/users");
        let input = quote! {
            async fn list_users(body: Json<FilterRequest>) -> Json<Vec<UserResponse>> {
                Json(vec![])
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Should NOT generate request_schema for GET method
        assert!(!output_str.contains("fn request_schema"));
        assert!(!output_str.contains("fn request_content_type"));
        assert!(!output_str.contains("fn request_body_required"));
    }

    #[test]
    fn test_delete_with_json_body_no_request_schema() {
        // DELETE handlers should not generate requestBody even if they have Json<T> parameter
        let path = quote!("/users/:id");
        let input = quote! {
            async fn delete_user(body: Json<DeleteRequest>) -> StatusCode {
                StatusCode::NO_CONTENT
            }
        };

        let output = route_macro_core("DELETE", path, input);
        let output_str = output.to_string();

        // Should NOT generate request_schema for DELETE method
        assert!(!output_str.contains("fn request_schema"));
        assert!(!output_str.contains("fn request_content_type"));
        assert!(!output_str.contains("fn request_body_required"));
    }

    #[test]
    fn test_no_body_no_request_schema_or_content_type() {
        let path = quote!("/users");
        let input = quote! {
            async fn list_users() -> Json<Vec<UserResponse>> {
                Json(vec![])
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Should NOT generate request_schema or request_content_type for handlers without body
        assert!(!output_str.contains("fn request_schema"));
        assert!(!output_str.contains("fn request_content_type"));
    }

    #[test]
    fn test_non_json_return_type_no_response_schema() {
        let path = quote!("/health");
        let input = quote! {
            async fn health() -> &'static str {
                "ok"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Check response_schema method is NOT generated for non-Json types
        assert!(!output_str.contains("fn response_schema"));
        assert!(!output_str.contains("openapi_schema_for"));
    }

    #[test]
    fn test_user_state_variable_not_shadowed() {
        // Regression test for issue #134 - user naming their extractor 'state'
        // should not conflict with internal macro variables
        let path = quote!("/users");
        let input = quote! {
            async fn list_users(state: rapina::extract::State<MyState>) -> String {
                "ok".to_string()
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Internal variables should use __rapina_ prefix
        assert!(output_str.contains("__rapina_state"));
        assert!(output_str.contains("__rapina_params"));
        // User's variable 'state' should still be extracted
        assert!(output_str.contains("let state ="));
    }

    #[test]
    fn test_no_closure_wrapper_for_type_inference() {
        // Regression test for issue #134 - Result type inference should work
        let path = quote!("/users");
        let input = quote! {
            async fn get_user() -> Result<String, Error> {
                Ok("user".to_string())
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        // Should NOT use closure wrapper (|| async ...)
        assert!(!output_str.contains("|| async"));
        // Should use typed result with async block (: ReturnType = (async ...).await)
        assert!(output_str.contains("__rapina_result"));
        assert!(output_str.contains("Result < String , Error >"));
    }

    #[test]
    fn test_emits_route_descriptor() {
        let path = quote!("/users");
        let input = quote! {
            async fn list_users() -> &'static str {
                "users"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("inventory :: submit !"));
        assert!(output_str.contains("RouteDescriptor"));
        assert!(output_str.contains("method : \"GET\""));
        assert!(output_str.contains("path : \"/users\""));
        assert!(output_str.contains("handler_name : \"list_users\""));
        assert!(output_str.contains("is_public : false"));
        assert!(output_str.contains("__rapina_register_list_users"));
    }

    #[test]
    fn test_emits_route_descriptor_with_method() {
        let path = quote!("/users");
        let input = quote! {
            async fn create_user() -> &'static str {
                "created"
            }
        };

        let output = route_macro_core("POST", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("method : \"POST\""));
        assert!(output_str.contains("__rapina_router . post"));
    }

    #[test]
    fn test_public_attr_below_route_sets_is_public() {
        let path = quote!("/health");
        let input = quote! {
            #[public]
            async fn health() -> &'static str {
                "ok"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("is_public : true"));
    }

    #[test]
    fn test_cache_attr_injects_ttl_header() {
        let path = quote!("/products");
        let input = quote! {
            #[cache(ttl = 60)]
            async fn list_products() -> &'static str {
                "products"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("x-rapina-cache-ttl"));
        assert!(output_str.contains("60"));
    }

    #[test]
    fn test_relay_macro_generates_wrapper_and_inventory() {
        let attr = quote!("room:*");
        let input = quote! {
            async fn room(event: rapina::relay::RelayEvent, relay: rapina::relay::Relay) -> Result<(), rapina::error::Error> {
                Ok(())
            }
        };

        let output = relay_macro_impl(attr, input);
        let output_str = output.to_string();

        // Original function is preserved
        assert!(output_str.contains("async fn room"));
        // Wrapper function is generated
        assert!(output_str.contains("__rapina_channel_room"));
        // Inventory submission
        assert!(output_str.contains("inventory :: submit !"));
        assert!(output_str.contains("ChannelDescriptor"));
        assert!(output_str.contains("pattern : \"room:*\""));
        assert!(output_str.contains("is_prefix : true"));
        assert!(output_str.contains("match_prefix : \"room:\""));
        assert!(output_str.contains("handler_name : \"room\""));
    }

    #[test]
    fn test_relay_macro_exact_match() {
        let attr = quote!("chat:lobby");
        let input = quote! {
            async fn lobby(event: rapina::relay::RelayEvent) -> Result<(), rapina::error::Error> {
                Ok(())
            }
        };

        let output = relay_macro_impl(attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("is_prefix : false"));
        assert!(output_str.contains("match_prefix : \"chat:lobby\""));
    }

    #[test]
    fn test_relay_macro_extracts_additional_params() {
        let attr = quote!("room:*");
        let input = quote! {
            async fn room(
                event: rapina::relay::RelayEvent,
                relay: rapina::relay::Relay,
                log: rapina::extract::State<TestLog>,
            ) -> Result<(), rapina::error::Error> {
                Ok(())
            }
        };

        let output = relay_macro_impl(attr, input);
        let output_str = output.to_string();

        // Both extractors should use FromRequestParts
        assert!(output_str.contains("let relay ="));
        assert!(output_str.contains("let log ="));
        assert!(output_str.contains("FromRequestParts"));
    }

    #[test]
    fn test_no_cache_attr_no_ttl_header() {
        let path = quote!("/products");
        let input = quote! {
            async fn list_products() -> &'static str {
                "products"
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(!output_str.contains("x-rapina-cache-ttl"));
    }

    #[test]
    fn test_cache_attr_with_extractors() {
        let path = quote!("/users/:id");
        let input = quote! {
            #[cache(ttl = 120)]
            async fn get_user(id: rapina::extract::Path<u64>) -> String {
                format!("{}", id.into_inner())
            }
        };

        let output = route_macro_core("GET", path, input);
        let output_str = output.to_string();

        assert!(output_str.contains("x-rapina-cache-ttl"));
        assert!(output_str.contains("120"));
        // Single arg uses FromRequest (positional convention)
        assert!(output_str.contains("FromRequest"));
    }

    #[test]
    fn test_group_param_joins_path() {
        let attr = quote!("/users", group = "/api");
        let input = quote! {
            async fn list_users() -> &'static str {
                "users"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/users\""));
        assert!(output_str.contains("__rapina_router . get (\"/api/users\""));
    }

    #[test]
    fn test_group_param_with_nested_prefix() {
        let attr = quote!("/items", group = "/api/v1");
        let input = quote! {
            async fn list_items() -> &'static str {
                "items"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/v1/items\""));
    }

    #[test]
    fn test_without_group_param_backward_compatible() {
        let attr = quote!("/users");
        let input = quote! {
            async fn list_users() -> &'static str {
                "users"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/users\""));
        assert!(output_str.contains("__rapina_router . get (\"/users\""));
    }

    #[test]
    #[should_panic(expected = "group prefix must start with `/`")]
    fn test_group_prefix_must_start_with_slash() {
        let attr = quote!("/users", group = "api");
        let input = quote! {
            async fn list_users() -> &'static str {
                "users"
            }
        };

        route_macro_core("GET", attr, input);
    }

    #[test]
    fn test_group_with_trailing_slash_normalized() {
        let attr = quote!("/users", group = "/api/");
        let input = quote! {
            async fn list_users() -> &'static str {
                "users"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/users\""));
    }

    #[test]
    fn test_group_with_public_attr() {
        let attr = quote!("/health", group = "/api");
        let input = quote! {
            #[public]
            async fn health() -> &'static str {
                "ok"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/health\""));
        assert!(output_str.contains("is_public : true"));
    }

    #[test]
    fn test_group_with_cache_attr() {
        let attr = quote!("/products", group = "/api");
        let input = quote! {
            #[cache(ttl = 60)]
            async fn list_products() -> &'static str {
                "products"
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/products\""));
        assert!(output_str.contains("x-rapina-cache-ttl"));
        assert!(output_str.contains("60"));
    }

    #[test]
    fn test_group_with_errors_attr() {
        let attr = quote!("/users", group = "/api");
        let input = quote! {
            #[errors(UserError)]
            async fn get_user() -> Result<Json<UserResponse>> {
                Ok(Json(UserResponse { id: 1 }))
            }
        };

        let output = route_macro_core("GET", attr, input);
        let output_str = output.to_string();

        assert!(output_str.contains("path : \"/api/users\""));
        assert!(output_str.contains("fn error_responses"));
        assert!(output_str.contains("UserError"));
    }

    #[test]
    fn test_group_with_all_methods() {
        for method in &["GET", "POST", "PUT", "DELETE"] {
            let attr = quote!("/items", group = "/api");
            let input = quote! {
                async fn handler() -> &'static str {
                    "ok"
                }
            };

            let output = route_macro_core(method, attr, input);
            let output_str = output.to_string();

            assert!(
                output_str.contains("path : \"/api/items\""),
                "{method} should produce /api/items"
            );
            let method_lower = method.to_lowercase();
            assert!(
                output_str.contains(&format!("__rapina_router . {method_lower}")),
                "{method} should use .{method_lower}() on router"
            );
        }
    }

    #[test]
    fn test_join_paths_basic() {
        assert_eq!(join_paths("/api", "/users"), "/api/users");
        assert_eq!(join_paths("/api/v1", "/items"), "/api/v1/items");
    }

    #[test]
    fn test_join_paths_trailing_slash() {
        assert_eq!(join_paths("/api/", "/users"), "/api/users");
    }

    #[test]
    fn test_join_paths_empty_path() {
        assert_eq!(join_paths("/api", ""), "/api");
        assert_eq!(join_paths("/api", "/"), "/api");
    }

    #[test]
    fn test_join_paths_empty_prefix() {
        assert_eq!(join_paths("", "/users"), "/users");
        assert_eq!(join_paths("", ""), "/");
    }
}
