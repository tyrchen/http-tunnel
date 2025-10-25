//! Authentication module for WebSocket connections
//!
//! This module provides JWT-based authentication for WebSocket connections.
//! Authentication can be enabled/disabled via the REQUIRE_AUTH environment variable.

use anyhow::{Result, anyhow};
use aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequest;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

/// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // Subject (user ID)
    pub exp: usize,  // Expiration time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<usize>, // Issued at
}

/// Check if authentication is required based on environment variable
pub fn is_auth_required() -> bool {
    std::env::var("REQUIRE_AUTH")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase()
        == "true"
}

/// Extract token from WebSocket request
/// Checks (in order): Authorization header, query parameters
fn extract_token(request: &ApiGatewayWebsocketProxyRequest) -> Option<String> {
    // First try Authorization header (preferred - works with custom domains and not logged)
    if let Some(auth_header) = request
        .headers
        .get("authorization")
        .or_else(|| request.headers.get("Authorization"))
    {
        if let Some(token) = auth_header
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix("Bearer "))
        {
            debug!("Token extracted from Authorization header");
            return Some(token.to_string());
        }
    }

    // Fallback to query parameter (less secure - gets logged)
    if let Some(token) = request.query_string_parameters.first("token") {
        warn!("Token extracted from query parameter (consider using Authorization header)");
        return Some(token.to_string());
    }

    None
}

/// Validate JWT token
pub fn validate_token(token: &str) -> Result<Claims> {
    let secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "default-secret-change-in-production".to_string());

    let validation = Validation::new(Algorithm::HS256);

    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;

    Ok(token_data.claims)
}

/// Authenticate WebSocket connection request
///
/// Returns Ok(Some(claims)) if authentication is required and successful
/// Returns Ok(None) if authentication is not required
/// Returns Err if authentication is required but failed
pub fn authenticate_request(request: &ApiGatewayWebsocketProxyRequest) -> Result<Option<Claims>> {
    if !is_auth_required() {
        debug!("Authentication not required");
        return Ok(None);
    }

    info!("Authentication required, validating token");

    let token =
        extract_token(request).ok_or_else(|| anyhow!("No authentication token provided"))?;

    match validate_token(&token) {
        Ok(claims) => {
            info!("Token validated successfully for user: {}", claims.sub);
            Ok(Some(claims))
        }
        Err(e) => {
            warn!("Token validation failed: {}", e);
            Err(anyhow!("Invalid or expired token"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    #[test]
    fn test_create_and_validate_token() {
        let claims = Claims {
            sub: "user123".to_string(),
            exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
            iat: Some(chrono::Utc::now().timestamp() as usize),
        };

        let secret = "test-secret";
        unsafe { std::env::set_var("JWT_SECRET", secret) };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let validated = validate_token(&token).unwrap();
        assert_eq!(validated.sub, "user123");
    }

    #[test]
    fn test_expired_token() {
        let claims = Claims {
            sub: "user123".to_string(),
            exp: (chrono::Utc::now() - chrono::Duration::hours(1)).timestamp() as usize,
            iat: Some(chrono::Utc::now().timestamp() as usize),
        };

        let secret = "test-secret";
        unsafe { std::env::set_var("JWT_SECRET", secret) };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        assert!(validate_token(&token).is_err());
    }
}
