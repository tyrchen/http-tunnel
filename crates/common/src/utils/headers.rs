use http::{HeaderMap, HeaderName, HeaderValue};
use std::collections::HashMap;

/// Convert HTTP headers to our internal format
/// Supports multiple values per header name
pub fn headers_to_map(headers: &HeaderMap) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    for (name, value) in headers.iter() {
        let key = name.as_str().to_string();
        let val = value.to_str().unwrap_or("").to_string();

        map.entry(key).or_default().push(val);
    }

    map
}

/// Convert our internal header format to HTTP HeaderMap
pub fn map_to_headers(map: &HashMap<String, Vec<String>>) -> HeaderMap {
    let mut headers = HeaderMap::new();

    for (name, values) in map.iter() {
        if let Ok(header_name) = HeaderName::from_bytes(name.as_bytes()) {
            for value in values {
                if let Ok(header_value) = HeaderValue::from_str(value) {
                    headers.append(header_name.clone(), header_value);
                }
            }
        }
    }

    headers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headers_to_map_empty() {
        let headers = HeaderMap::new();
        let map = headers_to_map(&headers);
        assert!(map.is_empty());
    }

    #[test]
    fn test_headers_to_map_single() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        let map = headers_to_map(&headers);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("content-type").unwrap(), &vec!["application/json"]);
    }

    #[test]
    fn test_headers_to_map_multiple() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());
        headers.insert("authorization", "Bearer token123".parse().unwrap());
        headers.insert("x-custom-header", "custom-value".parse().unwrap());

        let map = headers_to_map(&headers);
        assert_eq!(map.len(), 3);
        assert_eq!(map.get("content-type").unwrap(), &vec!["application/json"]);
        assert_eq!(map.get("authorization").unwrap(), &vec!["Bearer token123"]);
        assert_eq!(map.get("x-custom-header").unwrap(), &vec!["custom-value"]);
    }

    #[test]
    fn test_headers_to_map_multiple_values() {
        let mut headers = HeaderMap::new();
        headers.insert("set-cookie", "session=abc".parse().unwrap());
        headers.append("set-cookie", "token=xyz".parse().unwrap());

        let map = headers_to_map(&headers);
        assert_eq!(map.len(), 1);

        let cookies = map.get("set-cookie").unwrap();
        assert_eq!(cookies.len(), 2);
        assert!(cookies.contains(&"session=abc".to_string()));
        assert!(cookies.contains(&"token=xyz".to_string()));
    }

    #[test]
    fn test_map_to_headers_empty() {
        let map: HashMap<String, Vec<String>> = HashMap::new();
        let headers = map_to_headers(&map);
        assert!(headers.is_empty());
    }

    #[test]
    fn test_map_to_headers_single() {
        let mut map = HashMap::new();
        map.insert(
            "content-type".to_string(),
            vec!["application/json".to_string()],
        );

        let headers = map_to_headers(&map);
        assert_eq!(headers.len(), 1);
        assert_eq!(headers.get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn test_map_to_headers_multiple() {
        let mut map = HashMap::new();
        map.insert("content-type".to_string(), vec!["text/plain".to_string()]);
        map.insert("host".to_string(), vec!["example.com".to_string()]);
        map.insert("user-agent".to_string(), vec!["test-agent".to_string()]);

        let headers = map_to_headers(&map);
        assert_eq!(headers.len(), 3);
        assert_eq!(headers.get("content-type").unwrap(), "text/plain");
        assert_eq!(headers.get("host").unwrap(), "example.com");
        assert_eq!(headers.get("user-agent").unwrap(), "test-agent");
    }

    #[test]
    fn test_map_to_headers_multiple_values() {
        let mut map = HashMap::new();
        map.insert(
            "set-cookie".to_string(),
            vec!["session=abc".to_string(), "token=xyz".to_string()],
        );

        let headers = map_to_headers(&map);

        // get_all returns an iterator over all values for a header
        let cookies: Vec<_> = headers
            .get_all("set-cookie")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();

        assert_eq!(cookies.len(), 2);
        assert!(cookies.contains(&"session=abc"));
        assert!(cookies.contains(&"token=xyz"));
    }

    #[test]
    fn test_roundtrip_conversion() {
        let mut original = HeaderMap::new();
        original.insert("content-type", "application/json".parse().unwrap());
        original.insert("authorization", "Bearer token".parse().unwrap());
        original.insert("x-request-id", "req-123".parse().unwrap());

        // Convert to map and back
        let map = headers_to_map(&original);
        let converted = map_to_headers(&map);

        // Verify all headers preserved
        assert_eq!(converted.len(), original.len());
        assert_eq!(
            converted.get("content-type").unwrap(),
            original.get("content-type").unwrap()
        );
        assert_eq!(
            converted.get("authorization").unwrap(),
            original.get("authorization").unwrap()
        );
        assert_eq!(
            converted.get("x-request-id").unwrap(),
            original.get("x-request-id").unwrap()
        );
    }

    #[test]
    fn test_roundtrip_with_multiple_values() {
        let mut original = HeaderMap::new();
        original.insert("accept", "text/html".parse().unwrap());
        original.append("accept", "application/json".parse().unwrap());
        original.insert("cookie", "session=abc".parse().unwrap());
        original.append("cookie", "token=xyz".parse().unwrap());

        let map = headers_to_map(&original);
        let converted = map_to_headers(&map);

        // Check accept header
        let accept_values: Vec<_> = converted
            .get_all("accept")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(accept_values.len(), 2);
        assert!(accept_values.contains(&"text/html"));
        assert!(accept_values.contains(&"application/json"));

        // Check cookie header
        let cookie_values: Vec<_> = converted
            .get_all("cookie")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(cookie_values.len(), 2);
        assert!(cookie_values.contains(&"session=abc"));
        assert!(cookie_values.contains(&"token=xyz"));
    }

    #[test]
    fn test_map_to_headers_invalid_header_name() {
        let mut map = HashMap::new();
        map.insert("valid-header".to_string(), vec!["value".to_string()]);
        map.insert("invalid header".to_string(), vec!["value".to_string()]); // Space is invalid

        let headers = map_to_headers(&map);

        // Only valid header should be included
        assert_eq!(headers.len(), 1);
        assert!(headers.get("valid-header").is_some());
        assert!(headers.get("invalid header").is_none());
    }

    #[test]
    fn test_headers_to_map_non_utf8_handling() {
        let mut headers = HeaderMap::new();
        headers.insert("content-type", "application/json".parse().unwrap());

        // Add a header with non-UTF8 value (though this is rare in practice)
        // HeaderValue allows non-UTF8 values
        let non_utf8_value = HeaderValue::from_bytes(&[0xFF, 0xFE]).unwrap();
        headers.insert("x-binary-header", non_utf8_value);

        let map = headers_to_map(&headers);

        // Non-UTF8 header should result in empty string
        assert_eq!(map.get("x-binary-header").unwrap(), &vec![""]);
        assert_eq!(map.get("content-type").unwrap(), &vec!["application/json"]);
    }
}
