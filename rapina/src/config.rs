//! Type-safe configuration loading from environment variables
//!
//! This module provides utilities for loading configuration from
//! environment variables and `.env` files

use std::env;
use std::str::FromStr;

/// Load environment variables from `.env` files if it exists.
///
/// Call this at the start of your application before accessing config.
pub fn load_dotenv() {
    let _ = dotenvy::dotenv();
}

/// Get a required environment variable.
///
/// Returns an error if the variable is not set.
pub fn get_env(key: &str) -> Result<String, ConfigError> {
    env::var(key).map_err(|_| ConfigError::Missing(key.to_string()))
}

/// Get an optional environment with a default value
pub fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Get and parse an environment variable.
pub fn get_env_parsed<T: FromStr>(key: &str) -> Result<T, ConfigError> {
    let value = get_env(key)?;
    value.parse().map_err(|_| ConfigError::Invalid {
        key: key.to_string(),
        value,
    })
}

/// Get and parse an environment variable with a default.
pub fn get_env_parsed_or<T: FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Configuration loading errors.
#[derive(Debug)]
pub enum ConfigError {
    /// Environment variable is not set.
    Missing(String),
    /// Multiple environment variables are not set.
    MissingMultiple(Vec<String>),
    /// Environment variable value is invalid.
    Invalid { key: String, value: String },
}
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(key) => {
                write!(f, "Missing required environment variable '{}'", key)
            }
            ConfigError::MissingMultiple(keys) => {
                writeln!(f, "Missing required environment variables:")?;
                for key in keys {
                    writeln!(f, "  - {}", key)?;
                }
                Ok(())
            }
            ConfigError::Invalid { key, value } => {
                write!(
                    f,
                    "Invalid value '{}' for environment variable '{}' (failed to parse as expected type)",
                    value, key
                )
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_missing() {
        let result = get_env("RAPINA_TEST_MISSING_VAR_12345");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_env_or_default() {
        let value = get_env_or("RAPINA_TEST_MISSING_VAR_12345", "default");
        assert_eq!(value, "default");
    }

    #[test]
    fn test_get_env_parsed_or_default() {
        let value: u16 = get_env_parsed_or("RAPINA_TEST_MISSING_VAR_12345", 3000);
        assert_eq!(value, 3000);
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::Missing("DATABASE_URL".to_string());
        assert_eq!(
            err.to_string(),
            "Missing required environment variable 'DATABASE_URL'"
        );

        let err = ConfigError::Invalid {
            key: "PORT".to_string(),
            value: "abc".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Invalid value 'abc' for environment variable 'PORT' (failed to parse as expected type)"
        );
    }
}
