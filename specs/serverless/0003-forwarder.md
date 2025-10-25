# Local Forwarder (Client Agent) Specification

## Overview

The forwarder is a Rust CLI application that runs on the developer's local machine. It establishes a persistent WebSocket connection to the AWS-hosted tunnel service and forwards incoming HTTP requests to a local service.

## Architecture

### High-Level Flow

```
Internet Request → API Gateway HTTP API → Lambda ForwardingHandler
                                             ↓ (PostToConnection)
                                          WebSocket API
                                             ↓
                        ┌────────────────────┴───────────────────┐
                        │   Local Forwarder (This Application)   │
                        └────────────────────┬───────────────────┘
                                             ↓
                              Local Service (localhost:PORT)
```

### Component Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Main Application                       │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐ │
│  │ CLI Parser   │  │   Config     │  │  Supervisor  │ │
│  └──────────────┘  └──────────────┘  └──────────────┘ │
└────────────────────────┬────────────────────────────────┘
                         │
        ┌────────────────┴─────────────────┐
        ├─ Connection Manager (Reconnect)  │
        │     ├─ WebSocket Handler         │
        │     ├─ Heartbeat Task            │
        │     └─ Message Router            │
        │                                   │
        └─ Request Handler Pool             │
              └─ HTTP Client → Local Service
```

## Core Components

### 1. CLI Interface

#### Command Structure

```bash
# Basic usage
forwarder --port 3000

# With custom WebSocket endpoint
forwarder --port 3000 --endpoint wss://tunnel.example.com

# With authentication token
forwarder --port 3000 --token <JWT_TOKEN>

# With verbose logging
forwarder --port 3000 --verbose

# Show version
forwarder --version
```

#### CLI Arguments

```rust
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "ttf")]
#[command(about = "Local HTTP tunnel forwarder agent", long_about = None)]
#[command(version)]
struct Args {
    /// Local port to forward requests to
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Local host address
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// WebSocket tunnel endpoint
    #[arg(short, long, env = "TUNNEL_ENDPOINT")]
    endpoint: String,

    /// Authentication token (JWT)
    #[arg(short, long, env = "TUNNEL_TOKEN")]
    token: Option<String>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Connection timeout in seconds
    #[arg(long, default_value = "10")]
    connect_timeout: u64,

    /// Request timeout in seconds
    #[arg(long, default_value = "25")]
    request_timeout: u64,
}
```

### 2. Configuration

```rust
#[derive(Debug, Clone)]
pub struct Config {
    /// Local service address (e.g., "http://127.0.0.1:3000")
    pub local_address: String,

    /// WebSocket endpoint URL with optional token
    pub websocket_url: String,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Request timeout when calling local service
    pub request_timeout: Duration,

    /// Heartbeat interval
    pub heartbeat_interval: Duration,

    /// Reconnection strategy
    pub reconnect_config: ReconnectConfig,
}

#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    pub min_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub max_attempts: Option<usize>,
}

impl Config {
    pub fn from_args(args: Args) -> Self {
        let mut websocket_url = args.endpoint;

        // Append token as query parameter if provided
        if let Some(token) = args.token {
            websocket_url = format!("{}?token={}", websocket_url, token);
        }

        Self {
            local_address: format!("http://{}:{}", args.host, args.port),
            websocket_url,
            connect_timeout: Duration::from_secs(args.connect_timeout),
            request_timeout: Duration::from_secs(args.request_timeout),
            heartbeat_interval: Duration::from_secs(HEARTBEAT_INTERVAL_SECS),
            reconnect_config: ReconnectConfig {
                min_delay: Duration::from_millis(RECONNECT_MIN_DELAY_MS),
                max_delay: Duration::from_millis(RECONNECT_MAX_DELAY_MS),
                multiplier: RECONNECT_MULTIPLIER,
                max_attempts: None, // Infinite retries
            },
        }
    }
}
```

### 3. Connection Manager

The connection manager is responsible for establishing and maintaining the WebSocket connection with automatic reconnection.

```rust
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};
use futures_util::{StreamExt, SinkExt};

