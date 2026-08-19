//! `Header<T>` parameter detection and the extraction code it generates.

use heck::ToKebabCase;
use quote::quote;
use syn::{LitStr, Pat};

/// Generate the extraction code for a `Header<T>` or `Option<Header<T>>` parameter.
///
/// `header_name` is the resolved HTTP header name (kebab-case, possibly from
/// an explicit `#[header("name")]` attribute).
pub(crate) fn gen_header_extraction(
    inner_type: &syn::Type,
    required: bool,
    header_name: &str,
    tmp: &syn::Ident,
) -> proc_macro2::TokenStream {
    if required {
        quote! {
            let #tmp = match rapina::extract::extract_header::<#inner_type>(&__rapina_parts, #header_name) {
                Ok(v) => rapina::extract::Header::new(#header_name, v),
                Err(e) => return rapina::response::IntoResponse::into_response(e),
            };
        }
    } else {
        quote! {
            let #tmp = match rapina::extract::extract_optional_header::<#inner_type>(&__rapina_parts, #header_name) {
                Ok(Some(v)) => Some(rapina::extract::Header::new(#header_name, v)),
                Ok(None) => None,
                Err(e) => return rapina::response::IntoResponse::into_response(e),
            };
        }
    }
}

/// Metadata about a single `Header<T>` or `Option<Header<T>>` parameter.
pub(crate) struct HeaderParamMeta {
    /// Zero-based index of this param in the handler's argument list.
    pub(crate) arg_idx: usize,
    /// The HTTP header name (e.g. "x-request-id").
    pub(crate) name: String,
    /// Whether the parameter is required (`Header<T>`) or optional (`Option<Header<T>>`).
    pub(crate) required: bool,
    /// The inner `T` type (for generating the extraction call).
    pub(crate) inner_type: syn::Type,
}

/// Extract `#[header("name")]` attribute from a parameter's attribute list.
///
/// Returns the explicit header name if present, removing the attribute.
fn extract_header_attr(attrs: &mut Vec<syn::Attribute>) -> Option<String> {
    let idx = attrs
        .iter()
        .position(|attr| attr.path().is_ident("header"))?;
    let attr = attrs.remove(idx);
    let lit: LitStr = attr.parse_args().expect("expected #[header(\"name\")]");
    Some(lit.value())
}

/// Detect if `ty` is `Header<T>` (required) or `Option<Header<T>>` (optional).
///
/// Returns `Some((inner_type, required))` on match, `None` otherwise.
///
/// Matches `Header<T>` (bare or path-qualified as `extract::Header<T>` /
/// `rapina::extract::Header<T>`).  Any other qualifying path (e.g.
/// `my_crate::Header<T>`) returns `None`, so user-defined types named `Header`
/// fall through to normal handling instead of producing a confusing compile
/// error from macro-generated code.
pub(crate) fn detect_header_type(ty: &syn::Type) -> Option<(syn::Type, bool)> {
    let syn::Type::Path(type_path) = ty else {
        return None;
    };
    let last = type_path.path.segments.last()?;

    // Direct Header<T>
    if last.ident == "Header" {
        // When the type is qualified (e.g. `foo::Header`), only treat it as
        // rapina's Header if the leading path is a known rapina prefix.
        // Bare `Header` (imported via prelude) has no leading segments and
        // is always accepted.
        let segments: Vec<_> = type_path.path.segments.iter().collect();
        let is_rapina_header = match segments.len() {
            1 => true,                                                            // bare `Header`
            2 => segments[0].ident == "extract", // `extract::Header`
            3 => segments[0].ident == "rapina" && segments[1].ident == "extract", // `rapina::extract::Header`
            _ => false,
        };
        if is_rapina_header {
            if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
                if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                    return Some((inner.clone(), true));
                }
            }
        }
    }

    // Option<Header<T>>
    if last.ident == "Option" {
        if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                if let Some((inner_t, _)) = detect_header_type(inner) {
                    return Some((inner_t, false));
                }
            }
        }
    }

    None
}

