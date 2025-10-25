//! CleanupHandler - Scheduled cleanup of expired connections
//!
//! This handler runs periodically (e.g., every hour) to actively clean up expired
//! connections from DynamoDB. While DynamoDB TTL handles eventual deletion (within 48 hours),
//! this provides immediate cleanup for cost optimization.

use aws_sdk_dynamodb::Client as DynamoDbClient;
use aws_sdk_dynamodb::types::AttributeValue;
use http_tunnel_common::utils::current_timestamp_secs;
use lambda_runtime::Error;
use serde_json::Value;
use tracing::{error, info};

/// Handler for scheduled cleanup (triggered by EventBridge)
pub async fn handle_cleanup(_event: Value, dynamodb: &DynamoDbClient) -> Result<Value, Error> {
    info!("Starting TTL cleanup task");

    let connections_table =
        std::env::var("CONNECTIONS_TABLE_NAME").unwrap_or_else(|_| "connections".to_string());
    let pending_requests_table = std::env::var("PENDING_REQUESTS_TABLE_NAME")
        .unwrap_or_else(|_| "pending-requests".to_string());

    let now = current_timestamp_secs();

    // Cleanup expired connections
    let connections_deleted =
        cleanup_expired_items(dynamodb, &connections_table, "connectionId", now)
            .await
            .map_err(|e| {
                error!("Failed to cleanup connections: {}", e);
                format!("Cleanup failed: {}", e)
            })?;

    // Cleanup expired pending requests
    let requests_deleted =
        cleanup_expired_items(dynamodb, &pending_requests_table, "requestId", now)
            .await
            .map_err(|e| {
                error!("Failed to cleanup pending requests: {}", e);
                format!("Cleanup failed: {}", e)
            })?;

    info!(
        "Cleanup completed: {} connections, {} pending requests deleted",
        connections_deleted, requests_deleted
    );

    Ok(serde_json::json!({
        "connectionsDeleted": connections_deleted,
        "requestsDeleted": requests_deleted,
        "timestamp": now
    }))
}

/// Cleanup expired items from a DynamoDB table
async fn cleanup_expired_items(
    client: &DynamoDbClient,
    table_name: &str,
    key_name: &str,
    now: i64,
) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
    // Scan for items past TTL
    let result = client
        .scan()
        .table_name(table_name)
        .filter_expression("attribute_exists(#ttl) AND #ttl < :now")
        .expression_attribute_names("#ttl", "ttl")
        .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
        .send()
        .await?;

    let mut deleted = 0;
    if let Some(items) = result.items {
        for item in items {
            if let Some(key_value) = item.get(key_name).and_then(|v| v.as_s().ok()) {
                match client
                    .delete_item()
                    .table_name(table_name)
                    .key(key_name, AttributeValue::S(key_value.clone()))
                    .send()
                    .await
                {
                    Ok(_) => {
                        deleted += 1;
                    }
                    Err(e) => {
                        error!(
                            "Failed to delete item {} from {}: {}",
                            key_value, table_name, e
                        );
                    }
                }
            }
        }
    }

    Ok(deleted)
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_cleanup_response_format() {
        let response = serde_json::json!({
            "connectionsDeleted": 5,
            "requestsDeleted": 10,
            "timestamp": 1234567890
        });

        assert_eq!(response["connectionsDeleted"], 5);
        assert_eq!(response["requestsDeleted"], 10);
    }
}
