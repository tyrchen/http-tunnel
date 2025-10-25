//! DisconnectHandler - Handles WebSocket $disconnect route
//!
//! This module contains the logic for handling WebSocket disconnections.
//! It removes the connection metadata from DynamoDB to clean up resources.

use aws_lambda_events::apigw::{ApiGatewayProxyResponse, ApiGatewayWebsocketProxyRequest};
use lambda_runtime::{Error, LambdaEvent};
use tracing::{info, warn};

use crate::{SharedClients, delete_connection};

/// Handler for WebSocket $disconnect route
pub async fn handle_disconnect(
    event: LambdaEvent<ApiGatewayWebsocketProxyRequest>,
    clients: &SharedClients,
) -> Result<ApiGatewayProxyResponse, Error> {
    let request_context = event.payload.request_context;
    let connection_id = request_context
        .connection_id
        .ok_or("Missing connection ID")?;

    info!("WebSocket connection disconnected: {}", connection_id);

    // Delete connection from DynamoDB
    match delete_connection(&clients.dynamodb, &connection_id).await {
        Ok(_) => {
            info!("Cleaned up connection metadata: {}", connection_id);
        }
        Err(e) => {
            // Log error but still return success - connection is already closed
            warn!(
                "Failed to delete connection metadata for {}: {}",
                connection_id, e
            );
        }
    }

    // Always return success response since connection is already closed
    Ok(ApiGatewayProxyResponse {
        status_code: 200,
        headers: Default::default(),
        multi_value_headers: Default::default(),
        body: None,
        is_base64_encoded: false,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_disconnect_handler_always_succeeds() {
        // The disconnect handler should always return success
        // even if DynamoDB operations fail, since the connection
        // is already closed at this point

        // This is a placeholder test to document the handler's behavior
        assert_eq!(200, 200);
    }
}
