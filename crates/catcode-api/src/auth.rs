use axum::http::StatusCode;
use axum::http::request::Parts;

/// Authentication configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub mode: AuthMode,
    pub token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Authentication mode for the API server.
pub enum AuthMode {
    /// Only listen on 127.0.0.1, no auth required.
/// [`LocalOnly`].
    LocalOnly,
    /// Bearer token authentication.
/// [`Token`].
    Token,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: AuthMode::LocalOnly,
            token: None,
        }
    }
}

/// Extract and validate authentication from request parts.
///
/// Returns Ok(()) if authenticated, Err(status) if not.
pub fn validate_auth(config: &AuthConfig, parts: &Parts) -> Result<(), StatusCode> {
    match config.mode {
        AuthMode::LocalOnly => Ok(()),
        AuthMode::Token => {
            let token = config.token.as_deref().unwrap_or("");

            if token.is_empty() {
                return Err(StatusCode::INTERNAL_SERVER_ERROR);
            }

            let auth_header = parts
                .headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");

            if auth_header == format!("Bearer {}", token) {
                Ok(())
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn empty_parts() -> Parts {
        Request::builder().body(()).unwrap().into_parts().0
    }

    #[test]
    fn test_local_only_always_ok() {
        let config = AuthConfig {
            mode: AuthMode::LocalOnly,
            token: None,
        };
        let parts = empty_parts();
        assert!(validate_auth(&config, &parts).is_ok());
    }

    #[test]
    fn test_token_valid() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("secret123".to_string()),
        };
        let mut parts = empty_parts();
        parts.headers.insert(
            "authorization",
            "Bearer secret123".parse().unwrap(),
        );
        assert!(validate_auth(&config, &parts).is_ok());
    }

    #[test]
    fn test_token_invalid() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("secret123".to_string()),
        };
        let mut parts = empty_parts();
        parts
            .headers
            .insert("authorization", "Bearer wrong".parse().unwrap());
        assert_eq!(
            validate_auth(&config, &parts),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn test_token_missing() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("secret123".to_string()),
        };
        let parts = empty_parts();
        assert_eq!(
            validate_auth(&config, &parts),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn test_auth_config_default() {
        let config = AuthConfig::default();
        assert_eq!(config.mode, AuthMode::LocalOnly);
        assert!(config.token.is_none());
    }

    #[test]
    fn test_token_empty_token_returns_500() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some(String::new()),
        };
        let parts = empty_parts();
        assert_eq!(
            validate_auth(&config, &parts),
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        );
    }

    #[test]
    fn test_token_whitespace_token_matches() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("   ".to_string()),
        };
        let mut parts = empty_parts();
        parts.headers.insert(
            "authorization",
            "Bearer    ".parse().unwrap(),
        );
        assert!(validate_auth(&config, &parts).is_ok());
    }

    #[test]
    fn test_token_empty_bearer_prefix() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("secret".to_string()),
        };
        let mut parts = empty_parts();
        parts.headers.insert(
            "authorization",
            "".parse().unwrap(),
        );
        assert_eq!(
            validate_auth(&config, &parts),
            Err(StatusCode::UNAUTHORIZED)
        );
    }

    #[test]
    fn test_local_only_ignores_token() {
        let config = AuthConfig {
            mode: AuthMode::LocalOnly,
            token: Some("should_not_matter".to_string()),
        };
        let parts = empty_parts();
        assert!(validate_auth(&config, &parts).is_ok());
    }

    #[test]
    fn test_token_with_special_chars() {
        let config = AuthConfig {
            mode: AuthMode::Token,
            token: Some("tok-en_123!@#".to_string()),
        };
        let mut parts = empty_parts();
        parts.headers.insert(
            "authorization",
            "Bearer tok-en_123!@#".parse().unwrap(),
        );
        assert!(validate_auth(&config, &parts).is_ok());
    }
}
