# HTTP Tunnel Improvements - Implementation Plan

**Document Version**: 1.0
**Date**: 2024-01-26
**Status**: Draft

## Overview

This document provides detailed implementation steps for the improvements outlined in `improvements-spec.md`. Each section includes technical approach, file changes, testing strategy, and rollout plan.

---

## Phase 1: Foundation (Weeks 1-6)

### 1.1 Error Handling Consistency

**Epic**: Standardize error handling across the codebase

**Story 1.1.1**: Create unified error types
- **Tasks**:
  1. Extend `crates/common/src/error.rs`:
     ```rust
     #[derive(Error, Debug)]
     pub enum TunnelError {
         // Existing variants...

         #[error("DynamoDB operation failed: {0}")]
         DynamoDbError(#[from] aws_sdk_dynamodb::Error),

         #[error("API Gateway operation failed: {0}")]
         ApiGatewayError(String),

         #[error("Rate limit exceeded: {0}")]
         RateLimitExceeded(String),

         #[error("Authentication failed: {0}")]
         AuthenticationError(String),

         #[error("Configuration error: {0}")]
         ConfigurationError(String),
     }

     impl TunnelError {
         pub fn error_code(&self) -> ErrorCode {
             match self {
                 Self::Timeout => ErrorCode::Timeout,
                 Self::ConnectionError(_) => ErrorCode::ConnectionError,
                 Self::RateLimitExceeded(_) => ErrorCode::RateLimitExceeded,
                 // ... map all variants
             }
         }

         pub fn http_status(&self) -> u16 {
             match self {
                 Self::RateLimitExceeded(_) => 429,
                 Self::AuthenticationError(_) => 401,
                 Self::InvalidMessage(_) => 400,
                 Self::Timeout => 504,
                 Self::ConnectionError(_) => 502,
                 _ => 500,
             }
         }
     }
     ```

  2. Add error context utilities:
     ```rust
     use anyhow::Context;

     pub trait ErrorContext<T> {
         fn with_tunnel_context(self, tunnel_id: &str) -> anyhow::Result<T>;
         fn with_request_context(self, request_id: &str) -> anyhow::Result<T>;
     }

     impl<T, E> ErrorContext<T> for Result<T, E>
     where
         E: std::error::Error + Send + Sync + 'static,
     {
         fn with_tunnel_context(self, tunnel_id: &str) -> anyhow::Result<T> {
             self.with_context(|| format!("tunnel_id={}", tunnel_id))
         }

         fn with_request_context(self, request_id: &str) -> anyhow::Result<T> {
             self.with_context(|| format!("request_id={}", request_id))
         }
     }
     ```

  3. Update `apps/handler/src/error_handling.rs`:
     - Add structured error response builder
     - Include error ID for tracking
     - Map internal errors to user-friendly messages

**Story 1.1.2**: Refactor handlers to use new error types
- **Files to update**:
  - `apps/handler/src/handlers/forwarding.rs`
  - `apps/handler/src/handlers/connect.rs`
  - `apps/handler/src/handlers/response.rs`
  - `apps/handler/src/handlers/disconnect.rs`

- **Pattern**:
  ```rust
  // Before
  lookup_connection_by_tunnel_id(client, tunnel_id)
      .await
      .map_err(|e| anyhow!("Failed to lookup: {}", e))?;

  // After
  lookup_connection_by_tunnel_id(client, tunnel_id)
      .await
      .with_tunnel_context(tunnel_id)
      .context("Failed to lookup connection")?;
  ```

**Story 1.1.3**: Add error middleware for Lambda responses
- Create `apps/handler/src/middleware/error.rs`:
  ```rust
  pub fn handle_error(error: anyhow::Error) -> ApiGatewayProxyResponse {
      let error_id = uuid::Uuid::new_v4();

      // Log full error with backtrace
      error!("Error {}: {:?}", error_id, error);

      // Extract or downcast to TunnelError
      let tunnel_error = error.downcast_ref::<TunnelError>()
          .cloned()
          .unwrap_or(TunnelError::InternalError(format!("{}", error)));

      // Build response
      ApiGatewayProxyResponse {
          status_code: tunnel_error.http_status() as i64,
          body: Some(Body::Text(json!({
              "error": tunnel_error.error_code(),
              "message": tunnel_error.to_string(),
              "error_id": error_id,
          }).to_string())),
          ..Default::default()
      }
  }
  ```

**Testing**:
- Unit tests for error conversion
- Integration tests for error responses
- Verify CloudWatch logs include context

**Success Criteria**:
- All handlers use consistent error types
- Error responses include error codes and IDs
- CloudWatch logs have full error context

---

### 1.2 Request/Response Correlation & Tracing

**Epic**: Add distributed tracing and correlation

**Story 1.2.1**: Add tracing instrumentation
- **Tasks**:
  1. Add to `apps/handler/Cargo.toml`:
     ```toml
     tracing-opentelemetry = "0.18"
     opentelemetry = { version = "0.18", features = ["rt-tokio"] }
     opentelemetry-aws = "0.6"  # AWS X-Ray exporter
     ```

  2. Initialize in `apps/handler/src/main.rs`:
     ```rust
     use opentelemetry::global;
     use opentelemetry_aws::XrayPropagator;
     use tracing_subscriber::layer::SubscriberExt;

     #[tokio::main]
     async fn main() -> Result<(), Error> {
         // X-Ray setup
         global::set_text_map_propagator(XrayPropagator::default());

         let tracer = opentelemetry_aws::new_pipeline()
             .with_service_name("http-tunnel-handler")
             .install()?;

         let telemetry_layer = tracing_opentelemetry::layer()
             .with_tracer(tracer);

         let subscriber = tracing_subscriber::fmt()
             .json()
             .with_max_level(tracing::Level::INFO)
             .finish()
             .with(telemetry_layer);

         tracing::subscriber::set_global_default(subscriber)?;

         // ... rest of main
     }
     ```

  3. Instrument handlers:
     ```rust
     use tracing::instrument;

     #[instrument(
         skip(event, clients),
         fields(
             tunnel_id,
             request_id = %uuid::Uuid::new_v4(),
             method = %event.payload.http_method,
             path = %event.payload.path.as_deref().unwrap_or("/"),
         )
     )]
     pub async fn handle_forwarding(
         event: LambdaEvent<ApiGatewayProxyRequest>,
         clients: &SharedClients,
     ) -> Result<ApiGatewayProxyResponse, Error> {
         // tracing::Span::current() contains all fields

         let routing_mode = detect_routing_mode(...)?;
         tracing::Span::current()
             .record("tunnel_id", routing_mode.tunnel_id());

         // ... handler logic
     }
     ```