pub struct ConnectionManager {
    config: Config,
    connection_state: Arc<Mutex<ConnectionState>>,
}

#[derive(Debug, Clone)]
enum ConnectionState {
    Disconnected,
    Connecting,
    Connected {
        connection_id: String,
        public_url: String,
    },
    Reconnecting {
        attempt: usize,
        next_delay: Duration,
    },
}

impl ConnectionManager {
    pub async fn run(&self) -> Result<()> {
        let mut reconnect_delay = self.config.reconnect_config.min_delay;
        let mut attempt = 0;

        loop {
            match self.establish_connection().await {
                Ok((ws_stream, public_url)) => {
                    info!("✓ Tunnel established: {}", public_url);
                    reconnect_delay = self.config.reconnect_config.min_delay;
                    attempt = 0;

                    // Handle the connection until it drops
                    if let Err(e) = self.handle_connection(ws_stream).await {
                        error!("Connection error: {}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to connect: {}", e);
                }
            }

            // Reconnection backoff
            attempt += 1;
            info!("Reconnecting in {:?} (attempt {})", reconnect_delay, attempt);

            tokio::time::sleep(reconnect_delay).await;

            // Exponential backoff
            reconnect_delay = Duration::from_millis(
                (reconnect_delay.as_millis() as f64 * self.config.reconnect_config.multiplier)
                    .min(self.config.reconnect_config.max_delay.as_millis() as f64)
                    as u64
            );
        }
    }

    async fn establish_connection(&self) -> Result<(WebSocketStream, String)> {
        debug!("Connecting to {}", self.config.websocket_url);

        let (ws_stream, _) = connect_async(&self.config.websocket_url)
            .await
            .map_err(|e| TunnelError::ConnectionError(e.to_string()))?;

        // Wait for ConnectionEstablished message
        // (Implementation details in message handling section)

        Ok((ws_stream, public_url))
    }

    async fn handle_connection(&self, ws_stream: WebSocketStream) -> Result<()> {
        let (write, read) = ws_stream.split();

        // Create channels for internal communication
        let (outgoing_tx, outgoing_rx) = mpsc::channel(100);

        // Spawn tasks
        let write_task = self.spawn_write_task(write, outgoing_rx);
        let read_task = self.spawn_read_task(read, outgoing_tx.clone());
        let heartbeat_task = self.spawn_heartbeat_task(outgoing_tx.clone());

        // Wait for any task to complete (usually means connection dropped)
        tokio::select! {
            result = write_task => {
                warn!("Write task ended: {:?}", result);
            }
            result = read_task => {
                warn!("Read task ended: {:?}", result);
            }
            result = heartbeat_task => {
                warn!("Heartbeat task ended: {:?}", result);
            }
        }

        Ok(())
    }
}
```

### 4. Message Handler

```rust
async fn spawn_read_task(
    mut read: SplitStream<WebSocketStream>,
    outgoing_tx: mpsc::Sender<WsMessage>,
) -> Result<()> {
    while let Some(message) = read.next().await {
        match message {
            Ok(WsMessage::Text(text)) => {
                if let Err(e) = self.handle_text_message(&text, &outgoing_tx).await {
                    error!("Error handling message: {}", e);
                }
            }
            Ok(WsMessage::Binary(_)) => {
                warn!("Received unexpected binary message");
            }
            Ok(WsMessage::Ping(data)) => {
                if let Err(e) = outgoing_tx.send(WsMessage::Pong(data)).await {
                    error!("Failed to send pong: {}", e);
                    break;
                }
            }
            Ok(WsMessage::Pong(_)) => {
                // Heartbeat acknowledged
            }
            Ok(WsMessage::Close(_)) => {
                info!("Server closed connection");
                break;
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
            _ => {}
        }
    }

    Ok(())
}

async fn handle_text_message(
    &self,
    text: &str,
    outgoing_tx: &mpsc::Sender<WsMessage>,
) -> Result<()> {
    let message: Message = serde_json::from_str(text)?;

    match message {
        Message::ConnectionEstablished {
            connection_id,
            public_subdomain,
            public_url,
        } => {
            info!("✓ Connection established");
            info!("  Connection ID: {}", connection_id);
            info!("  Public URL: {}", public_url);

            let mut state = self.connection_state.lock().await;
            *state = ConnectionState::Connected {
                connection_id,
                public_url: public_url.clone(),
            };
        }

        Message::HttpRequest(request) => {
            // Spawn a new task to handle this request
            let local_address = self.config.local_address.clone();
            let request_timeout = self.config.request_timeout;
            let outgoing_tx = outgoing_tx.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_http_request(
                    request,
                    &local_address,
                    request_timeout,
                    outgoing_tx,
                ).await {
                    error!("Failed to handle request: {}", e);
                }
            });
        }

        Message::Pong => {
            debug!("Received pong");
        }

        Message::Error { request_id, code, message } => {
            error!("Server error: {:?} - {}", code, message);
        }

        _ => {
            warn!("Received unexpected message: {:?}", message);
        }
    }

