use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds
pub fn current_timestamp_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

/// Get current Unix timestamp in milliseconds
pub fn current_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_millis() as u64
}

/// Calculate TTL timestamp (current time + duration in seconds)
pub fn calculate_ttl(duration_secs: i64) -> i64 {
    current_timestamp_secs() + duration_secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_current_timestamp_secs() {
        let ts1 = current_timestamp_secs();
        assert!(ts1 > 0);

        // Sleep a bit and check timestamp increases
        thread::sleep(Duration::from_millis(100));
        let ts2 = current_timestamp_secs();
        assert!(ts2 >= ts1);
    }

    #[test]
    fn test_current_timestamp_millis() {
        let ts1 = current_timestamp_millis();
        assert!(ts1 > 0);

        // Sleep and verify milliseconds increased
        thread::sleep(Duration::from_millis(100));
        let ts2 = current_timestamp_millis();
        assert!(ts2 >= ts1 + 100);
    }

    #[test]
    fn test_timestamp_relationship() {
        let secs = current_timestamp_secs();
        let millis = current_timestamp_millis();

        // Milliseconds should be roughly 1000x seconds
        // Allow some margin for execution time
        let expected_millis = secs as u64 * 1000;
        let diff = millis.abs_diff(expected_millis);

        // Should be within 1 second (1000ms)
        assert!(diff < 1000, "Timestamp mismatch too large: {}", diff);
    }

    #[test]
    fn test_calculate_ttl_positive() {
        let now = current_timestamp_secs();
        let ttl = calculate_ttl(3600); // 1 hour
        assert_eq!(ttl, now + 3600);
    }

    #[test]
    fn test_calculate_ttl_zero() {
        let now = current_timestamp_secs();
        let ttl = calculate_ttl(0);
        assert_eq!(ttl, now);
    }

    #[test]
    fn test_calculate_ttl_various_durations() {
        let durations = vec![1, 60, 300, 3600, 7200, 86400];

        for duration in durations {
            let now = current_timestamp_secs();
            let ttl = calculate_ttl(duration);

            // TTL should be within reasonable range
            assert!(ttl >= now + duration - 1); // Allow 1 second tolerance
            assert!(ttl <= now + duration + 1);
        }
    }

    #[test]
    fn test_timestamp_ordering() {
        let mut timestamps = Vec::new();

        for _ in 0..5 {
            timestamps.push(current_timestamp_millis());
            thread::sleep(Duration::from_millis(10));
        }

        // Verify timestamps are monotonically increasing
        for i in 1..timestamps.len() {
            assert!(
                timestamps[i] >= timestamps[i - 1],
                "Timestamps not monotonic"
            );
        }
    }

    #[test]
    fn test_ttl_in_future() {
        let now = current_timestamp_secs();
        let ttl = calculate_ttl(100);

        assert!(ttl > now);
        assert_eq!(ttl - now, 100);
    }

    #[test]
    fn test_millis_precision() {
        let ts1 = current_timestamp_millis();
        thread::sleep(Duration::from_millis(50));
        let ts2 = current_timestamp_millis();

        let elapsed = ts2 - ts1;
        // Should be at least 50ms, but allow some overhead
        assert!(elapsed >= 50, "Expected at least 50ms, got {}ms", elapsed);
        assert!(elapsed < 200, "Expected less than 200ms, got {}ms", elapsed);
    }
}
