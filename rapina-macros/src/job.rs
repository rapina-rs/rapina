//! The `#[job]` attribute macro.

use quote::quote;
use syn::spanned::Spanned;
use syn::{FnArg, ItemFn};

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

pub(crate) fn job_macro_impl(
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

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

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