    Ok(())
}
```

### 5. HTTP Request Handler

```rust
use reqwest::Client;
use http_tunnel_common::{HttpRequest, HttpResponse, Message};

async fn handle_http_request(
    request: HttpRequest,
    local_address: &str,
    timeout: Duration,
    outgoing_tx: mpsc::Sender<WsMessage>,
) -> Result<()> {
    let start_time = Instant::now();
    let request_id = request.request_id.clone();

    debug!("→ {} {}", request.method, request.uri);

    // Build HTTP request to local service
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| TunnelError::HttpError(e.to_string()))?;

    let url = format!("{}{}", local_address, request.uri);

    let mut req_builder = match request.method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url),
        _ => {
            return Err(TunnelError::InvalidMessage(
                format!("Unsupported HTTP method: {}", request.method)
            ));
        }
    };

    // Add headers
    for (name, values) in request.headers.iter() {
        for value in values {
            req_builder = req_builder.header(name, value);
        }
    }

    // Add body if present
    if !request.body.is_empty() {
        let body_bytes = decode_body(&request.body)?;
        req_builder = req_builder.body(body_bytes);
    }

    // Execute request
    match req_builder.send().await {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let headers = headers_to_map(response.headers());
            let body_bytes = response.bytes().await
                .map_err(|e| TunnelError::HttpError(e.to_string()))?;
            let body = encode_body(&body_bytes);

            let processing_time = start_time.elapsed().as_millis() as u64;

            debug!("← {} ({}ms)", status_code, processing_time);

            let http_response = HttpResponse {
                request_id,
                status_code,
                headers,
                body,
                processing_time_ms: processing_time,
            };

            let response_message = Message::HttpResponse(http_response);
            let response_json = serde_json::to_string(&response_message)?;

            outgoing_tx.send(WsMessage::Text(response_json)).await
                .map_err(|e| TunnelError::WebSocketError(e.to_string()))?;
        }
        Err(e) => {
            error!("Local service error: {}", e);

            let error_message = Message::Error {
                request_id: Some(request_id),
                code: ErrorCode::LocalServiceUnavailable,
                message: e.to_string(),
            };

            let error_json = serde_json::to_string(&error_message)?;

            outgoing_tx.send(WsMessage::Text(error_json)).await
                .map_err(|e| TunnelError::WebSocketError(e.to_string()))?;
        }
    }

    Ok(())
}
```

### 6. Heartbeat Task

```rust
async fn spawn_heartbeat_task(
    outgoing_tx: mpsc::Sender<WsMessage>,
    interval: Duration,
) -> Result<()> {
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;

        let ping_message = Message::Ping;
        let ping_json = serde_json::to_string(&ping_message)?;

        if let Err(e) = outgoing_tx.send(WsMessage::Text(ping_json)).await {
            error!("Failed to send heartbeat: {}", e);
            break;
        }

        debug!("Sent heartbeat");
    }

    Ok(())
}
```

### 7. Write Task

```rust
async fn spawn_write_task(
    mut write: SplitSink<WebSocketStream, WsMessage>,
    mut outgoing_rx: mpsc::Receiver<WsMessage>,
) -> Result<()> {
    while let Some(message) = outgoing_rx.recv().await {
        if let Err(e) = write.send(message).await {
            error!("Failed to send message: {}", e);
            break;
        }
    }

    Ok(())
}
```

## Main Entry Point

```rust
use tracing::{info, debug, error, warn};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Initialize logging
    let log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    info!("HTTP Tunnel Forwarder v{}", env!("CARGO_PKG_VERSION"));
    info!("Local service: {}:{}", args.host, args.port);

    // Build configuration
    let config = Config::from_args(args);

    // Create and run connection manager
    let manager = ConnectionManager::new(config);

    // Run until interrupted
    tokio::select! {
        result = manager.run() => {
            error!("Connection manager exited: {:?}", result);
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C, shutting down...");
        }
    }

    Ok(())
}
```

## Dependencies

Add to `apps/forwarder/Cargo.toml`:

```toml
[dependencies]
anyhow.workspace = true
http-tunnel-common.workspace = true
tokio = { workspace = true, features = ["full"] }
serde.workspace = true
serde_json.workspace = true

