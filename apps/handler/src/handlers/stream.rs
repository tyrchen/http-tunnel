//! DynamoDB Stream Handler - Processes stream events for event-driven responses
//!
//! This module handles DynamoDB Stream events from the pending_requests table.
//! When a request status changes to "completed", it publishes an event to EventBridge
//! to notify the waiting handler.

use aws_lambda_events::event::dynamodb::Event as DynamoDbStreamEvent;
use aws_lambda_events::event::dynamodb::EventRecord;
use lambda_runtime::{Error, LambdaEvent};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

use crate::SharedClients;

/// Minimal struct to deserialize pending request from DynamoDB Stream
#[derive(Debug, Clone, Deserialize, Serialize)]
struct StreamPendingRequest {
    #[serde(rename = "requestId")]
    request_id: String,
    status: String,
    #[serde(rename = "responseData")]
    response_data: Option<String>,
}

/// Handler for DynamoDB Stream events
pub async fn handle_stream(
    event: LambdaEvent<DynamoDbStreamEvent>,
    clients: &SharedClients,
) -> Result<(), Error> {
    let event_bus_name = std::env::var("EVENT_BUS_NAME")
        .unwrap_or_else(|_| format!("http-tunnel-events-dev"));

    let mut notifications_sent = 0;
    let mut notifications_skipped = 0;

    for record in &event.payload.records {
        // Try to deserialize new image using serde_dynamo
        match serde_dynamo::from_item::<_, StreamPendingRequest>(record.change.new_image.clone()) {
            Ok(pending_req) if pending_req.status == "completed" => {
                // Check if this is a new completion (not already completed)
                if is_status_change_to_completed(record) {
                    match publish_response_event(clients, &event_bus_name, &pending_req).await {
                        Ok(()) => {
                            info!(
                                request_id = %pending_req.request_id,
                                "Published response ready event to EventBridge"
                            );
                            notifications_sent += 1;
                        }
                        Err(e) => {
                            error!("Failed to publish event for {}: {}", pending_req.request_id, e);
                        }
                    }
                } else {
                    notifications_skipped += 1;
                }
            }
            Ok(_) => {
                // Status is not completed, skip
                notifications_skipped += 1;
            }
            Err(e) => {
                error!("Failed to deserialize stream record: {}", e);
                notifications_skipped += 1;
            }
        }
    }

    debug!(
        records_processed = event.payload.records.len(),
        notifications_sent = notifications_sent,
        notifications_skipped = notifications_skipped,
        "DynamoDB stream batch processed"
    );

    Ok(())
}

/// Check if status changed to completed (for MODIFY events)
fn is_status_change_to_completed(record: &EventRecord) -> bool {
    // INSERT events are always new
    match record.event_name.as_str() {
        "INSERT" => true,
        "MODIFY" => {
            // Check old status was not "completed"
            match serde_dynamo::from_item::<_, StreamPendingRequest>(record.change.old_image.clone()) {
                Ok(old_req) => old_req.status != "completed",
                Err(_) => true, // If we can't parse old image, assume it's new
            }
        }
        _ => false, // Unknown event type, skip
    }
}

/// Publish response ready event to EventBridge
async fn publish_response_event(
    clients: &SharedClients,
    event_bus_name: &str,
    pending_req: &StreamPendingRequest,
) -> Result<(), Error> {
    let response_data = pending_req
        .response_data
        .as_ref()
        .ok_or("Missing response_data in completed request")?;

    let detail = serde_json::json!({
        "requestId": pending_req.request_id,
        "responseData": response_data,
        "timestamp": http_tunnel_common::current_timestamp_millis(),
    });

    let entry = aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
        .source("http-tunnel.response")
        .detail_type("HttpResponseReady")
        .detail(detail.to_string())
        .event_bus_name(event_bus_name)
        .build();

    clients
        .eventbridge
        .put_events()
        .entries(entry)
        .send()
        .await
        .map_err(|e| format!("Failed to publish event to EventBridge: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lambda_events::event::dynamodb::StreamRecord;
    use serde_dynamo::AttributeValue;
    use std::collections::HashMap;

    #[test]
    fn test_is_status_change_insert() {
        let record = EventRecord {
            event_name: "INSERT".to_string(),
            change: StreamRecord::default(),
            ..Default::default()
        };

        assert!(is_status_change_to_completed(&record));
    }

    #[test]
    fn test_is_status_change_modify_from_pending() {
        let mut old_image = HashMap::new();
        old_image.insert("status".to_string(), serde_dynamo::AttributeValue::S("pending".to_string()));
        old_image.insert("requestId".to_string(), serde_dynamo::AttributeValue::S("req_123".to_string()));

        let record = EventRecord {
            event_name: "MODIFY".to_string(),
            change: StreamRecord {
                old_image: old_image.into(),
                ..Default::default()
            },
            ..Default::default()
        };

        assert!(is_status_change_to_completed(&record));
    }
}
