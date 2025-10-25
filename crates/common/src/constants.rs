/// Maximum connection lifetime before requiring reconnection (2 hours)
pub const MAX_CONNECTION_LIFETIME_SECS: i64 = 7200;

/// DynamoDB TTL buffer for cleanup of old connections (2 hours)
pub const CONNECTION_TTL_SECS: i64 = 7200;

/// Heartbeat interval to keep WebSocket connection alive (5 minutes)
pub const HEARTBEAT_INTERVAL_SECS: u64 = 300;

/// API Gateway WebSocket idle timeout (10 minutes)
pub const WEBSOCKET_IDLE_TIMEOUT_SECS: u64 = 600;

/// Request timeout waiting for response from agent (under API Gateway's 29s limit)
pub const REQUEST_TIMEOUT_SECS: u64 = 25;

/// Pending request TTL in DynamoDB (30 seconds)
pub const PENDING_REQUEST_TTL_SECS: i64 = 30;

/// Maximum request/response body size (2 MB per API Gateway limit)
pub const MAX_BODY_SIZE_BYTES: usize = 2 * 1024 * 1024;

/// Minimum delay for exponential backoff reconnection (1 second)
pub const RECONNECT_MIN_DELAY_MS: u64 = 1000;

/// Maximum delay for exponential backoff reconnection (60 seconds)
pub const RECONNECT_MAX_DELAY_MS: u64 = 60000;

/// Multiplier for exponential backoff reconnection
pub const RECONNECT_MULTIPLIER: f64 = 2.0;

/// Initial polling interval when waiting for response (50ms)
pub const POLL_INITIAL_INTERVAL_MS: u64 = 50;

/// Maximum polling interval (500ms)
pub const POLL_MAX_INTERVAL_MS: u64 = 500;

/// Polling backoff multiplier
pub const POLL_BACKOFF_MULTIPLIER: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants_values() {
        // These are compile-time checks for constant sanity
        // Even though they're optimized out, they document constraints
        const _: () = assert!(REQUEST_TIMEOUT_SECS < 29, "Must be under API Gateway limit");
        const _: () = assert!(HEARTBEAT_INTERVAL_SECS < WEBSOCKET_IDLE_TIMEOUT_SECS);
        const _: () = assert!(PENDING_REQUEST_TTL_SECS < MAX_CONNECTION_LIFETIME_SECS);
        const _: () = assert!(RECONNECT_MIN_DELAY_MS < RECONNECT_MAX_DELAY_MS);
        const _: () = assert!(RECONNECT_MULTIPLIER > 1.0);

        // Verify size limits
        assert_eq!(MAX_BODY_SIZE_BYTES, 2 * 1024 * 1024);
    }
}