**Story 1.2.2**: Add custom trace segments
- Create subsegments for:
  - DynamoDB operations
  - API Gateway Management calls
  - HTTP request forwarding
  - Response waiting

- **Example**:
  ```rust
  async fn lookup_connection_with_trace(
      client: &DynamoDbClient,
      tunnel_id: &str,
  ) -> Result<String> {
      let span = tracing::info_span!(
          "dynamodb.query",
          table = "connections",
          index = "tunnel-id-index",
          tunnel_id = %tunnel_id
      );

      async move {
          let result = lookup_connection_by_tunnel_id(client, tunnel_id).await?;
          tracing::info!("Connection found: {}", result);
          Ok(result)
      }
      .instrument(span)
      .await
  }
  ```

**Story 1.2.3**: Structured logging with context
- Update all log statements to include context:
  ```rust
  tracing::info!(
      tunnel_id = %tunnel_id,
      connection_id = %connection_id,
      "Forwarding request to WebSocket connection"
  );

  tracing::error!(
      error = %e,
      tunnel_id = %tunnel_id,
      "Failed to send request to connection"
  );
  ```

**Infrastructure Changes**:
- Update `infra/src/lambda.ts`:
  ```typescript
  const handler = new aws.lambda.Function("handler", {
      // ... existing config
      tracingConfig: {
          mode: "Active",  // Enable X-Ray
      },
      environment: {
          variables: {
              // ... existing vars
              RUST_LOG: "info,http_tunnel_handler=debug",
          },
      },
  });
  ```

**Testing**:
- Verify X-Ray traces appear in AWS console
- Check trace continuity across Lambda invocations
- Validate structured logs in CloudWatch Logs Insights

**Success Criteria**:
- End-to-end traces visible in X-Ray
- All logs include correlation IDs
- Latency breakdown visible per operation

---

### 1.3 Metrics & Alerting

**Epic**: Comprehensive monitoring and alerting

**Story 1.3.1**: Emit custom CloudWatch metrics
- **Tasks**:
  1. Create metrics helper in `apps/handler/src/lib.rs`:
     ```rust
     use std::sync::Arc;
     use aws_sdk_cloudwatch::Client as CloudWatchClient;
     use aws_sdk_cloudwatch::types::{Dimension, MetricDatum, StandardUnit};
     use tokio::sync::Mutex;
     use std::time::SystemTime;

     pub struct MetricsCollector {
         client: CloudWatchClient,
         namespace: String,
         buffer: Arc<Mutex<Vec<MetricDatum>>>,
     }

     impl MetricsCollector {
         pub fn new(client: CloudWatchClient, namespace: String) -> Self {
             Self {
                 client,
                 namespace,
                 buffer: Arc::new(Mutex::new(Vec::new())),
             }
         }

         pub async fn emit_count(
             &self,
             name: &str,
             value: f64,
             dimensions: Vec<(&str, &str)>,
         ) {
             self.emit_metric(name, value, StandardUnit::Count, dimensions).await;
         }

         pub async fn emit_latency(
             &self,
             name: &str,
             value_ms: f64,
             dimensions: Vec<(&str, &str)>,
         ) {
             self.emit_metric(name, value_ms, StandardUnit::Milliseconds, dimensions).await;
         }

         async fn emit_metric(
             &self,
             name: &str,
             value: f64,
             unit: StandardUnit,
             dimensions: Vec<(&str, &str)>,
         ) {
             let metric = MetricDatum::builder()
                 .metric_name(name)
                 .value(value)
                 .unit(unit)
                 .timestamp(SystemTime::now().into())
                 .set_dimensions(Some(
                     dimensions
                         .into_iter()
                         .map(|(k, v)| {
                             Dimension::builder()
                                 .name(k)
                                 .value(v)
                                 .build()
                         })
                         .collect(),
                 ))
                 .build();

             let mut buffer = self.buffer.lock().await;
             buffer.push(metric);

             // Flush if buffer is large
             if buffer.len() >= 20 {
                 self.flush_metrics(&mut buffer).await;
             }
         }

         async fn flush_metrics(&self, buffer: &mut Vec<MetricDatum>) {
             if buffer.is_empty() {
                 return;
             }

             if let Err(e) = self
                 .client
                 .put_metric_data()
                 .namespace(&self.namespace)
                 .set_metric_data(Some(buffer.drain(..).collect()))
                 .send()
                 .await
             {
                 tracing::error!("Failed to emit metrics: {}", e);
             }
         }
     }
     ```

  2. Add to `SharedClients`:
     ```rust
     pub struct SharedClients {
         pub dynamodb: DynamoDbClient,
         pub apigw_management: Option<ApiGatewayManagementClient>,
         pub eventbridge: EventBridgeClient,
         pub metrics: Arc<MetricsCollector>,  // Add this
     }
     ```

  3. Emit metrics in handlers:
     ```rust
     // Connection established
     clients.metrics.emit_count(
         "ConnectionEstablished",
         1.0,
         vec![("Environment", &env)],
     ).await;

     // Request forwarded
     clients.metrics.emit_count(
         "RequestForwarded",
         1.0,
         vec![
             ("TunnelId", tunnel_id),
             ("Method", &method),
         ],
     ).await;

     // Latency tracking
     let start = Instant::now();
     // ... operation
     clients.metrics.emit_latency(
         "RequestLatency",
         start.elapsed().as_millis() as f64,
         vec![("Operation", "Forwarding")],
     ).await;

     // Error tracking
     clients.metrics.emit_count(
         "ErrorOccurred",
         1.0,
         vec![
             ("ErrorType", "DynamoDbTimeout"),
             ("Handler", "Forwarding"),
         ],
     ).await;
     ```

