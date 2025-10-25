//! Content rewriting module for path-based routing
//!
//! This module handles rewriting absolute paths in response content to include
//! the tunnel ID prefix. This is necessary for path-based routing where the
//! tunnel ID is part of the URL path.
//!
//! For example, if a tunnel ID is "abc123" and the local service returns HTML
//! with `href="/api/users"`, it needs to be rewritten to `href="/abc123/api/users"`
//! so that the browser sends requests to the correct tunnel path.

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::{Captures, Regex};
use tracing::{debug, warn};

/// Strategy for rewriting content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteStrategy {
    /// No rewriting (pass through unchanged)
    None,
    /// HTML: inject <base> tag only
    BaseTag,
    /// HTML: rewrite all absolute paths
    FullRewrite,
}

impl Default for RewriteStrategy {
    fn default() -> Self {
        RewriteStrategy::FullRewrite
    }
}

/// Check if content type should be rewritten
pub fn should_rewrite_content(content_type: &str) -> bool {
    let content_type_lower = content_type.to_lowercase();
    matches!(
        content_type_lower.split(';').next().unwrap_or("").trim(),
        "text/html" | "text/css" | "application/javascript" | "text/javascript" | "application/json"
    )
}

/// Main entry point for content rewriting
pub fn rewrite_response_content(
    body: &str,
    content_type: &str,
    tunnel_id: &str,
    strategy: RewriteStrategy,
) -> Result<(String, bool)> {
    if !should_rewrite_content(content_type) {
        return Ok((body.to_string(), false));
    }

    let prefix = format!("/{}", tunnel_id);
    let content_type_lower = content_type.to_lowercase();
    let mime_type = content_type_lower.split(';').next().unwrap_or("").trim();

    let result = match (mime_type, strategy) {
        (_, RewriteStrategy::None) => {
            debug!("Content rewriting disabled by strategy");
            return Ok((body.to_string(), false));
        }
        ("text/html", RewriteStrategy::BaseTag) => inject_base_tag(body, &prefix),
        ("text/html", RewriteStrategy::FullRewrite) => rewrite_html(body, &prefix),
        ("text/css", _) => rewrite_css(body, &prefix),
        ("application/javascript" | "text/javascript", _) => {
            // JavaScript rewriting is complex and risky, skip for now
            debug!("Skipping JavaScript rewriting (not implemented)");
            return Ok((body.to_string(), false));
        }
        ("application/json", _) => rewrite_json(body, &prefix),
        _ => {
            return Ok((body.to_string(), false));
        }
    };

    let rewritten = result?;
    let was_rewritten = rewritten != body;

    if was_rewritten {
        debug!(
            "Rewrote {} content: {} bytes -> {} bytes",
            mime_type,
            body.len(),
            rewritten.len()
        );
    }

    Ok((rewritten, was_rewritten))
}