# Additional dependencies
clap = { version = "4.5", features = ["derive", "env"] }
tokio-tungstenite = { version = "0.26", features = ["native-tls"] }
futures-util = "0.3"
reqwest = { version = "0.12", features = ["json"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
url = "2.5"
```

## Error Handling

### Graceful Degradation

1. **Connection Failures**: Automatic reconnection with exponential backoff
2. **Local Service Errors**: Return error response to server, don't crash agent
3. **Malformed Messages**: Log and skip, continue processing
4. **Timeout Handling**: Return timeout error to server

### Logging Strategy

- **INFO**: Connection status, public URL, reconnection attempts
- **DEBUG**: Individual requests, heartbeats, message details
- **ERROR**: Connection failures, request errors, unexpected conditions
- **WARN**: Unexpected messages, non-fatal issues

## Testing Strategy

### Unit Tests

- Test message parsing and serialization
- Test request/response conversion
- Test exponential backoff calculation
- Test header conversion

### Integration Tests

- Test full request forwarding cycle with mock local server
- Test reconnection logic with mock WebSocket server
- Test heartbeat functionality
- Test timeout handling

### Manual Testing

```bash
# Terminal 1: Start a simple local server
python3 -m http.server 8000

# Terminal 2: Run forwarder
cargo run --bin http-tunnel-forwarder -- --port 8000 --endpoint wss://your-api.com

# Terminal 3: Test with curl
curl https://abc123.your-domain.com
```

## Performance Considerations

1. **Concurrent Request Handling**: Spawn separate tokio tasks per request
2. **Connection Pooling**: Reuse reqwest Client across requests
3. **Bounded Channels**: Use bounded mpsc channels to apply backpressure
4. **Memory Management**: Stream large responses rather than buffering entirely
5. **Connection Reuse**: Keep WebSocket connection alive with heartbeats

## Security Considerations

1. **TLS Verification**: Always use native-tls for WebSocket connections
2. **Token Handling**: Don't log tokens, pass via environment variables
3. **Local Access Only**: Default to 127.0.0.1, don't expose to network
4. **Request Validation**: Validate incoming requests before forwarding
5. **Size Limits**: Enforce maximum body size to prevent memory exhaustion
