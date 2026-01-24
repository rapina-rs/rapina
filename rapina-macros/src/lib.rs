use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, LitStr, Pat};

#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro(attr, item)
}

#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro(attr, item)
}

#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro(attr, item)
}

#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro(attr, item)
}

fn route_macro_core(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let _path: LitStr = syn::parse2(attr).expect("expected path as string literal");
    let func: ItemFn = syn::parse2(item).expect("expected function");

    let func_name = &func.sig.ident;
    let func_name_str = func_name.to_string();
    let func_vis = &func.vis;

    let args: Vec<_> = func.sig.inputs.iter().collect();

    // Build the handler body
    let handler_body = if args.is_empty() {
        let inner_block = &func.block;
        quote! {
            rapina::response::IntoResponse::into_response((|| async #inner_block)().await)
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
                        let #arg_name = match <#arg_type as rapina::extract::FromRequestParts>::from_request_parts(&parts, &params, &state).await {
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
                let req = http::Request::from_parts(parts, body);
                let #arg_name = match <#arg_type as rapina::extract::FromRequest>::from_request(req, &params, &state).await {
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
            let (parts, body) = req.into_parts();
            #(#parts_extractions)*
            #body_extraction
            rapina::response::IntoResponse::into_response((|| async #inner_block)().await)
        }
    };

    // Generate the struct and Handler impl
    quote! {
        #[derive(Clone, Copy)]
        #[allow(non_camel_case_types)]
        #func_vis struct #func_name;

        impl rapina::handler::Handler for #func_name {
            const NAME: &'static str = #func_name_str;

            fn call(
                &self,
                req: hyper::Request<hyper::body::Incoming>,
                params: rapina::extract::PathParams,
                state: std::sync::Arc<rapina::state::AppState>,
            ) -> std::pin::Pin<Box<dyn std::future::Future<Output = hyper::Response<rapina::response::BoxBody>> + Send>> {
                Box::pin(async move {
                    #handler_body
                })
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
}

fn route_macro(attr: TokenStream, item: TokenStream) -> TokenStream {
    route_macro_core(attr.into(), item.into()).into()
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

        let output = route_macro_core(path, input);
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

        let output = route_macro_core(path, input);
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

        let output = route_macro_core(path, input);
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

        route_macro_core(path, input);
    }

    #[test]
    #[should_panic(expected = "expected function")]
    fn test_invalid_input_panics() {
        let path = quote!("/");
        let invalid_input = quote! { not_a_function };

        route_macro_core(path, invalid_input);
    }
}