/// Collect all `Header<T>` / `Option<Header<T>>` parameters from handler inputs.
///
/// Also strips any `#[header("name")]` attributes from the parameters
/// (they are not valid Rust attributes and must be removed before codegen).
pub(crate) fn collect_header_params(
    inputs: &mut syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]>,
) -> syn::Result<Vec<HeaderParamMeta>> {
    let mut params = Vec::new();

    for (arg_idx, arg) in inputs.iter_mut().enumerate() {
        let syn::FnArg::Typed(pat_type) = arg else {
            continue;
        };

        let Some((inner_type, required)) = detect_header_type(&pat_type.ty) else {
            continue;
        };

        // Check for explicit #[header("name")] override on the parameter
        let explicit_name = extract_header_attr(&mut pat_type.attrs);

        // Derive header name from snake_case param name, or use explicit override.
        let name = if let Some(n) = explicit_name {
            n
        } else if let Pat::Ident(pat_ident) = &*pat_type.pat {
            pat_ident.ident.to_string().to_kebab_case()
        } else {
            // Destructure pattern — can't infer name, user must use #[header("name")]
            return Err(syn::Error::new_spanned(
                &*pat_type.pat,
                "Header<T> parameter with a destructure pattern must have a #[header(\"name\")] attribute",
            ));
        };

        params.push(HeaderParamMeta {
            arg_idx,
            name,
            required,
            inner_type,
        });
    }

    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::{collect_header_params, detect_header_type, gen_header_extraction};
    use quote::quote;

    fn inputs_of(func: syn::ItemFn) -> syn::punctuated::Punctuated<syn::FnArg, syn::Token![,]> {
        func.sig.inputs
    }

    #[test]
    fn detects_bare_header() {
        let ty: syn::Type = syn::parse_quote!(Header<String>);
        let (inner, required) = detect_header_type(&ty).expect("Header<T> should be detected");

        assert!(required);
        assert_eq!(quote!(#inner).to_string(), "String");
    }

    #[test]
    fn detects_qualified_header_paths() {
        for ty in [
            syn::parse_quote!(extract::Header<u64>),
            syn::parse_quote!(rapina::extract::Header<u64>),
        ] {
            let ty: syn::Type = ty;
            let (inner, required) = detect_header_type(&ty).expect("qualified Header<T>");

            assert!(required);
            assert_eq!(quote!(#inner).to_string(), "u64");
        }
    }

    #[test]
    fn detects_optional_header() {
        let ty: syn::Type = syn::parse_quote!(Option<Header<String>>);
        let (inner, required) = detect_header_type(&ty).expect("Option<Header<T>>");

        assert!(!required);
        assert_eq!(quote!(#inner).to_string(), "String");
    }

    #[test]
    fn ignores_non_header_types() {
        let ty: syn::Type = syn::parse_quote!(Json<Payload>);

        assert!(detect_header_type(&ty).is_none());
    }

    #[test]
    fn ignores_foreign_header_types() {
        // A user type also named `Header` must fall through to normal handling
        // instead of producing confusing errors from macro-generated code.
        let ty: syn::Type = syn::parse_quote!(my_crate::Header<String>);

        assert!(detect_header_type(&ty).is_none());
    }

    #[test]
    fn derives_kebab_case_name_from_param() {
        let mut inputs = inputs_of(syn::parse_quote! {
            async fn handler(x_request_id: Header<String>) {}
        });

        let params = collect_header_params(&mut inputs).expect("valid header param");

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "x-request-id");
        assert_eq!(params[0].arg_idx, 0);
        assert!(params[0].required);
    }

    #[test]
    fn explicit_attribute_overrides_derived_name() {
        let mut inputs = inputs_of(syn::parse_quote! {
            async fn handler(#[header("X-Api-Key")] api_key: Header<String>) {}
        });

        let params = collect_header_params(&mut inputs).expect("valid header param");

        assert_eq!(params[0].name, "X-Api-Key");
        // The attribute is not valid Rust, so it must be stripped before codegen.
        let syn::FnArg::Typed(pat_type) = &inputs[0] else {
            panic!("expected a typed argument");
        };
        assert!(pat_type.attrs.is_empty());
    }

    #[test]
    fn skips_non_header_params_and_tracks_positions() {
        let mut inputs = inputs_of(syn::parse_quote! {
            async fn handler(
                body: Json<Payload>,
                trace_id: Header<String>,
                tenant: Option<Header<String>>,
            ) {
            }
        });

        let params = collect_header_params(&mut inputs).expect("valid header params");

        assert_eq!(params.len(), 2);
        assert_eq!(params[0].arg_idx, 1);
        assert!(params[0].required);
        assert_eq!(params[1].arg_idx, 2);
        assert!(!params[1].required);
        assert_eq!(params[1].name, "tenant");
    }

    #[test]
    fn destructured_param_without_attribute_is_an_error() {
        let mut inputs = inputs_of(syn::parse_quote! {
            async fn handler(Header(id): Header<String>) {}
        });

        let Err(err) = collect_header_params(&mut inputs) else {
            panic!("destructured Header<T> without #[header(\"name\")] should be an error");
        };

        assert!(err.to_string().contains("#[header(\"name\")]"));
    }

    #[test]
    fn generates_required_extraction() {
        let inner: syn::Type = syn::parse_quote!(String);
        let tmp = syn::Ident::new("__rapina_arg_0", proc_macro2::Span::call_site());

        let output = gen_header_extraction(&inner, true, "x-request-id", &tmp).to_string();

        assert!(output.contains("extract_header :: < String >"));
        assert!(output.contains("\"x-request-id\""));
        assert!(!output.contains("extract_optional_header"));
    }

    #[test]
    fn generates_optional_extraction() {
        let inner: syn::Type = syn::parse_quote!(String);
        let tmp = syn::Ident::new("__rapina_arg_0", proc_macro2::Span::call_site());

        let output = gen_header_extraction(&inner, false, "x-tenant", &tmp).to_string();

        assert!(output.contains("extract_optional_header :: < String >"));
        assert!(output.contains("Ok (None) => None"));
    }
}
