use rand::distributions::Alphanumeric;
use rand::{Rng, thread_rng};
use uuid::Uuid;

/// Generate a random, URL-safe subdomain
/// Format: 12 lowercase alphanumeric characters
pub fn generate_subdomain() -> String {
    thread_rng()
        .sample_iter(&Alphanumeric)
        .take(12)
        .map(|c| c.to_ascii_lowercase())
        .map(char::from)
        .collect()
}

/// Generate a unique request identifier using UUID v4
pub fn generate_request_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_generate_subdomain_length() {
        let subdomain = generate_subdomain();
        assert_eq!(subdomain.len(), 12);
    }

    #[test]
    fn test_generate_subdomain_format() {
        let subdomain = generate_subdomain();

        // Should only contain lowercase alphanumeric characters
        assert!(subdomain.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(subdomain.chars().all(|c| !c.is_ascii_uppercase()));
    }

    #[test]
    fn test_generate_subdomain_uniqueness() {
        let mut subdomains = HashSet::new();

        // Generate 1000 subdomains and check they're all unique
        for _ in 0..1000 {
            let subdomain = generate_subdomain();
            assert!(
                subdomains.insert(subdomain),
                "Generated duplicate subdomain"
            );
        }
    }

    #[test]
    fn test_generate_request_id_format() {
        let request_id = generate_request_id();

        // UUID v4 format: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
        assert_eq!(request_id.len(), 36); // 32 hex chars + 4 hyphens
        assert_eq!(request_id.chars().filter(|&c| c == '-').count(), 4);

        // Should be a valid UUID
        assert!(Uuid::parse_str(&request_id).is_ok());
    }

    #[test]
    fn test_generate_request_id_uniqueness() {
        let mut ids = HashSet::new();

        // Generate 1000 request IDs and check they're all unique
        for _ in 0..1000 {
            let id = generate_request_id();
            assert!(ids.insert(id), "Generated duplicate request ID");
        }
    }

    #[test]
    fn test_generate_request_id_is_v4() {
        let request_id = generate_request_id();
        let uuid = Uuid::parse_str(&request_id).unwrap();

        // Verify it's a v4 UUID
        assert_eq!(uuid.get_version_num(), 4);
    }
}
