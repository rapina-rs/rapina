use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, Pat};

mod schema;

#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("GET", attr, item)
}

#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("POST", attr, item)
}

#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro("PUT", attr, item)
}

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
    let path: LitStr = syn::parse2(attr).expect("expected path as string literal");
    let path_str = path.value();
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
                    Some(serde_json::to_value(rapina::schemars::schema_for!(#inner_type)).unwrap())
                }
            }
        } else {
            quote! {}
        }
    } else {
        quote! {}
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
        let mut parts_extractions = Vec::new();
        let mut body_extractors: Vec<(syn::Ident, Box<syn::Type>)> = Vec::new();

        for arg in &args {
            if let FnArg::Typed(pat_type) = arg
                && let Pat::Ident(pat_ident) = &*pat_type.pat
            {
                let arg_name = &pat_ident.ident;
                let arg_type = &pat_type.ty;

                let type_str = quote!(#arg_type).to_string();
                if is_parts_only_extractor(&type_str) {
                    parts_extractions.push(quote! {
                        let #arg_name = match <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(&__rapina_parts, &__rapina_params, &__rapina_state).await {
                            Ok(v) => v,
                            Err(e) => return rapina::response::IntoResponse::into_response(e),
                        };
                    });
                } else {
                    body_extractors.push((arg_name.clone(), arg_type.clone()));
                }
            }
        }

        let body_extraction = if body_extractors.is_empty() {
            quote! {}
        } else if body_extractors.len() == 1 {
            let (arg_name, arg_type) = &body_extractors[0];
            quote! {
                let __rapina_req = rapina::http::Request::from_parts(__rapina_parts, __rapina_body);
                let #arg_name = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                    Ok(v) => v,
                    Err(e) => return rapina::response::IntoResponse::into_response(e),
                };
            }
        } else {
            let names: Vec<_> = body_extractors.iter().map(|(n, _)| n.to_string()).collect();
            panic!(
                "Multiple body-consuming extractors are not supported: {}. Only one extractor can consume the request body.",
                names.join(", ")
            );
        };

        let inner_block = &func.block;

        quote! {
            let (__rapina_parts, __rapina_body) = __rapina_req.into_parts();
            #(#parts_extractions)*
            #body_extraction
            let __rapina_result #return_type_annotation = (async #inner_block).await;
            let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
            #cache_header_injection
            __rapina_response
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
                error_responses: <#func_name as rapina::handler::Handler>::error_responses,
                register: #register_fn_name,
            }
        }
    }
}

fn is_parts_only_extractor(type_str: &str) -> bool {
    type_str.contains("Path")
        || type_str.contains("Query")
        || type_str.contains("Headers")
        || type_str.contains("State")
        || type_str.contains("Context")
        || type_str.contains("CurrentUser")
        || type_str.contains("Db")
        || type_str.contains("Cookie")
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
    use super::route_macro_core;
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

        // Check struct is generated
        assert!(output_str.contains("struct get_user"));
        // Check extraction code is present
        assert!(output_str.contains("FromRequestParts"));
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
    #[should_panic(expected = "Multiple body-consuming extractors are not supported")]
    fn test_multiple_body_extractors_panics() {
        let path = quote!("/users");
        let input = quote! {
            async fn handler(
                body1: rapina::extract::Json<String>,
                body2: rapina::extract::Json<String>
            ) -> String {
                "ok".to_string()
            }
        };

        route_macro_core("POST", path, input);
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

        // Check response_schema method is generated with schema_for!
        assert!(output_str.contains("fn response_schema"));
        assert!(output_str.contains("rapina :: schemars :: schema_for !"));
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
        assert!(output_str.contains("rapina :: schemars :: schema_for !"));
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
        assert!(!output_str.contains("schema_for"));
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
        assert!(output_str.contains("FromRequestParts"));
    }
}
