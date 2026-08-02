use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr};

mod headers;
mod metadata;

use headers::{HeaderParamMeta, collect_header_params, detect_header_type, gen_header_extraction};
use metadata::{
    extract_cache_attr, extract_doc_description, extract_errors_attr, extract_json_inner_type,
    extract_public_attr, extract_request_body_meta,
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
pub(crate) fn join_paths(prefix: &str, path: &str) -> String {
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

pub(crate) fn route_macro(method: &str, attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro_core(method, attr.into(), item.into()).into()
}

pub(crate) fn route_macro_core(
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

    // Resolve description: explicit attr wins, then first rustdoc line, then None
    let description_value: Option<String> = route_attr
        .description
        .as_ref()
        .map(|l| l.value())
        .or_else(|| extract_doc_description(&func.attrs));

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

    // Collect header params (also strips #[header("name")] attrs from inputs)
    let header_params = match collect_header_params(&mut func.sig.inputs) {
        Ok(p) => p,
        Err(e) => return e.to_compile_error(),
    };

    // Build an index: arg_idx → &HeaderParamMeta for O(1) lookup during codegen
    let header_by_arg: std::collections::HashMap<usize, &HeaderParamMeta> =
        header_params.iter().map(|p| (p.arg_idx, p)).collect();

    // Build header_parameters() impl for the Handler trait
    let header_parameters_impl = if header_params.is_empty() {
        quote! {}
    } else {
        let entries = header_params.iter().map(|p| {
            let name = &p.name;
            let required = p.required;
            quote! {
                rapina::discovery::HeaderParamInfo {
                    name: #name.to_string(),
                    required: #required,
                }
            }
        });
        quote! {
            fn header_parameters() -> Vec<rapina::discovery::HeaderParamInfo> {
                vec![#(#entries),*]
            }
        }
    };

    // Build description() impl for the Handler trait
    let description_impl = if let Some(ref desc) = description_value {
        quote! {
            fn description() -> Option<&'static str> {
                Some(#desc)
            }
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
        let inner_block = &func.block;

        // Check if all args are header extractors (so we never need to split req into parts)
        let all_headers = args.iter().all(|arg| {
            if let FnArg::Typed(pt) = arg {
                detect_header_type(&pt.ty).is_some()
            } else {
                false
            }
        });

        // Check if the single arg is a header type
        let single_is_header = args.len() == 1
            && args.first().is_some_and(|arg| {
                if let FnArg::Typed(pt) = arg {
                    detect_header_type(&pt.ty).is_some()
                } else {
                    false
                }
            });

        if args.len() == 1 && !single_is_header {
            // Single non-header arg: pass request directly to FromRequest
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
        } else if all_headers {
            // All args are header extractors — extract from parts, no body split needed
            let mut header_extractions = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                if let FnArg::Typed(pat_type) = arg {
                    let pat = &pat_type.pat;
                    let tmp = syn::Ident::new(
                        &format!("__rapina_arg_{}", i),
                        proc_macro2::Span::call_site(),
                    );
                    let meta = header_by_arg.get(&i).expect("all_headers: missing meta");
                    header_extractions.push(gen_header_extraction(
                        &meta.inner_type,
                        meta.required,
                        &meta.name,
                        &tmp,
                    ));
                    header_extractions.push(quote! { let #pat = #tmp; });
                }
            }
            quote! {
                let (__rapina_parts, _) = __rapina_req.into_parts();
                #(#header_extractions)*
                let __rapina_result #return_type_annotation = (async #inner_block).await;
                let __rapina_response = rapina::response::IntoResponse::into_response(__rapina_result);
                #cache_header_injection
                __rapina_response
            }
        } else {
            // Multiple args: all but last use FromRequestParts (or header extraction), last uses FromRequest
            let mut parts_extractions = Vec::new();

            for (i, arg) in args[..args.len() - 1].iter().enumerate() {
                if let FnArg::Typed(pat_type) = arg {
                    let pat = &pat_type.pat;
                    let arg_type = &pat_type.ty;
                    let tmp = syn::Ident::new(
                        &format!("__rapina_arg_{}", i),
                        proc_macro2::Span::call_site(),
                    );
                    if detect_header_type(arg_type).is_some() {
                        let meta = header_by_arg.get(&i).expect("mixed: missing meta");
                        parts_extractions.push(gen_header_extraction(
                            &meta.inner_type,
                            meta.required,
                            &meta.name,
                            &tmp,
                        ));
                        parts_extractions.push(quote! { let #pat = #tmp; });
                    } else {
                        parts_extractions.push(quote! {
                            let #tmp = match <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(&__rapina_parts, &__rapina_params, &__rapina_state).await {
                                Ok(v) => v,
                                Err(e) => return rapina::response::IntoResponse::into_response(e),
                            };
                            let #pat = #tmp;
                        });
                    }
                }
            }

            let last_arg = args.last().unwrap();
            let last_extraction = if let FnArg::Typed(pat_type) = last_arg {
                let pat = &pat_type.pat;
                let arg_type = &pat_type.ty;
                let last_idx = args.len() - 1;
                let tmp = syn::Ident::new(
                    &format!("__rapina_arg_{}", last_idx),
                    proc_macro2::Span::call_site(),
                );
                if detect_header_type(arg_type).is_some() {
                    let meta = header_by_arg
                        .get(&last_idx)
                        .expect("last arg: missing meta");
                    let header_extr =
                        gen_header_extraction(&meta.inner_type, meta.required, &meta.name, &tmp);
                    quote! {
                        #header_extr
                        let #pat = #tmp;
                        // Reconstruct the request (body not consumed for header-only last arg)
                        let _ = __rapina_body;
                    }
                } else {
                    quote! {
                        let __rapina_req = rapina::http::Request::from_parts(__rapina_parts, __rapina_body);
                        let #tmp = match <#arg_type as rapina::extract::FromRequest>::from_request(__rapina_req, &__rapina_params, &__rapina_state).await {
                            Ok(v) => v,
                            Err(e) => return rapina::response::IntoResponse::into_response(e),
                        };
                        let #pat = #tmp;
                    }
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
            #header_parameters_impl
            #description_impl

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
                header_parameters: <#func_name as rapina::handler::Handler>::header_parameters,
                description: <#func_name as rapina::handler::Handler>::description,
                register: #register_fn_name,
            }
        }
    }
}
