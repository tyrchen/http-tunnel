//! Common utilities and types for the HTTP tunnel system
//!
//! This crate provides shared data structures, protocols, and utilities used by both
//! the forwarder (client agent) and handler (Lambda functions).

pub mod constants;
pub mod error;
pub mod models;
pub mod protocol;
pub mod utils;

// Re-export commonly used types for convenience
pub use error::{Result, TunnelError};
pub use models::{ClientInfo, ConnectionMetadata, PendingRequest};
pub use protocol::{ErrorCode, HttpRequest, HttpResponse, Message};
pub use utils::{
    calculate_ttl, current_timestamp_millis, current_timestamp_secs, decode_body, encode_body,
    generate_request_id, generate_subdomain, headers_to_map, map_to_headers,
};
