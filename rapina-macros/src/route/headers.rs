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
