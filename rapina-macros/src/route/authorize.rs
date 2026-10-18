//! `#[authorize]` macro parameter detection and the extraction code it generates.

use crate::route::headers::detect_header_type;
use proc_macro2::Ident;
use quote::{quote, quote_spanned};
use syn::__private::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Error, FnArg, Pat, PatIdent, PatType, Token, Type, parenthesized};

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
pub(crate) struct AuthorizeArgs {
    pub(crate) auth_fn: syn::Path,
    pub(crate) deps: Vec<Type>,
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
/// route's normal extractor bindings have been created. Reused handler dependencies must be
/// validated to implement FromRequestParts to reject body-consuming authorization dependencies.
pub(crate) struct AuthorizePlan {
    /// Compile-time validation for reused authorization dependencies. Reused dependencies must
    /// implement FromRequestParts unless they are Header<T>, which is extracted through the
    /// macro-generated header path.
    ///
    /// These checks do not perform runtime extraction and do not require
    /// request parts.
    pub(crate) reused_dependency_validation: proc_macro2::TokenStream,

    /// Dependencies not present in the handler signature. These are extracted
    /// from request parts before handler extraction consumes the request.
    pub(crate) extracts: proc_macro2::TokenStream,

    /// Invokes the authorization function after reusable handler bindings have
    /// been created.
    pub(crate) call: proc_macro2::TokenStream,

