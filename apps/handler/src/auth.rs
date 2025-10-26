//! Authentication module for WebSocket connections
//!
//! This module provides JWT-based authentication for WebSocket connections.
//! Authentication can be enabled/disabled via the REQUIRE_AUTH environment variable.

use anyhow::{Context, Result, anyhow};
use aws_lambda_events::apigw::ApiGatewayWebsocketProxyRequest;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tracing::{debug, info, warn};

/// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // Subject (user ID)
    pub exp: usize,  // Expiration time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<usize>, // Issued at
}

/// JWKS (JSON Web Key Set) structure
#[derive(Debug, Clone, Deserialize)]
struct Jwks {
    keys: Vec<JwkKey>,
}

/// Individual JWK (JSON Web Key)
#[derive(Debug, Clone, Deserialize)]
struct JwkKey {
    kty: String, // Key type (RSA or oct)
    kid: String, // Key ID
    alg: String, // Algorithm (RS256, HS256, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<String>, // RSA modulus (base64url)
    #[serde(skip_serializing_if = "Option::is_none")]
    e: Option<String>, // RSA exponent (base64url)
    #[serde(skip_serializing_if = "Option::is_none")]
    k: Option<String>, // Symmetric key (base64url)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[allow(dead_code)]
    r#use: Option<String>, // Key use (sig, enc)
}

/// Cached JWKS loaded from file
static JWKS_CACHE: Lazy<RwLock<Option<Jwks>>> = Lazy::new(|| RwLock::new(None));

/// Load JWKS from environment variable or file (cached)
fn load_jwks() -> Result<Jwks> {
    // Check cache first
    {
        let cache = JWKS_CACHE.read().unwrap();
        if let Some(jwks) = cache.as_ref() {
            return Ok(jwks.clone());
        }
    }

    // Try loading from JWKS environment variable first
    let jwks_content = if let Ok(jwks_json) = std::env::var("JWKS") {
        debug!("Loading JWKS from JWKS environment variable");
        jwks_json
    } else {
        // Fallback to file
        let jwks_path =
            std::env::var("JWKS_PATH").unwrap_or_else(|_| "/var/task/jwks.json".to_string());

        debug!("Loading JWKS from file: {}", jwks_path);

        std::fs::read_to_string(&jwks_path)
            .with_context(|| format!("Failed to read JWKS file at {}", jwks_path))?
    };

    let jwks: Jwks = serde_json::from_str(&jwks_content).context("Failed to parse JWKS JSON")?;

    // Cache it
    {
        let mut cache = JWKS_CACHE.write().unwrap();
        *cache = Some(jwks.clone());
    }

    info!("JWKS loaded successfully with {} keys", jwks.keys.len());
    Ok(jwks)
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
        && let Some(token) = auth_header
            .to_str()
            .ok()
            .and_then(|s| s.strip_prefix("Bearer "))
    {
        debug!("Token extracted from Authorization header");
        return Some(token.to_string());
    }

    // Fallback to query parameter (less secure - gets logged)
    if let Some(token) = request.query_string_parameters.first("token") {
        warn!("Token extracted from query parameter (consider using Authorization header)");
        return Some(token.to_string());
    }

    None
}

/// Validate JWT token using JWKS file or JWT_SECRET
pub fn validate_token(token: &str) -> Result<Claims> {
    // Try JWKS first if available
    if let Ok(jwks) = load_jwks() {
        // Try each key in JWKS
        for key in &jwks.keys {
            debug!(
                "Trying key: {} (type: {}, alg: {})",
                key.kid, key.kty, key.alg
            );

            let result = match key.kty.as_str() {
                "RSA" => validate_with_rsa_key(token, key),
                "oct" => validate_with_symmetric_key(token, key),
                _ => {
                    warn!("Unsupported key type: {} (kid: {})", key.kty, key.kid);
                    continue;
                }
            };

            match result {
                Ok(claims) => {
                    info!("✅ Token validated with key: {} ({})", key.kid, key.alg);
                    return Ok(claims);
                }
                Err(e) => {
                    debug!("Key {} validation failed: {}", key.kid, e);
                }
            }
        }

        warn!(
            "Token validation failed with all {} JWKS keys",
            jwks.keys.len()
        );
        return Err(anyhow!("Token validation failed with all JWKS keys"));
    }

    // Fallback to JWT_SECRET environment variable
    let secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "default-secret-change-in-production".to_string());

    debug!("Using JWT_SECRET for validation (JWKS not available)");

    let validation = Validation::new(Algorithm::HS256);
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;

    Ok(token_data.claims)
}

/// Validate token with RSA public key
fn validate_with_rsa_key(token: &str, key: &JwkKey) -> Result<Claims> {
    let n = key
        .n
        .as_ref()
        .ok_or_else(|| anyhow!("Missing 'n' in RSA key"))?;
    let e = key
        .e
        .as_ref()
        .ok_or_else(|| anyhow!("Missing 'e' in RSA key"))?;

    let algorithm = match key.alg.as_str() {
        "RS256" => Algorithm::RS256,
        "RS384" => Algorithm::RS384,
        "RS512" => Algorithm::RS512,
        _ => return Err(anyhow!("Unsupported RSA algorithm: {}", key.alg)),
    };

    // DecodingKey::from_rsa_components expects base64url strings directly
    let decoding_key = DecodingKey::from_rsa_components(n, e)?;

    // Create validation without audience/issuer checks (accept any)
    let mut validation = Validation::new(algorithm);
    validation.validate_aud = false;
    validation.validate_exp = true;

    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;
    Ok(token_data.claims)
}

/// Validate token with symmetric (HMAC) key
fn validate_with_symmetric_key(token: &str, key: &JwkKey) -> Result<Claims> {
    let k = key
        .k
        .as_ref()
        .ok_or_else(|| anyhow!("Missing 'k' in symmetric key"))?;

    let key_bytes = base64_url_decode(k)?;

    let algorithm = match key.alg.as_str() {
        "HS256" => Algorithm::HS256,
        "HS384" => Algorithm::HS384,
        "HS512" => Algorithm::HS512,
        _ => return Err(anyhow!("Unsupported HMAC algorithm: {}", key.alg)),
    };

    let decoding_key = DecodingKey::from_secret(&key_bytes);
    let validation = Validation::new(algorithm);

    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;
    Ok(token_data.claims)
}

/// Decode base64url string (with or without padding)
fn base64_url_decode(s: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(s))
        .context("Failed to decode base64url")
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
