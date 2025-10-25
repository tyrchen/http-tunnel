use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Encode bytes to Base64 string
pub fn encode_body(body: &[u8]) -> String {
    STANDARD.encode(body)
}

/// Decode Base64 string to bytes
pub fn decode_body(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
    STANDARD.decode(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_empty() {
        let empty: &[u8] = &[];
        let encoded = encode_body(empty);
        assert_eq!(encoded, "");
    }

    #[test]
    fn test_encode_simple_text() {
        let text = b"Hello, World!";
        let encoded = encode_body(text);
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_encode_json() {
        let json = br#"{"name":"test","value":123}"#;
        let encoded = encode_body(json);
        assert_eq!(encoded, "eyJuYW1lIjoidGVzdCIsInZhbHVlIjoxMjN9");
    }

    #[test]
    fn test_encode_binary_data() {
        let binary = vec![0x00, 0x01, 0x02, 0xFF, 0xFE];
        let encoded = encode_body(&binary);
        assert_eq!(encoded, "AAEC//4=");
    }

    #[test]
    fn test_decode_empty() {
        let decoded = decode_body("").unwrap();
        assert_eq!(decoded, Vec::<u8>::new());
    }

    #[test]
    fn test_decode_simple_text() {
        let decoded = decode_body("SGVsbG8sIFdvcmxkIQ==").unwrap();
        assert_eq!(decoded, b"Hello, World!");
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_decode_json() {
        let decoded = decode_body("eyJuYW1lIjoidGVzdCIsInZhbHVlIjoxMjN9").unwrap();
        assert_eq!(decoded, br#"{"name":"test","value":123}"#);
    }

    #[test]
    fn test_decode_binary_data() {
        let decoded = decode_body("AAEC//4=").unwrap();
        assert_eq!(decoded, vec![0x00, 0x01, 0x02, 0xFF, 0xFE]);
    }

    #[test]
    fn test_roundtrip_text() {
        let original = b"This is a test message with special chars: \n\t\r!@#$%^&*()";
        let encoded = encode_body(original);
        let decoded = decode_body(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_roundtrip_binary() {
        let original: Vec<u8> = (0..=255).collect();
        let encoded = encode_body(&original);
        let decoded = decode_body(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_roundtrip_large_data() {
        let original = vec![0xAB; 10000]; // 10KB of data
        let encoded = encode_body(&original);
        let decoded = decode_body(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_invalid_base64() {
        let result = decode_body("This is not valid base64!!!");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_invalid_padding() {
        let result = decode_body("SGVsbG8"); // Missing padding
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_utf8_text() {
        let utf8_text = "Hello 世界 🌍".as_bytes();
        let encoded = encode_body(utf8_text);
        let decoded = decode_body(&encoded).unwrap();
        assert_eq!(decoded, utf8_text);
        assert_eq!(String::from_utf8(decoded).unwrap(), "Hello 世界 🌍");
    }
}