**Story 1.3.2**: Create CloudWatch Dashboard
- **Infrastructure** (`infra/src/monitoring.ts`):
  ```typescript
  import * as aws from "@pulumi/aws";

  export function createMonitoringDashboard(
      lambdaFunction: aws.lambda.Function,
      connectionsTable: aws.dynamodb.Table,
      pendingRequestsTable: aws.dynamodb.Table,
  ) {
      const dashboard = new aws.cloudwatch.Dashboard("http-tunnel-dashboard", {
          dashboardName: "http-tunnel-metrics",
          dashboardBody: JSON.stringify({
              widgets: [
                  // Active tunnels
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["HttpTunnel", "ConnectionEstablished", { stat: "Sum", period: 300 }],
                              [".", "ConnectionClosed", { stat: "Sum", period: 300 }],
                          ],
                          title: "Active Connections",
                          region: "us-west-2",
                      },
                  },
                  // Request throughput
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["HttpTunnel", "RequestForwarded", { stat: "Sum", period: 60 }],
                          ],
                          title: "Requests per Minute",
                          region: "us-west-2",
                      },
                  },
                  // Latency percentiles
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["HttpTunnel", "RequestLatency", { stat: "p50", period: 300 }],
                              [".", ".", { stat: "p95", period: 300 }],
                              [".", ".", { stat: "p99", period: 300 }],
                          ],
                          title: "Request Latency (ms)",
                          region: "us-west-2",
                      },
                  },
                  // Error rate
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["HttpTunnel", "ErrorOccurred", { stat: "Sum", period: 300 }],
                          ],
                          title: "Errors",
                          region: "us-west-2",
                      },
                  },
                  // Lambda metrics
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["AWS/Lambda", "Invocations", { stat: "Sum" }],
                              [".", "Errors", { stat: "Sum" }],
                              [".", "Throttles", { stat: "Sum" }],
                              [".", "Duration", { stat: "Average" }],
                          ],
                          title: "Lambda Performance",
                          region: "us-west-2",
                      },
                  },
                  // DynamoDB metrics
                  {
                      type: "metric",
                      properties: {
                          metrics: [
                              ["AWS/DynamoDB", "ConsumedReadCapacityUnits",
                                  { stat: "Sum", dimensions: { TableName: connectionsTable.name } }],
                              [".", "ConsumedWriteCapacityUnits",
                                  { stat: "Sum", dimensions: { TableName: connectionsTable.name } }],
                          ],
                          title: "DynamoDB Capacity",
                          region: "us-west-2",
                      },
                  },
              ],
          }),
      });

      return dashboard;
  }
  ```

**Story 1.3.3**: Configure CloudWatch Alarms
- **Infrastructure** (`infra/src/alarms.ts`):
  ```typescript
  export function createAlarms(
      lambdaFunction: aws.lambda.Function,
      snsTopicArn: string,
  ) {
      // High error rate alarm
      new aws.cloudwatch.MetricAlarm("high-error-rate", {
          alarmName: "http-tunnel-high-error-rate",
          comparisonOperator: "GreaterThanThreshold",
          evaluationPeriods: 2,
          metricName: "ErrorOccurred",
          namespace: "HttpTunnel",
          period: 300,
          statistic: "Sum",
          threshold: 10,
          alarmDescription: "Alert when error count exceeds 10 in 5 minutes",
          alarmActions: [snsTopicArn],
          treatMissingData: "notBreaching",
      });

      // Lambda throttling alarm
      new aws.cloudwatch.MetricAlarm("lambda-throttling", {
          alarmName: "http-tunnel-lambda-throttling",
          comparisonOperator: "GreaterThanThreshold",
          evaluationPeriods: 1,
          metricName: "Throttles",
          namespace: "AWS/Lambda",
          dimensions: {
              FunctionName: lambdaFunction.name,
          },
          period: 60,
          statistic: "Sum",
          threshold: 5,
          alarmDescription: "Alert on Lambda throttling",
          alarmActions: [snsTopicArn],
      });

      // High latency alarm (P95 > 1000ms)
      new aws.cloudwatch.MetricAlarm("high-latency", {
          alarmName: "http-tunnel-high-latency",
          comparisonOperator: "GreaterThanThreshold",
          evaluationPeriods: 2,
          metricName: "RequestLatency",
          namespace: "HttpTunnel",
          period: 300,
          extendedStatistic: "p95",
          threshold: 1000,
          alarmDescription: "Alert when P95 latency exceeds 1 second",
          alarmActions: [snsTopicArn],
      });

      // DynamoDB throttling
      new aws.cloudwatch.MetricAlarm("dynamodb-throttling", {
          alarmName: "http-tunnel-dynamodb-throttled",
          comparisonOperator: "GreaterThanThreshold",
          evaluationPeriods: 1,
          metricName: "UserErrors",
          namespace: "AWS/DynamoDB",
          period: 60,
          statistic: "Sum",
          threshold: 10,
          alarmDescription: "Alert on DynamoDB throttling",
          alarmActions: [snsTopicArn],
      });
  }
  ```

**Testing**:
- Verify metrics appear in CloudWatch
- Trigger alarms with test scenarios
- Validate dashboard displays correctly

**Success Criteria**:
- All key metrics tracked
- Alarms configured and tested
- Dashboard provides system overview

---

### 1.4 Test Coverage Expansion

**Epic**: Achieve 70%+ test coverage