    /// Whether `extracts` needs access to `__rapina_parts`.
    pub(crate) needs_request_parts: bool,
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
pub(crate) fn build_authorize_plan(
    inputs: &Punctuated<FnArg, Token![,]>,
    auth: &AuthorizeArgs,
) -> syn::Result<AuthorizePlan> {
    let auth_fn = &auth.auth_fn;

    let mut reused_dependency_validation = Vec::new();
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

            // Reused authorization dependencies must not be body-consuming. Body-consuming
            // extractors, e.g. Json<T>, Form<T>, and Validated<T>, are rejected.
            // Header<T> is a special macro-managed extractor. Its header name comes
            // from the handler parameter and #[header(...)] attribute, so it cannot use
            // the generic FromRequestParts implementation. All other reused
            // authorization dependencies must implement FromRequestParts.
            if detect_header_type(dependency_type).is_none() {
                let dependency_span = dependency_type.span();

                reused_dependency_validation.push(quote_spanned! { dependency_span =>
                    const _: () = {
                        const fn __rapina_require_from_request_parts<T>()
                        where
                            T: rapina::extract::FromRequestParts,
                        {
                        }

                        __rapina_require_from_request_parts::<#dependency_type>();
                    };
                });
            }

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

    let reused_dependency_validation = quote! {
        #(#reused_dependency_validation)*
    };

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
        reused_dependency_validation,
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

#[cfg(test)]
mod tests {
    use crate::public_macro_impl;
    use crate::route::authorize::{AuthorizeArgs, build_authorize_plan};
    use crate::route::route_macro_core;
    use quote::quote;
    use syn::parse_quote;

    #[test]
    fn authorize_args_parse_zero_dependencies() {
        let args: AuthorizeArgs =
            syn::parse2(quote! { authz::authorize }).expect("authorization arguments should parse");

        assert_eq!(
            args.auth_fn
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>(),
            ["authz", "authorize"]
        );
        assert!(args.deps.is_empty());
    }

    #[test]
    fn authorize_args_parse_multiple_dependencies() {
        let args: AuthorizeArgs = syn::parse2(quote! {
            authz::authorize(
                rapina::extract::State<AppState>,
                rapina::extract::Headers,
            )
        })
        .expect("authorization arguments should parse");

        assert_eq!(args.deps.len(), 2);

        let dependencies = args
            .deps
            .iter()
            .map(|dependency| quote!(#dependency).to_string())
            .collect::<Vec<_>>();

        assert_eq!(
            dependencies,
            [
                "rapina :: extract :: State < AppState >",
                "rapina :: extract :: Headers",
            ]
        );
    }

    #[test]
    fn authorize_args_accept_trailing_dependency_comma() {
        let args: AuthorizeArgs = syn::parse2(quote! {
            authorize(Headers,)
        })
        .expect("a trailing dependency comma should be accepted");

        assert_eq!(args.deps.len(), 1);
    }

    #[test]
    fn authorize_args_reject_non_parenthesized_dependencies() {
        let error = match syn::parse2::<AuthorizeArgs>(quote! {
            authorize, Headers
        }) {
            Ok(_) => panic!("dependencies outside parentheses must be rejected"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("expected dependency list in parentheses")
        );
    }

    #[test]
    fn authorize_args_reject_trailing_tokens() {
        let error = match syn::parse2::<AuthorizeArgs>(quote! {
            authorize(Headers) unexpected
        }) {
            Ok(_) => panic!("trailing tokens must be rejected"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("unexpected tokens after authorization dependencies")
        );
    }

    #[test]
    fn authorize_rejects_public_conflict_below_route_macro() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[public]
                #[authorize(policy)]
                async fn admin() -> &'static str {
                    "admin"
                }
            },
        )
        .to_string();

        assert!(output.contains("compile_error"));
        assert!(output.contains("contradicts"));
        assert!(output.contains("public"));
    }

    #[test]
    fn public_rejects_authorize_below_route_macro() {
        let output = public_macro_impl(quote! {
            #[get("/admin")]
            #[public]
            #[authorize(policy)]
            async fn admin() -> &'static str {
                "admin"
            }
        })
        .to_string();

        assert!(output.contains("compile_error"));
        assert!(output.contains("contradicts"));
    }

    #[test]
    fn authorize_rejects_reused_non_identifier_pattern() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[authorize(policy(State<AppState>))]
                async fn admin(
                    State(state): State<AppState>,
                ) -> &'static str {
                    "admin"
                }
            },
        )
        .to_string();

        assert!(output.contains("compile_error"));
        assert!(output.contains("simple identifier parameters"));
    }

    #[test]
    fn authorize_rejects_duplicate_attributes() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[authorize(first_policy)]
                #[authorize(second_policy)]
                async fn admin() -> &'static str {
                    "admin"
                }
            },
        )
        .to_string();

        assert!(output.contains("compile_error"));
        assert!(output.contains("authorize"));
        assert!(output.contains("can only be added once per handler"));
    }

    #[test]
    fn authorize_accepts_single_identifier_dependency() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[authorize(policy(State<AppState>))]
                async fn admin(state: State<AppState>) -> &'static str {
                    let _ = state;
                    "admin"
                }
            },
        )
        .to_string();

        assert!(!output.contains("compile_error"));
    }

    #[test]
    fn authorize_accepts_multiple_identifier_dependencies() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[authorize(policy(
                    State<AppState>,
                    Headers,
                ))]
                async fn admin(
                    state: State<AppState>,
                    headers: Headers,
                ) -> &'static str {
                    let _ = (state, headers);
                    "admin"
                }
            },
        )
        .to_string();

        assert!(!output.contains("compile_error"));
    }

    #[test]
    fn authorize_accepts_trailing_dependency_comma() {
        let output = route_macro_core(
            "GET",
            quote!("/admin"),
            quote! {
                #[authorize(policy(State<AppState>,))]
                async fn admin(state: State<AppState>) -> &'static str {
                    let _ = state;
                    "admin"
                }
            },
        )
        .to_string();

        assert!(!output.contains("compile_error"));
    }

    #[test]
    fn authorize_reused_handler_dependency_generates_parts_validation() {
        let inputs = parse_quote! {
            state: State<AppState>
        };

        let auth = parse_quote! {
            authorize(State<AppState>)
        };

        let plan = build_authorize_plan(&inputs, &auth).unwrap();
        let validation = plan.reused_dependency_validation.to_string();

        assert!(validation.contains("__rapina_require_from_request_parts"));
        assert!(validation.contains("FromRequestParts"));
        assert!(validation.contains("State < AppState >"));
    }

    #[test]
    fn authorize_missing_handler_dependency_does_not_generate_reused_validation() {
        let inputs = parse_quote! {
            value: String
        };

        let auth = parse_quote! {
            authorize(State<AppState>)
        };

        let plan = build_authorize_plan(&inputs, &auth).unwrap();

        assert!(plan.reused_dependency_validation.is_empty());
        assert!(!plan.extracts.is_empty());
        assert!(plan.needs_request_parts);
    }

    #[test]
    fn authorize_all_reused_dependencies_generate_parts_validation() {
        let inputs = parse_quote! {
            state: State<AppState>,
            headers: HeaderMap
        };

        let auth = parse_quote! {
            authorize(State<AppState>, HeaderMap)
        };

        let plan = build_authorize_plan(&inputs, &auth).unwrap();
        let validation = plan.reused_dependency_validation.to_string();

        // contains four mentions of __rapina_require_from_request_parts in total: two declarations, two invocations
        assert_eq!(
            validation
                .matches("__rapina_require_from_request_parts")
                .count(),
            4
        );
        assert!(validation.contains("State < AppState >"));
        assert!(validation.contains("HeaderMap"));
        assert_eq!(validation.matches("FromRequestParts").count(), 2);
    }
}