// Regex patterns (compiled once, reused many times)
static HTML_HREF_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"href="(/[^"]*)""#).expect("Invalid regex"));
static HTML_SRC_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"src="(/[^"]*)""#).expect("Invalid regex"));
static HTML_ACTION_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"action="(/[^"]*)""#).expect("Invalid regex"));
static HTML_DATA_URL_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"(href|src)="(https?://|data:|//|#)"#).expect("Invalid regex"));

// Match url() with various quote styles
static CSS_URL_SINGLE_QUOTE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"url\('(/[^']+)'\)"#).expect("Invalid regex"));
static CSS_URL_DOUBLE_QUOTE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"url\("(/[^"]+)"\)"#).expect("Invalid regex"));
static CSS_URL_NO_QUOTE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"url\((/[^)]+)\)"#).expect("Invalid regex"));

static JSON_PATH_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#""(/[a-zA-Z0-9/_-]+)""#).expect("Invalid regex"));

/// Inject <base> tag into HTML to set base path
/// This is a simpler approach that works for many HTML pages
fn inject_base_tag(html: &str, prefix: &str) -> Result<String> {
    // Look for <head> tag (case-insensitive)
    let head_regex = Regex::new(r"(?i)<head[^>]*>")?;

    if let Some(mat) = head_regex.find(html) {
        let insert_pos = mat.end();
        let base_tag = format!(r#"<base href="{}/""#, prefix);
        let mut result = html.to_string();
        result.insert_str(insert_pos, &base_tag);
        return Ok(result);
    }

    // If no <head> tag found, try to inject after <html>
    let html_regex = Regex::new(r"(?i)<html[^>]*>")?;
    if let Some(mat) = html_regex.find(html) {
        let insert_pos = mat.end();
        let base_tag = format!(r#"<head><base href="{}/""></head>"#, prefix);
        let mut result = html.to_string();
        result.insert_str(insert_pos, &base_tag);
        return Ok(result);
    }

    warn!("Could not find <head> or <html> tag for base tag injection");
    Ok(html.to_string())
}

/// Inject tunnel context script into HTML
/// This provides a global JavaScript variable that client code can use for dynamic URL construction
fn inject_tunnel_context(html: &str, tunnel_id: &str) -> Result<String> {
    let head_regex = Regex::new(r"(?i)<head[^>]*>")?;

    // Script that provides tunnel context to client-side JavaScript
    let context_script = format!(
        r#"<script>
// HTTP Tunnel Context - provides tunnel ID for dynamic URL construction
window.__TUNNEL_CONTEXT__ = {{
    tunnelId: '{}',
    basePath: '{}',
    // Helper function to construct URLs with tunnel prefix
    url: function(path) {{
        if (!path) return this.basePath;
        // Remove leading slash if present
        const cleanPath = path.startsWith('/') ? path.substring(1) : path;
        return this.basePath + '/' + cleanPath;
    }},
    // Get the full base URL including tunnel prefix
    getBaseUrl: function() {{
        return window.location.origin + this.basePath;
    }}
}};
// Also set base path as a simple variable for backwards compatibility
window.__TUNNEL_BASE_PATH__ = '{}';
</script>"#,
        tunnel_id, tunnel_id, tunnel_id
    );

    if let Some(mat) = head_regex.find(html) {
        let insert_pos = mat.end();
        let mut result = html.to_string();
        result.insert_str(insert_pos, &context_script);
        return Ok(result);
    }

    // If no <head> tag, try after <html>
    let html_regex = Regex::new(r"(?i)<html[^>]*>")?;
    if let Some(mat) = html_regex.find(html) {
        let insert_pos = mat.end();
        let script_with_head = format!("<head>{}</head>", context_script);
        let mut result = html.to_string();
        result.insert_str(insert_pos, &script_with_head);
        return Ok(result);
    }

    // If no structure found, prepend to document
    Ok(format!("{}{}", context_script, html))
}

/// Rewrite absolute paths in HTML attributes and inline JavaScript
fn rewrite_html(body: &str, prefix: &str) -> Result<String> {
    // Helper function to check if path should be rewritten
    let should_rewrite_path = |path: &str| -> bool {
        // Don't rewrite if:
        // - Already prefixed
        // - External URL (http://, https://)
        // - Protocol-relative URL (//)
        // - Data URL (data:)
        // - Anchor only (#)
        // - Empty
        if path.is_empty() || path.starts_with('#') {
            return false;
        }
        if path.starts_with("http://")
            || path.starts_with("https://")
            || path.starts_with("//")
            || path.starts_with("data:")
        {
            return false;
        }
        // Check if already prefixed
        if path.starts_with(&format!("{}/", prefix)) || path == prefix {
            return false;
        }
        true
    };

    // Rewrite href attributes
    let result = HTML_HREF_REGEX.replace_all(body, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite_path(path) {
            format!(r#"href="{}{}""#, prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Rewrite src attributes
    let result = HTML_SRC_REGEX.replace_all(&result, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite_path(path) {
            format!(r#"src="{}{}""#, prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Rewrite action attributes
    let result = HTML_ACTION_REGEX.replace_all(&result, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite_path(path) {
            format!(r#"action="{}{}""#, prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Rewrite JavaScript string literals (for inline scripts)
    // This is conservative and only rewrites obvious patterns
    let result = rewrite_inline_javascript(&result, prefix)?;

    // Inject tunnel context for dynamic JavaScript URL construction
    // This enables client-side code to build URLs correctly
    let tunnel_id = prefix.trim_start_matches('/');
    let result = inject_tunnel_context(&result, tunnel_id)?;

    Ok(result)
}

/// Rewrite JavaScript string literals in inline scripts
/// This handles common patterns like: url: '/api/path', fetch('/api/path'), etc.
fn rewrite_inline_javascript(html: &str, prefix: &str) -> Result<String> {
    // Match JavaScript string literals with absolute paths
    // Patterns: 'string', "string" with absolute paths
    let js_single_quote = Regex::new(r#"'(/[a-zA-Z0-9/_\-\.]+)'"#)?;
    let js_double_quote = Regex::new(r#""(/[a-zA-Z0-9/_\-\.]+)""#)?;

    let should_rewrite_js_path = |path: &str| -> bool {
        // Only rewrite if it looks like an API path or common web paths
        // Don't rewrite very short paths or paths that might be variable names
        if path.len() < 2 {
            return false;
        }
        // Check if already prefixed
        if path.starts_with(&format!("{}/", prefix)) || path == prefix {
            return false;
        }
        // Only rewrite paths that look like web resources
        path.starts_with("/api")
            || path.starts_with("/docs")
            || path.starts_with("/openapi")
            || path.starts_with("/swagger")
            || path.starts_with("/v1")
            || path.starts_with("/v2")
            || path.starts_with("/v3")
            || path.ends_with(".json")
            || path.ends_with(".yaml")
            || path.ends_with(".yml")
    };

    // Rewrite single-quoted strings
    let result = js_single_quote.replace_all(html, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite_js_path(path) {
            format!("'{}{}'", prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Rewrite double-quoted strings
    let result = js_double_quote.replace_all(&result, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite_js_path(path) {
            format!("\"{}{}\"", prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    Ok(result.into_owned())
}

/// Rewrite url() references in CSS
fn rewrite_css(body: &str, prefix: &str) -> Result<String> {
    let should_rewrite = |path: &str| -> bool {
        !path.starts_with("http://")
            && !path.starts_with("https://")
            && !path.starts_with("//")
            && !path.starts_with("data:")
            && !path.starts_with(&format!("{}/", prefix))
    };

    // Process single quotes
    let result = CSS_URL_SINGLE_QUOTE.replace_all(body, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite(path) {
            format!("url('{}{}')", prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Process double quotes
    let result = CSS_URL_DOUBLE_QUOTE.replace_all(&result, |caps: &Captures| {
        let path = &caps[1];
        if should_rewrite(path) {
            format!("url(\"{}{}\")", prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    // Process no quotes (must be last to avoid matching already-processed URLs)
    let result = CSS_URL_NO_QUOTE.replace_all(&result, |caps: &Captures| {
        let path = caps[1].trim();
        // Skip if it has quotes (already processed) or is external
        if path.starts_with('\'') || path.starts_with('"') || !should_rewrite(path) {
            return caps[0].to_string();
        }
        format!("url({}{})", prefix, path)
    });

    Ok(result.into_owned())
}

/// Rewrite absolute paths in JSON content
/// This is conservative and only rewrites obvious path-like strings
/// Also handles OpenAPI spec's servers field
fn rewrite_json(body: &str, prefix: &str) -> Result<String> {
    // First, handle OpenAPI servers field specially
    // "servers": [{"url": "/api"}] or "servers": [{"url": "https://example.com"}]
    let servers_regex = Regex::new(r#""servers"\s*:\s*\[\s*\{\s*"url"\s*:\s*"([^"]*)""#)?;

    let result = servers_regex.replace_all(body, |caps: &Captures| {
        let url = &caps[1];

        // If it's a relative path (starts with /), rewrite it
        if url.starts_with('/') && !url.starts_with(&format!("{}/", prefix)) {
            format!(r#""servers": [{{"url": "{}{}""#, prefix, url)
        } else if url.starts_with("http://") || url.starts_with("https://") {
            // It's a full URL - don't rewrite
            caps[0].to_string()
        } else {
            caps[0].to_string()
        }
    });

    // Then handle general JSON path rewriting
    let result = JSON_PATH_REGEX.replace_all(&result, |caps: &Captures| {
        let path = &caps[1];

        // Don't rewrite if:
        // - Already prefixed
        // - Looks like a URL scheme (http:, https:, etc.)
        // - Too short to be a meaningful path
        if path.len() < 2 {
            return caps[0].to_string();
        }
        if path.starts_with(&format!("{}/", prefix)) || path == prefix {
            return caps[0].to_string();
        }
        // Check if it looks like a URL scheme
        if path.contains("://") {
            return caps[0].to_string();
        }

        // Only rewrite if it looks like an API path (starts with /api, /v1, etc.)
        // or is in a known OpenAPI field
        let path_lower = path.to_lowercase();
        if path_lower.starts_with("/api")
            || path_lower.starts_with("/v1")
            || path_lower.starts_with("/v2")
            || path_lower.starts_with("/v3")
            || path_lower.starts_with("/docs")
            || path_lower.starts_with("/openapi")
            || path_lower.starts_with("/swagger")
            || path_lower.starts_with("/todos") // Common API path
        {
            format!(r#""{}{}""#, prefix, path)
        } else {
            caps[0].to_string()
        }
    });

    Ok(result.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_rewrite_content() {
        assert!(should_rewrite_content("text/html"));
        assert!(should_rewrite_content("text/html; charset=utf-8"));
        assert!(should_rewrite_content("text/css"));
        assert!(should_rewrite_content("application/json"));
        assert!(should_rewrite_content("application/javascript"));
        assert!(should_rewrite_content("text/javascript"));

        assert!(!should_rewrite_content("image/png"));
        assert!(!should_rewrite_content("application/octet-stream"));
        assert!(!should_rewrite_content("video/mp4"));
    }

    #[test]
    fn test_inject_base_tag() {
        let html = r#"<html><head><title>Test</title></head><body></body></html>"#;
        let result = inject_base_tag(html, "/abc123").unwrap();
        assert!(result.contains(r#"<base href="/abc123/""#));
        assert!(result.contains("<title>Test</title>"));
    }

    #[test]
    fn test_inject_base_tag_no_head() {
        let html = r#"<html><body>No head tag</body></html>"#;
        let result = inject_base_tag(html, "/abc123").unwrap();
        assert!(result.contains(r#"<base href="/abc123/""#));
    }

    #[test]
    fn test_rewrite_html_href() {
        let html = r#"<a href="/api/users">Users</a>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, r#"<a href="/abc123/api/users">Users</a>"#);
    }

    #[test]
    fn test_rewrite_html_src() {
        let html = r#"<img src="/images/logo.png">"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, r#"<img src="/abc123/images/logo.png">"#);
    }

    #[test]
    fn test_rewrite_html_action() {
        let html = r#"<form action="/submit">...</form>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, r#"<form action="/abc123/submit">...</form>"#);
    }

    #[test]
    fn test_dont_rewrite_external_url() {
        let html = r#"<a href="https://example.com/page">External</a>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_dont_rewrite_protocol_relative_url() {
        let html = r#"<script src="//cdn.example.com/script.js"></script>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_dont_rewrite_data_url() {
        let html = r#"<img src="data:image/png;base64,iVBOR...">"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_dont_rewrite_anchor() {
        let html = "<a href=\"#section\">Jump</a>";
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_dont_double_prefix() {
        let html = r#"<a href="/abc123/api/users">Already prefixed</a>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert_eq!(result, html);
    }

    #[test]
    fn test_rewrite_css_url() {
        let css = r#"background: url('/images/bg.png');"#;
        let result = rewrite_css(css, "/abc123").unwrap();
        assert_eq!(result, r#"background: url('/abc123/images/bg.png');"#);
    }

    #[test]
    fn test_rewrite_css_url_no_quotes() {
        let css = r#"background: url(/images/bg.png);"#;
        let result = rewrite_css(css, "/abc123").unwrap();
        assert_eq!(result, r#"background: url(/abc123/images/bg.png);"#);
    }

    #[test]
    fn test_rewrite_css_url_double_quotes() {
        let css = r#"background: url("/images/bg.png");"#;
        let result = rewrite_css(css, "/abc123").unwrap();
        assert_eq!(result, r#"background: url("/abc123/images/bg.png");"#);
    }

    #[test]
    fn test_dont_rewrite_css_external_url() {
        let css = r#"background: url('https://cdn.example.com/bg.png');"#;
        let result = rewrite_css(css, "/abc123").unwrap();
        assert_eq!(result, css);
    }

    #[test]
    fn test_rewrite_json_api_path() {
        let json = r#"{"url": "/api/users"}"#;
        let result = rewrite_json(json, "/abc123").unwrap();
        assert_eq!(result, r#"{"url": "/abc123/api/users"}"#);
    }

    #[test]
    fn test_rewrite_json_versioned_api() {
        let json = r#"{"baseUrl": "/v1/resources"}"#;
        let result = rewrite_json(json, "/abc123").unwrap();
        assert_eq!(result, r#"{"baseUrl": "/abc123/v1/resources"}"#);
    }

    #[test]
    fn test_dont_rewrite_json_arbitrary_path() {
        let json = r#"{"path": "/some/random/path"}"#;
        let result = rewrite_json(json, "/abc123").unwrap();
        // Should not rewrite paths that don't look like API paths
        assert_eq!(result, json);
    }

    #[test]
    fn test_dont_rewrite_json_url_scheme() {
        let json = r#"{"url": "https://example.com/api"}"#;
        let result = rewrite_json(json, "/abc123").unwrap();
        assert_eq!(result, json);
    }

    #[test]
    fn test_rewrite_response_content_html_full() {
        let html = r#"<html><head></head><body><a href="/api">API</a></body></html>"#;
        let (result, rewritten) =
            rewrite_response_content(html, "text/html", "abc123", RewriteStrategy::FullRewrite)
                .unwrap();
        assert!(rewritten);
        assert!(result.contains(r#"href="/abc123/api""#));
    }

    #[test]
    fn test_rewrite_response_content_html_base_tag() {
        let html = r#"<html><head></head><body><a href="/api">API</a></body></html>"#;
        let (result, rewritten) =
            rewrite_response_content(html, "text/html", "abc123", RewriteStrategy::BaseTag)
                .unwrap();
        assert!(rewritten);
        assert!(result.contains(r#"<base href="/abc123/""#));
    }

    #[test]
    fn test_rewrite_response_content_no_rewrite_strategy() {
        let html = r#"<a href="/api">API</a>"#;
        let (result, rewritten) =
            rewrite_response_content(html, "text/html", "abc123", RewriteStrategy::None).unwrap();
        assert!(!rewritten);
        assert_eq!(result, html);
    }

    #[test]
    fn test_rewrite_response_content_css() {
        let css = r#"div { background: url('/img/bg.png'); }"#;
        let (result, rewritten) =
            rewrite_response_content(css, "text/css", "abc123", RewriteStrategy::FullRewrite)
                .unwrap();
        assert!(rewritten);
        assert!(result.contains("/abc123/img/bg.png"));
    }

    #[test]
    fn test_rewrite_response_content_non_rewritable() {
        let content = "binary data";
        let (result, rewritten) = rewrite_response_content(
            content,
            "image/png",
            "abc123",
            RewriteStrategy::FullRewrite,
        )
        .unwrap();
        assert!(!rewritten);
        assert_eq!(result, content);
    }

    #[test]
    fn test_content_type_with_charset() {
        assert!(should_rewrite_content("text/html; charset=utf-8"));
        assert!(should_rewrite_content("application/json; charset=utf-8"));
        assert!(should_rewrite_content(
            "text/html; charset=utf-8; boundary=something"
        ));
    }

    #[test]
    fn test_rewrite_inline_javascript() {
        let html = "<script>\nconst ui = { url: '/openapi.json', path: '/api/v1' };\n</script>";
        let result = rewrite_html(html, "/abc123").unwrap();
        assert!(result.contains("'/abc123/openapi.json'"));
        assert!(result.contains("'/abc123/api/v1'"));
    }

    #[test]
    fn test_rewrite_swagger_config() {
        let html = r#"<script>
    const ui = SwaggerUIBundle({
        url: '/openapi.json',
        oauth2RedirectUrl: window.location.origin + '/docs/oauth2-redirect',
    })
    </script>"#;
        let result = rewrite_html(html, "/abc123").unwrap();
        assert!(result.contains("url: '/abc123/openapi.json'"));
        assert!(result.contains("+ '/abc123/docs/oauth2-redirect'"));
    }

    #[test]
    fn test_dont_rewrite_short_js_paths() {
        let html = "<script>const x = '/';</script>";
        let result = rewrite_html(html, "/abc123").unwrap();
        // Very short paths like '/' should not be rewritten
        assert!(result.contains("const x = '/';"));
    }

    #[test]
    fn test_inject_tunnel_context() {
        let html = "<html><head></head><body></body></html>";
        let result = rewrite_html(html, "/abc123").unwrap();
        // Should inject tunnel context script
        assert!(result.contains("window.__TUNNEL_CONTEXT__"));
        assert!(result.contains("tunnelId: 'abc123'"));
        assert!(result.contains("basePath: 'abc123'"));
        assert!(result.contains("window.__TUNNEL_BASE_PATH__"));
    }

    #[test]
    fn test_complex_html_document() {
        let html = "<!DOCTYPE html>\n<html>\n<head>\n    <title>Test Page</title>\n    <link rel=\"stylesheet\" href=\"/static/style.css\">\n    <script src=\"/static/app.js\"></script>\n</head>\n<body>\n    <a href=\"/api/users\">Users</a>\n    <a href=\"https://external.com\">External</a>\n    <a href=\"#section\">Anchor</a>\n    <img src=\"/images/logo.png\">\n    <form action=\"/submit\" method=\"POST\">\n        <input type=\"submit\">\n    </form>\n</body>\n</html>";

        let result = rewrite_html(html, "/abc123").unwrap();

        // Should rewrite local paths
        assert!(result.contains("href=\"/abc123/static/style.css\""));
        assert!(result.contains("src=\"/abc123/static/app.js\""));
        assert!(result.contains("href=\"/abc123/api/users\""));
        assert!(result.contains("src=\"/abc123/images/logo.png\""));
        assert!(result.contains("action=\"/abc123/submit\""));

        // Should NOT rewrite external URLs and anchors
        assert!(result.contains("href=\"https://external.com\""));
        assert!(result.contains("href=\"#section\""));
    }
}