**Story 1.4.1**: Unit test infrastructure
- **Tasks**:
  1. Create test utilities in `tests/common/mod.rs`:
     ```rust
     use aws_sdk_dynamodb::Client as DynamoDbClient;
     use aws_sdk_dynamodb::config::{Builder, Region};

     pub async fn mock_dynamodb_client() -> DynamoDbClient {
         // For CI: use LocalStack or DynamoDB Local
         let endpoint_url = std::env::var("DYNAMODB_ENDPOINT")
             .unwrap_or_else(|_| "http://localhost:8000".to_string());

         let config = Builder::new()
             .region(Region::new("us-west-2"))
             .endpoint_url(endpoint_url)
             .build();

         DynamoDbClient::from_conf(config)
     }

     pub async fn create_test_table(client: &DynamoDbClient, table_name: &str) {
         // Create table schema for testing
     }

     pub fn mock_api_gateway_request(
         method: http::Method,
         path: &str,
     ) -> ApiGatewayProxyRequest {
         ApiGatewayProxyRequest {
             http_method: method,
             path: Some(path.to_string()),
             ..Default::default()
         }
     }
     ```

  2. Add test fixtures in `fixtures/`:
     - `sample_requests.json` - Various HTTP requests
     - `sample_responses.json` - Expected responses
     - `test_tunnels.json` - Test tunnel configurations

**Story 1.4.2**: Handler unit tests
- Create `apps/handler/src/handlers/tests/forwarding_tests.rs`:
  ```rust
  #[cfg(test)]
  mod forwarding_tests {
      use super::*;

      #[tokio::test]
      async fn test_subdomain_routing() {
          let event = mock_api_gateway_request(
              http::Method::GET,
              "/api/users",
          );
          // ... test implementation
      }

      #[tokio::test]
      async fn test_path_based_routing() {
          // ...
      }

      #[tokio::test]
      async fn test_rate_limit_exceeded() {
          // ...
      }

      #[tokio::test]
      async fn test_tunnel_not_found() {
          // ...
      }

      #[tokio::test]
      async fn test_request_timeout() {
          // ...
      }
  }
  ```

**Story 1.4.3**: Integration tests
- Create `tests/integration/test_e2e_flow.rs`:
  ```rust
  #[tokio::test]
  async fn test_full_tunnel_lifecycle() {
      // 1. Start forwarder
      // 2. Establish WebSocket connection
      // 3. Verify ConnectionEstablished message
      // 4. Send HTTP request via tunnel
      // 5. Verify request reaches local service
      // 6. Verify response returned correctly
      // 7. Close connection
      // 8. Verify cleanup
  }

  #[tokio::test]
  async fn test_reconnection_flow() {
      // 1. Establish connection
      // 2. Force disconnect
      // 3. Verify automatic reconnection
      // 4. Verify new tunnel ID assigned
      // 5. Verify requests work after reconnect
  }

  #[tokio::test]
  async fn test_concurrent_requests() {
      // Send 100 concurrent requests through tunnel
      // Verify all complete successfully
      // Check latency distribution
  }
  ```

**Story 1.4.4**: Load tests
- Create `tests/load/concurrent_connections.js` (k6):
  ```javascript
  import ws from 'k6/ws';
  import { check } from 'k6';

  export let options = {
      stages: [
          { duration: '30s', target: 10 },  // Ramp up to 10 connections
          { duration: '1m', target: 10 },   // Stay at 10
          { duration: '30s', target: 50 },  // Ramp to 50
          { duration: '2m', target: 50 },   // Stay at 50
          { duration: '30s', target: 0 },   // Ramp down
      ],
  };

  export default function () {
      const url = 'wss://your-endpoint.amazonaws.com';

      const res = ws.connect(url, (socket) => {
          socket.on('open', () => {
              console.log('Connected');
              socket.send(JSON.stringify({ type: 'ready' }));
          });

          socket.on('message', (data) => {
              const msg = JSON.parse(data);
              check(msg, {
                  'received connection_established': (m) => m.type === 'connection_established',
              });
          });

          socket.setTimeout(() => {
              socket.close();
          }, 60000);  // Keep connection for 1 minute
      });
  }
  ```

**CI/CD Integration**:
- Update `.github/workflows/ci.yml`:
  ```yaml
  name: CI

  on: [push, pull_request]

  jobs:
    test:
      runs-on: ubuntu-latest
      services:
        dynamodb:
          image: amazon/dynamodb-local
          ports:
            - 8000:8000

      steps:
        - uses: actions/checkout@v3

        - name: Setup Rust
          uses: actions-rs/toolchain@v1
          with:
            toolchain: stable
            override: true

        - name: Run tests
          run: cargo test --all
          env:
            DYNAMODB_ENDPOINT: http://localhost:8000

        - name: Generate coverage
          run: |
            cargo install cargo-tarpaulin
            cargo tarpaulin --out Xml --output-dir coverage

        - name: Upload coverage
          uses: codecov/codecov-action@v3
          with:
            files: ./coverage/cobertura.xml

    lint:
      runs-on: ubuntu-latest
      steps:
        - uses: actions/checkout@v3
        - uses: actions-rs/toolchain@v1
          with:
            toolchain: stable
            components: clippy, rustfmt

        - name: Run clippy
          run: cargo clippy --all-targets --all-features -- -D warnings

        - name: Check formatting
          run: cargo fmt -- --check

        - name: Security audit
          run: |
            cargo install cargo-deny
            cargo deny check
  ```

**Success Criteria**:
- Unit test coverage > 70%
- All handlers have tests
- Integration tests pass
- CI pipeline runs on every PR

---

### 1.5 Documentation Improvements

**Epic**: Comprehensive documentation

