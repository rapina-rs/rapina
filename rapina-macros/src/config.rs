//! The `derive(Config)` macro.

use quote::quote;

pub(crate) fn derive_config_impl(input: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
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
    use super::derive_config_impl;
    use quote::quote;

    #[test]
    fn test_config_env_attr_overrides_field_name() {
        let input = quote! {
            struct AppConfig {
                #[env = "PG_DSN"]
                database_url: String,
            }
        };

        let output = derive_config_impl(input);
        let output_str = output.to_string();

        assert!(output_str.contains("fn from_env"));
        assert!(output_str.contains("get_env_parsed"));
        assert!(output_str.contains("\"PG_DSN\""));
        assert!(!output_str.contains("\"DATABASE_URL\""));
    }

    #[test]
    fn test_config_default_attr_skips_missing_check() {
        let input = quote! {
            struct AppConfig {
                #[default = "8080"]
                port: u16,
            }
        };

        let output = derive_config_impl(input);
        let output_str = output.to_string();

        assert!(output_str.contains("get_env_or"));
        assert!(output_str.contains("\"PORT\""));
        assert!(output_str.contains("\"8080\""));
        assert!(!output_str.contains("get_env_parsed"));
        // `missing.push` is the only `push` the macro emits, so its absence is what
        // proves a defaulted field is never reported as missing
        assert!(!output_str.contains("push"));
    }

    #[test]
    fn test_config_field_without_default_is_required() {
        let input = quote! {
            struct AppConfig {
                api_key: String,
            }
        };

        let output = derive_config_impl(input);
        let output_str = output.to_string();

        assert!(output_str.contains("get_env_parsed"));
        assert!(output_str.contains("\"API_KEY\""));
        assert!(output_str.contains("push"));
    }

    #[test]
    #[should_panic(expected = "Config derive only supports structs")]
    fn test_config_rejects_non_struct_input() {
        derive_config_impl(quote! {
            enum AppConfig {
                Dev,
                Prod,
            }
        });
    }

    #[test]
    #[should_panic(expected = "Config derive only supports structs with named fields")]
    fn test_config_rejects_tuple_struct() {
        derive_config_impl(quote! {
            struct AppConfig(String);
        });
    }
}