**Story 1.5.1**: Architecture documentation site
- **Tasks**:
  1. Setup mdBook:
     ```bash
     cargo install mdbook
     mdbook init docs/book
     ```

  2. Structure (`docs/book/src/SUMMARY.md`):
     ```markdown
     # Summary

     [Introduction](./introduction.md)

     # User Guide
     - [Getting Started](./user-guide/getting-started.md)
     - [Installation](./user-guide/installation.md)
     - [Configuration](./user-guide/configuration.md)
     - [Custom Domains](./user-guide/custom-domains.md)
     - [Authentication](./user-guide/authentication.md)

     # Architecture
     - [System Overview](./architecture/overview.md)
     - [Components](./architecture/components.md)
     - [Data Flow](./architecture/data-flow.md)
     - [WebSocket Protocol](./architecture/protocol.md)

     # Operations
     - [Deployment](./operations/deployment.md)
     - [Monitoring](./operations/monitoring.md)
     - [Troubleshooting](./operations/troubleshooting.md)
     - [Cost Optimization](./operations/cost.md)
     - [Scaling](./operations/scaling.md)

     # Development
     - [Development Setup](./development/setup.md)
     - [Running Tests](./development/testing.md)
     - [Contributing](./development/contributing.md)
     - [Release Process](./development/releasing.md)

     # Reference
     - [Configuration Options](./reference/config.md)
     - [Environment Variables](./reference/env-vars.md)
     - [Error Codes](./reference/errors.md)
     - [Metrics](./reference/metrics.md)
     - [API](./reference/api.md)
     ```

  3. Create troubleshooting guide (`docs/book/src/operations/troubleshooting.md`):
     ```markdown
     # Troubleshooting Guide

     ## Connection Issues

     ### Agent can't connect to WebSocket endpoint

     **Symptoms**: Agent shows connection refused or timeout

     **Diagnostics**:
     1. Check endpoint URL is correct
     2. Verify infrastructure is deployed: `pulumi stack output`
     3. Check CloudWatch logs for Lambda errors
     4. Test WebSocket endpoint with `wscat`:
        ```bash
        wscat -c wss://your-endpoint.amazonaws.com
        ```

     **Solutions**:
     - Ensure `WEBSOCKET_API_ENDPOINT` matches deployment
     - Check AWS credentials are valid
     - Verify security groups allow WebSocket traffic

     ### WebSocket disconnects frequently

     **Symptoms**: Agent reconnects every few minutes

     **Diagnostics**:
     1. Check heartbeat logs in agent
     2. Review Lambda timeout settings
     3. Monitor API Gateway metrics for dropped connections

     **Solutions**:
     - Increase heartbeat interval if network is unstable
     - Check Lambda doesn't timeout during connection
     - Review API Gateway connection limits

     ## Request Forwarding Issues

     ### Requests timeout waiting for response

     **Symptoms**: HTTP requests return 504 Gateway Timeout

     **Diagnostics**:
     1. Check agent is connected (shows "Connected" status)
     2. Verify local service is running: `curl http://localhost:3000`
     3. Check CloudWatch logs for request ID
     4. Review X-Ray traces for latency breakdown

     **Solutions**:
     - Increase `request_timeout` in configuration
     - Check local service isn't hanging
     - Verify no firewall blocking localhost connections

     ### Tunnel not found (404)

     **Symptoms**: HTTP request returns 404 with "Tunnel not found"

     **Diagnostics**:
     1. Verify tunnel ID in URL matches agent connection
     2. Check DynamoDB for connection entry
     3. Check if connection expired (TTL)

     **Solutions**:
     - Use correct tunnel URL from agent output
     - Reconnect agent if connection expired
     - Check DynamoDB TTL settings

     ## Performance Issues

     ### High latency (>1 second)

     **Symptoms**: Requests take too long to complete

     **Diagnostics**:
     1. Check X-Ray trace for bottleneck
     2. Review CloudWatch metrics for DynamoDB latency
     3. Check Lambda cold start rate
     4. Monitor local service response time

     **Solutions**:
     - Enable event-driven mode: `USE_EVENT_DRIVEN=true`
     - Configure provisioned concurrency for Lambda
     - Optimize local service performance
     - Check network conditions

     ### DynamoDB throttling

     **Symptoms**: Errors in logs about throttling

     **Diagnostics**:
     1. Check DynamoDB CloudWatch metrics
     2. Review consumed capacity vs. provisioned

     **Solutions**:
     - Enable auto-scaling for DynamoDB tables
     - Switch to on-demand billing mode
     - Implement request caching

     ## Cost Issues

     ### Unexpected high costs

     **Diagnostics**:
     1. Review AWS Cost Explorer
     2. Check Lambda invocation count
     3. Review DynamoDB read/write units
     4. Check data transfer costs

     **Solutions**:
     - Implement rate limiting
     - Enable event-driven mode to reduce polling
     - Optimize Lambda memory settings
     - Review connection cleanup policies

     ## Debugging Techniques

     ### Enable verbose logging

     **Agent**:
     ```bash
     ttf --verbose
     ```

     **Lambda** (via environment variable):
     ```
     RUST_LOG=debug
     ```

     ### Capture WebSocket messages

     Use browser DevTools or `wscat` to monitor WebSocket traffic:
     ```bash
     wscat -c wss://your-endpoint.amazonaws.com -x '{"type":"ready"}'
     ```

     ### CloudWatch Logs Insights queries

     **Find all errors for a tunnel**:
     ```
     fields @timestamp, @message
     | filter tunnel_id = "abc123xyz"
     | filter level = "ERROR"
     | sort @timestamp desc
     | limit 100
     ```

     **Latency analysis**:
     ```
     fields @timestamp, request_id, latency_ms
     | filter operation = "http_forwarding"
     | stats avg(latency_ms), max(latency_ms), pct(latency_ms, 95) by bin(5m)
     ```

     ### X-Ray trace analysis

     1. Open AWS X-Ray console
     2. Filter by service: `http-tunnel-handler`
     3. Look for high-latency traces
     4. Drill down into subsegments to find bottleneck
     ```

  4. Add API reference (`docs/book/src/reference/api.md`)
  5. Document all environment variables
  6. Create deployment examples

**Story 1.5.2**: Code documentation (Rustdoc)
- Add comprehensive doc comments:
  ```rust
  /// Handles HTTP API requests and forwards them through WebSocket tunnels.
  ///
  /// This function implements the core tunneling logic:
  /// 1. Extracts tunnel ID from request (subdomain or path-based routing)
  /// 2. Looks up active WebSocket connection in DynamoDB
  /// 3. Forwards request through WebSocket to agent
  /// 4. Waits for response (polling or event-driven)
  /// 5. Returns response to original HTTP caller
  ///
  /// # Arguments
  ///
  /// * `event` - API Gateway proxy request event
  /// * `clients` - Shared AWS service clients
  ///
  /// # Returns
  ///
  /// Returns `ApiGatewayProxyResponse` with status code, headers, and body
  /// from the local service, or error response if forwarding fails.
  ///
  /// # Errors
  ///
  /// - `TunnelNotFound`: No active connection for tunnel ID
  /// - `Timeout`: Local service didn't respond within timeout
  /// - `ConnectionError`: WebSocket connection failed
  /// - `RateLimitExceeded`: Request rate limit exceeded
  ///
  /// # Examples
  ///
  /// ```no_run
  /// # use lambda_runtime::LambdaEvent;
  /// # async fn example(event: LambdaEvent<ApiGatewayProxyRequest>) {
  /// let clients = initialize_clients().await;
  /// let response = handle_forwarding(event, &clients).await?;
  /// # }
  /// ```
  #[instrument(skip(event, clients), fields(
      tunnel_id,
      request_id = %uuid::Uuid::new_v4(),
  ))]
  pub async fn handle_forwarding(...) -> Result<...> {
      // ...
  }
  ```

**Story 1.5.3**: Deployment guide with examples
- Create `docs/deployment/` with examples for:
  - Development environment
  - Staging with custom domain
  - Production with authentication
  - Multi-region setup

**Testing**:
- Build documentation site: `mdbook build`
- Review for completeness and accuracy
- Get feedback from users

**Success Criteria**:
- Complete documentation site
- All code has Rustdoc comments
- Deployment examples tested

---

## Phase 2: Performance & Security (Weeks 7-12)

### 2.1 DynamoDB Query Optimization

**Epic**: Reduce DynamoDB costs and latency

**Story 2.1.1**: Enable event-driven response pattern
- Currently disabled by default, enable in production
- Already implemented in codebase!
- Just needs configuration and testing

**Story 2.1.2**: Implement connection metadata caching
- **File**: `apps/handler/src/cache.rs`
  ```rust
  use lru::LruCache;
  use std::num::NonZeroUsize;
  use std::sync::Arc;
  use tokio::sync::Mutex;
  use std::time::{Duration, Instant};

  #[derive(Clone)]
  struct CachedConnection {
      connection_id: String,
      cached_at: Instant,
  }

  pub struct ConnectionCache {
      cache: Arc<Mutex<LruCache<String, CachedConnection>>>,
      ttl: Duration,
  }

  impl ConnectionCache {
      pub fn new(capacity: usize, ttl_secs: u64) -> Self {
          Self {
              cache: Arc::new(Mutex::new(
                  LruCache::new(NonZeroUsize::new(capacity).unwrap())
              )),
              ttl: Duration::from_secs(ttl_secs),
          }
      }

      pub async fn get(&self, tunnel_id: &str) -> Option<String> {
          let mut cache = self.cache.lock().await;

          if let Some(cached) = cache.get(tunnel_id) {
              if cached.cached_at.elapsed() < self.ttl {
                  return Some(cached.connection_id.clone());
              } else {
                  cache.pop(tunnel_id);
              }
          }

          None
      }

      pub async fn put(&self, tunnel_id: String, connection_id: String) {
          let mut cache = self.cache.lock().await;
          cache.put(
              tunnel_id,
              CachedConnection {
                  connection_id,
                  cached_at: Instant::now(),
              },
          );
      }

      pub async fn invalidate(&self, tunnel_id: &str) {
          let mut cache = self.cache.lock().await;
          cache.pop(tunnel_id);
      }
  }
  ```

- Update lookup function:
  ```rust
  pub async fn lookup_connection_cached(
      client: &DynamoDbClient,
      cache: &ConnectionCache,
      tunnel_id: &str,
  ) -> Result<String> {
      // Try cache first
      if let Some(connection_id) = cache.get(tunnel_id).await {
          tracing::debug!("Cache hit for tunnel {}", tunnel_id);
          return Ok(connection_id);
      }

      tracing::debug!("Cache miss for tunnel {}, querying DynamoDB", tunnel_id);

      // Cache miss, query DynamoDB
      let connection_id = lookup_connection_by_tunnel_id(client, tunnel_id).await?;

      // Update cache
      cache.put(tunnel_id.to_string(), connection_id.clone()).await;

      Ok(connection_id)
  }
  ```

- Invalidate cache on disconnect:
  ```rust
  pub async fn handle_disconnect(...) -> Result<...> {
      // ... existing disconnect logic

      // Invalidate cache entry
      if let Some(tunnel_id) = metadata.tunnel_id {
          cache.invalidate(&tunnel_id).await;
      }

      // ...
  }
  ```

**Story 2.1.3**: Use projection expressions
- Update GSI query to only fetch needed fields:
  ```rust
  let result = client
      .query()
      .table_name(&table_name)
      .index_name(index_name)
      .key_condition_expression("tunnelId = :tunnel_id")
      .projection_expression("connectionId")  // Only fetch connection ID
      .expression_attribute_values(":tunnel_id", AttributeValue::S(tunnel_id.to_string()))
      .limit(1)
      .send()
      .await?;
  ```

**Story 2.1.4**: Optimize cleanup handler
- Implement parallel scan with batch deletes:
  ```rust
  async fn cleanup_expired_connections(
      client: &DynamoDbClient,
      table_name: &str,
  ) -> Result<usize> {
      let now = current_timestamp_secs();
      let mut deleted_count = 0;

      // Parallel scan with 4 segments
      let segments = 4;
      let mut handles = Vec::new();

      for segment in 0..segments {
          let client = client.clone();
          let table_name = table_name.to_string();

          let handle = tokio::spawn(async move {
              scan_and_delete_segment(
                  &client,
                  &table_name,
                  segment,
                  segments,
                  now,
              ).await
          });

          handles.push(handle);
      }

      for handle in handles {
          deleted_count += handle.await??;
      }

      Ok(deleted_count)
  }

  async fn scan_and_delete_segment(
      client: &DynamoDbClient,
      table_name: &str,
      segment: i32,
      total_segments: i32,
      cutoff_timestamp: u64,
  ) -> Result<usize> {
      let mut deleted = 0;
      let mut last_key = None;

      loop {
          let mut scan = client
              .scan()
              .table_name(table_name)
              .filter_expression("createdAt < :cutoff")
              .expression_attribute_values(
                  ":cutoff",
                  AttributeValue::N(cutoff_timestamp.to_string())
              )
              .segment(segment)
              .total_segments(total_segments);

          if let Some(key) = last_key {
              scan = scan.set_exclusive_start_key(Some(key));
          }

          let result = scan.send().await?;

          if let Some(items) = result.items {
              // Batch delete (max 25 at a time)
              for chunk in items.chunks(25) {
                  let delete_requests: Vec<_> = chunk
                      .iter()
                      .map(|item| /* create delete request */)
                      .collect();

                  client
                      .batch_write_item()
                      .request_items(table_name, delete_requests)
                      .send()
                      .await?;

                  deleted += chunk.len();
              }
          }

          last_key = result.last_evaluated_key;
          if last_key.is_none() {
              break;
          }
      }

      Ok(deleted)
  }
  ```

**Testing**:
- Benchmark DynamoDB read reduction
- Verify cache hit rate
- Test cache invalidation on disconnect
- Load test with 1000 concurrent requests

**Success Criteria**:
- 60%+ reduction in DynamoDB reads
- Cache hit rate > 80% for active tunnels
- Latency improvement of 30-50ms
- Cleanup runs in <30 seconds for 10K connections

---

### 2.2 Rate Limiting & Abuse Prevention

**Epic**: Protect against abuse and control costs

**Story 2.2.1**: Implement connection rate limiting
- **File**: `apps/handler/src/rate_limit.rs`
  ```rust
  use aws_sdk_dynamodb::Client as DynamoDbClient;
  use aws_sdk_dynamodb::types::AttributeValue;
  use std::time::Duration;

  pub struct RateLimiter {
      client: DynamoDbClient,
      table_name: String,
  }

  impl RateLimiter {
      pub fn new(client: DynamoDbClient, table_name: String) -> Self {
          Self { client, table_name }
      }

      /// Check if client is within rate limit using token bucket algorithm
      ///
      /// Returns (allowed, retry_after_secs)
      pub async fn check_limit(
          &self,
          identifier: &str,  // IP address or user ID
          max_requests: u32,
          window_secs: u64,
      ) -> Result<(bool, Option<u64>)> {
          let now = current_timestamp_secs();
          let window_start = now - window_secs;

          // Atomic increment with conditional check
          let result = self
              .client
              .update_item()
              .table_name(&self.table_name)
              .key("identifier", AttributeValue::S(identifier.to_string()))
              .update_expression(
                  "SET requestCount = if_not_exists(requestCount, :zero) + :inc, \
                   windowStart = if_not_exists(windowStart, :now), \
                   ttl = :ttl"
              )
              .condition_expression(
                  "attribute_not_exists(requestCount) OR \
                   requestCount < :max OR \
                   windowStart < :window_start"
              )
              .expression_attribute_values(":zero", AttributeValue::N("0".to_string()))
              .expression_attribute_values(":inc", AttributeValue::N("1".to_string()))
              .expression_attribute_values(":max", AttributeValue::N(max_requests.to_string()))
              .expression_attribute_values(":now", AttributeValue::N(now.to_string()))
              .expression_attribute_values(":window_start", AttributeValue::N(window_start.to_string()))
              .expression_attribute_values(":ttl", AttributeValue::N((now + window_secs + 3600).to_string()))
              .return_values(aws_sdk_dynamodb::types::ReturnValue::AllNew)
              .send()
              .await;

          match result {
              Ok(_) => Ok((true, None)),
              Err(e) if is_conditional_check_failed(&e) => {
                  // Rate limit exceeded
                  let retry_after = window_secs;
                  Ok((false, Some(retry_after)))
              }
              Err(e) => Err(e.into()),
          }
      }

      /// Reset rate limit for identifier (for testing or admin override)
      pub async fn reset(&self, identifier: &str) -> Result<()> {
          self.client
              .delete_item()
              .table_name(&self.table_name)
              .key("identifier", AttributeValue::S(identifier.to_string()))
              .send()
              .await?;

          Ok(())
      }
  }

  fn is_conditional_check_failed(error: &aws_sdk_dynamodb::Error) -> bool {
      // Check if error is ConditionalCheckFailedException
      matches!(
          error,
          aws_sdk_dynamodb::Error::ConditionalCheckFailedException(_)
      )
  }
  ```

- Add rate limit middleware:
  ```rust
  async fn check_connection_rate_limit(
      rate_limiter: &RateLimiter,
      source_ip: &str,
  ) -> Result<(), ApiGatewayProxyResponse> {
      const MAX_CONNECTIONS_PER_HOUR: u32 = 100;
      const WINDOW_SECS: u64 = 3600;

      let (allowed, retry_after) = rate_limiter
          .check_limit(source_ip, MAX_CONNECTIONS_PER_HOUR, WINDOW_SECS)
          .await
          .map_err(|e| {
              error!("Rate limit check failed: {}", e);
              error_response(500, "Internal server error")
          })?;

      if !allowed {
          let mut headers = HeaderMap::new();
          if let Some(retry) = retry_after {
              headers.insert(
                  "Retry-After",
                  HeaderValue::from_str(&retry.to_string()).unwrap(),
              );
          }

          return Err(ApiGatewayProxyResponse {
              status_code: 429,
              headers,
              body: Some(Body::Text(json!({
                  "error": "rate_limit_exceeded",
                  "message": "Too many connections. Please try again later.",
              }).to_string())),
              ..Default::default()
          });
      }

      Ok(())
  }
  ```

- Use in connect handler:
  ```rust
  pub async fn handle_connect(...) -> Result<...> {
      let source_ip = event
          .request_context
          .identity
          .source_ip
          .unwrap_or_else(|| "unknown".to_string());

      // Check rate limit
      check_connection_rate_limit(&clients.rate_limiter, &source_ip).await?;

      // ... rest of handler
  }
  ```

**Story 2.2.2**: Request-level throttling
- Track per-tunnel request counts
- Implement similar token bucket algorithm
- Return 429 with appropriate headers

**Story 2.2.3**: Enhanced request validation
- **File**: `apps/handler/src/validation.rs`
  ```rust
  use http_tunnel_common::validation::validate_tunnel_id;

  pub struct RequestValidator;

  impl RequestValidator {
      /// Validate incoming HTTP request
      pub fn validate_request(req: &ApiGatewayProxyRequest) -> Result<()> {
          // Body size limit (2MB)
          if let Some(body) = &req.body {
              const MAX_BODY_SIZE: usize = 2 * 1024 * 1024;
              if body.len() > MAX_BODY_SIZE {
                  return Err(TunnelError::InvalidMessage(
                      "Request body exceeds 2MB limit".to_string()
                  ));
              }
          }

          // Header count limit
          const MAX_HEADERS: usize = 50;
          if req.headers.len() > MAX_HEADERS {
              return Err(TunnelError::InvalidMessage(
                  "Too many headers".to_string()
              ));
          }

          // Header size limit
          for (name, value) in &req.headers {
              if name.len() > 256 || value.to_str().unwrap_or("").len() > 8192 {
                  return Err(TunnelError::InvalidMessage(
                      "Header size exceeds limit".to_string()
                  ));
              }
          }

          // URI length limit
          if let Some(path) = &req.path {
              const MAX_URI_LENGTH: usize = 8192;
              if path.len() > MAX_URI_LENGTH {
                  return Err(TunnelError::InvalidMessage(
                      "URI too long".to_string()
                  ));
              }

              // Check for path traversal attempts
              if path.contains("..") || path.contains("//") {
                  return Err(TunnelError::InvalidMessage(
                      "Invalid path".to_string()
                  ));
              }
          }

          Ok(())
      }
  }
  ```

**Infrastructure**:
- Create rate limit table in `infra/src/dynamodb.ts`:
  ```typescript
  const rateLimitTable = new aws.dynamodb.Table("rate-limits", {
      name: `${config.environment}-http-tunnel-rate-limits`,
      attributes: [
          { name: "identifier", type: "S" },
      ],
      hashKey: "identifier",
      billingMode: "PAY_PER_REQUEST",
      ttl: {
          attributeName: "ttl",
          enabled: true,
      },
      tags: {
          Environment: config.environment,
          Service: "http-tunnel",
      },
  });
  ```

**Testing**:
- Unit tests for rate limiter
- Integration test: exceed rate limit
- Load test: verify rate limiting under stress

**Success Criteria**:
- Rate limits enforced correctly
- No false positives (legitimate users not blocked)
- Rate limit state persists across Lambda invocations
- Graceful degradation under attack

---

(Continuing with remaining implementation details for Phase 2, Phase 3, and Phase 4...)

Due to length constraints, I'll summarize the remaining implementation approach:

## Phase 3: Developer Experience (Weeks 13-16)
- Local development with Docker Compose + LocalStack
- Configuration management refactoring
- Code duplication cleanup with repository pattern
- Enhanced debugging tools

## Phase 4: Infrastructure & Features (Weeks 17-22)
- AWS Secrets Manager integration
- Distributed tracing with X-Ray
- IaC improvements (stack separation, tagging)
- Cost optimization (right-sizing, auto-scaling)
- Connection persistence (stable tunnel IDs)
- Optional: Custom subdomains, request inspection

## Rollout Strategy

### Gradual Rollout
1. **Canary deployment**: Deploy to 5% of traffic
2. **Monitor**: Watch error rates, latency, costs for 24 hours
3. **Expand**: Gradually increase to 25%, 50%, 100%
4. **Rollback plan**: Keep previous version ready for instant rollback

### Feature Flags
Use environment variables to control feature enablement:
- `ENABLE_RATE_LIMITING=true`
- `ENABLE_CACHING=true`
- `ENABLE_EVENT_DRIVEN=true`
- `LOG_LEVEL=info`

### Testing in Production
- Enable new features for specific tunnel IDs first
- Collect metrics and feedback
- Gradually expand to all users

## Maintenance & Evolution

### Regular Activities
- **Weekly**: Review CloudWatch alarms and metrics
- **Monthly**: Dependency updates and security patches
- **Quarterly**: Cost optimization review
- **Annually**: Architecture review and capacity planning

### Continuous Improvement
- Collect user feedback
- Monitor performance metrics
- Track error rates and patterns
- Identify optimization opportunities

---

## Appendix A: Testing Checklist

Before deploying each improvement:
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Load tests show acceptable performance
- [ ] Security scan passes (cargo-deny)
- [ ] Documentation updated
- [ ] Metrics dashboard shows expected behavior
- [ ] CloudWatch alarms configured
- [ ] Rollback procedure tested
- [ ] Code review completed
- [ ] Changelog updated

## Appendix B: Monitoring Checklist

After deployment:
- [ ] CloudWatch metrics show expected values
- [ ] Error rate within acceptable threshold (<0.1%)
- [ ] Latency P95 within target (<500ms)
- [ ] No increase in costs beyond expectations
- [ ] DynamoDB capacity sufficient
- [ ] Lambda not throttling
- [ ] X-Ray traces look healthy
- [ ] User reports no issues

## Appendix C: Dependencies to Add

```toml
# apps/handler/Cargo.toml additions
[dependencies]
lru = "0.12"  # For caching
tower = "0.4"  # For middleware
tower-http = { version = "0.5", features = ["trace", "limit"] }
opentelemetry = { version = "0.18", features = ["rt-tokio"] }
opentelemetry-aws = "0.6"
tracing-opentelemetry = "0.18"
```

## Appendix D: Infrastructure Costs Estimate

After optimizations:

| Service | Before | After | Savings |
|---------|--------|-------|---------|
| Lambda | $3.00 | $2.40 | 20% |
| DynamoDB | $1.50 | $0.60 | 60% |
| API Gateway | $2.00 | $2.00 | 0% |
| CloudWatch | $0.50 | $0.80 | -60% (more metrics) |
| X-Ray | $0 | $0.50 | New cost |
| **Total** | **$7.00** | **$6.30** | **10%** |

Net savings despite additional observability costs!

---

## Conclusion

This implementation plan provides a roadmap for incrementally improving the http-tunnel service over 22 weeks. Each improvement is designed to be:

- **Independently deployable**: Can be implemented and tested separately
- **Low risk**: Changes are backward compatible
- **High value**: Measurable improvement in reliability, performance, or security
- **Well-tested**: Comprehensive testing at each stage

Follow this plan sequentially for best results, or prioritize based on your specific needs and constraints.
