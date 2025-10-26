use anyhow::Result;
use clap::Parser;
use futures_util::{SinkExt, StreamExt, stream::SplitSink, stream::SplitStream};
use http_tunnel_common::{
    ErrorCode, HttpRequest, HttpResponse, Message, TunnelError,
    constants::{
        HEARTBEAT_INTERVAL_SECS, RECONNECT_MAX_DELAY_MS, RECONNECT_MIN_DELAY_MS,
        RECONNECT_MULTIPLIER,
    },
    decode_body, encode_body, headers_to_map,
};
use reqwest::Client;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as WsMessage,
};
use tracing::{debug, error, info, warn};

type WebSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// CLI arguments for the forwarder agent
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
    #[arg(
        short,
        long,
        env = "TTF_ENDPOINT",
        default_value = "wss://your-websocket-api.execute-api.us-east-1.amazonaws.com/dev"
    )]
    endpoint: String,

    /// Authentication token (JWT)
    #[arg(short, long, env = "TTF_TOKEN")]
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

/// Configuration for the forwarder
#[derive(Debug, Clone)]
pub struct Config {
    /// Local service address (e.g., "http://127.0.0.1:3000")
    pub local_address: String,

    /// WebSocket endpoint URL
    pub websocket_url: String,

    /// Authentication token (JWT)
    pub token: Option<String>,

    /// Connection timeout
    pub connect_timeout: Duration,

    /// Request timeout when calling local service
    pub request_timeout: Duration,

    /// Heartbeat interval
    pub heartbeat_interval: Duration,

    /// Reconnection strategy
    pub reconnect_config: ReconnectConfig,
}

/// Reconnection configuration with exponential backoff
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    pub min_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
    pub max_attempts: Option<usize>,
}

impl Config {
    fn from_args(args: Args) -> Self {
        Self {
            local_address: format!("http://{}:{}", args.host, args.port),
            websocket_url: args.endpoint,
            token: args.token,
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

/// Connection state tracking
#[derive(Debug, Clone)]
#[allow(dead_code)]
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

/// Connection manager handles WebSocket lifecycle and reconnection
pub struct ConnectionManager {
    config: Config,
    connection_state: Arc<Mutex<ConnectionState>>,
}

impl ConnectionManager {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            connection_state: Arc::new(Mutex::new(ConnectionState::Disconnected)),
        }
    }

    /// Main run loop with automatic reconnection
    pub async fn run(&self) -> Result<()> {
        let mut reconnect_delay = self.config.reconnect_config.min_delay;
        let mut attempt = 0;

        loop {
            // Update state to connecting
            {
                let mut state = self.connection_state.lock().await;
                *state = ConnectionState::Connecting;
            }

            match self.establish_connection().await {
                Ok((ws_stream, public_url)) => {
                    info!("Tunnel established: {}", public_url);
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
            {
                let mut state = self.connection_state.lock().await;
                *state = ConnectionState::Reconnecting {
                    attempt,
                    next_delay: reconnect_delay,
                };
            }

            info!(
                "Reconnecting in {:?} (attempt {})",
                reconnect_delay, attempt
            );
            tokio::time::sleep(reconnect_delay).await;

            // Exponential backoff
            reconnect_delay = Duration::from_millis(
                ((reconnect_delay.as_millis() as f64 * self.config.reconnect_config.multiplier)
                    .min(self.config.reconnect_config.max_delay.as_millis() as f64))
                    as u64,
            );
        }
    }

    /// Establish WebSocket connection and perform handshake
    async fn establish_connection(&self) -> Result<(WebSocket, String)> {
        debug!("Connecting to {}", self.config.websocket_url);

        // Build WebSocket request with optional auth token
        let (mut ws_stream, _) = if let Some(ref token) = self.config.token {
            // Use Authorization header for auth (works with both direct and custom domains)
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            use tokio_tungstenite::tungstenite::http::HeaderValue;

            let mut request = self
                .config
                .websocket_url
                .clone()
                .into_client_request()
                .map_err(|e| TunnelError::ConnectionError(format!("Invalid URL: {}", e)))?;

            // Add token as Authorization header
            request.headers_mut().insert(
                "Authorization",
                HeaderValue::from_str(&format!("Bearer {}", token))
                    .map_err(|e| TunnelError::ConnectionError(format!("Invalid token: {}", e)))?,
            );

            debug!("Connecting with authentication token (Authorization header)");
            connect_async(request)
                .await
                .map_err(|e| TunnelError::ConnectionError(e.to_string()))?
        } else {
            debug!("Connecting without authentication");
            connect_async(&self.config.websocket_url)
                .await
                .map_err(|e| TunnelError::ConnectionError(e.to_string()))?
        };

        info!("✅ WebSocket connection established, sending Ready message");

        // Send Ready message to request connection info
        let ready_msg = Message::Ready;
        let ready_json = serde_json::to_string(&ready_msg)
            .map_err(|e| TunnelError::InternalError(format!("Failed to serialize Ready: {}", e)))?;

        ws_stream
            .send(WsMessage::Text(ready_json.into()))
            .await
            .map_err(|e| TunnelError::WebSocketError(format!("Failed to send Ready: {}", e)))?;

        debug!("Sent Ready message, waiting for ConnectionEstablished response");

        // Wait for ConnectionEstablished message with timeout
        let timeout = tokio::time::timeout(self.config.connect_timeout, async {
            while let Some(message) = ws_stream.next().await {
                match message {
                    Ok(WsMessage::Text(text)) => {
                        if let Ok(Message::ConnectionEstablished {
                            connection_id,
                            tunnel_id: _,
                            public_url,
                        }) = serde_json::from_str::<Message>(&text)
                        {
                            let mut state = self.connection_state.lock().await;
                            *state = ConnectionState::Connected {
                                connection_id: connection_id.clone(),
                                public_url: public_url.clone(),
                            };
                            return Ok(public_url);
                        }
                    }
                    Ok(WsMessage::Close(_)) => {
                        return Err(TunnelError::ConnectionError(
                            "Server closed connection during handshake".to_string(),
                        ));
                    }
                    Err(e) => {
                        return Err(TunnelError::WebSocketError(e.to_string()));
                    }
                    _ => {}
                }
            }
            Err(TunnelError::ConnectionError(
                "Connection closed before handshake".to_string(),
            ))
        });

        let public_url = timeout.await.map_err(|_| {
            TunnelError::ConnectionError("Connection handshake timeout".to_string())
        })??;

        Ok((ws_stream, public_url))
    }

    /// Handle active WebSocket connection with split read/write tasks
    async fn handle_connection(&self, ws_stream: WebSocket) -> Result<()> {
        let (write, read) = ws_stream.split();

        // Create channels for internal communication
        let (outgoing_tx, outgoing_rx) = mpsc::channel(100);

        // Spawn concurrent tasks
        let write_handle = tokio::spawn(spawn_write_task(write, outgoing_rx));

        let read_handle = tokio::spawn(spawn_read_task(
            read,
            outgoing_tx.clone(),
            self.config.local_address.clone(),
            self.config.request_timeout,
        ));

        let heartbeat_handle = tokio::spawn(spawn_heartbeat_task(
            outgoing_tx.clone(),
            self.config.heartbeat_interval,
        ));

        // Wait for any task to complete (usually means connection dropped)
        tokio::select! {
            result = write_handle => {
                warn!("Write task ended: {:?}", result);
            }
            result = read_handle => {
                warn!("Read task ended: {:?}", result);
            }
            result = heartbeat_handle => {
                warn!("Heartbeat task ended: {:?}", result);
            }
        }

        // Update state to disconnected
        {
            let mut state = self.connection_state.lock().await;
            *state = ConnectionState::Disconnected;
        }

        Ok(())
    }
}

/// Write task sends outgoing messages through WebSocket
async fn spawn_write_task(
    mut write: SplitSink<WebSocket, WsMessage>,
    mut outgoing_rx: mpsc::Receiver<WsMessage>,
) -> Result<()> {
    while let Some(message) = outgoing_rx.recv().await {
        if let Err(e) = write.send(message).await {
            error!("Failed to send message: {}", e);
            break;
        }
    }

    debug!("Write task exiting");
    Ok(())
}

/// Read task receives incoming messages and dispatches them
async fn spawn_read_task(
    mut read: SplitStream<WebSocket>,
    outgoing_tx: mpsc::Sender<WsMessage>,
    local_address: String,
    request_timeout: Duration,
) -> Result<()> {
    while let Some(message) = read.next().await {
        match message {
            Ok(WsMessage::Text(text)) => {
                if let Err(e) =
                    handle_text_message(&text, &outgoing_tx, &local_address, request_timeout).await
                {
                    error!("Error handling message: {}", e);
                }
            }
            Ok(WsMessage::Binary(_)) => {
                warn!("Received unexpected binary message");
            }
            Ok(WsMessage::Ping(data)) => {
                debug!("Received WebSocket ping");
                if let Err(e) = outgoing_tx.send(WsMessage::Pong(data)).await {
                    error!("Failed to send pong: {}", e);
                    break;
                }
            }
            Ok(WsMessage::Pong(_)) => {
                debug!("Received WebSocket pong");
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

    debug!("Read task exiting");
    Ok(())
}

/// Handle incoming text messages
async fn handle_text_message(
    text: &str,
    outgoing_tx: &mpsc::Sender<WsMessage>,
    local_address: &str,
    request_timeout: Duration,
) -> Result<()> {
    let message: Message = serde_json::from_str(text)
        .map_err(|e| TunnelError::InvalidMessage(format!("Failed to parse message: {}", e)))?;

    match message {
        Message::ConnectionEstablished {
            connection_id,
            tunnel_id: _,
            public_url,
        } => {
            info!("Connection established");
            info!("  Connection ID: {}", connection_id);
            info!("  Public URL: {}", public_url);
        }

        Message::HttpRequest(request) => {
            debug!("Received HTTP request: {} {}", request.method, request.uri);

            // Spawn a new task to handle this request concurrently
            let local_address = local_address.to_string();
            let outgoing_tx = outgoing_tx.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    handle_http_request(request, &local_address, request_timeout, outgoing_tx).await
                {
                    error!("Failed to handle request: {}", e);
                }
            });
        }

        Message::Pong => {
            debug!("Received pong");
        }

        Message::Error {
            request_id,
            code,
            message,
        } => {
            error!(
                "Server error: {:?} - {} (request_id: {:?})",
                code, message, request_id
            );
        }

        _ => {
            warn!("Received unexpected message type");
        }
    }

    Ok(())
}

/// Handle HTTP request by forwarding to local service
async fn handle_http_request(
    request: HttpRequest,
    local_address: &str,
    timeout: Duration,
    outgoing_tx: mpsc::Sender<WsMessage>,
) -> Result<()> {
    let start_time = Instant::now();
    let request_id = request.request_id.clone();

    debug!("Forwarding: {} {}", request.method, request.uri);

    // Build HTTP client
    let client = Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| TunnelError::HttpError(e.to_string()))?;

    let url = format!("{}{}", local_address, request.uri);

    // Build request with proper method
    let mut req_builder = match request.method.as_str() {
        "GET" => client.get(&url),
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "PATCH" => client.patch(&url),
        "HEAD" => client.head(&url),
        "OPTIONS" => client.request(reqwest::Method::OPTIONS, &url),
        _ => {
            return Err(TunnelError::InvalidMessage(format!(
                "Unsupported HTTP method: {}",
                request.method
            ))
            .into());
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
        let body_bytes = decode_body(&request.body)
            .map_err(|e| TunnelError::InvalidMessage(format!("Failed to decode body: {}", e)))?;
        req_builder = req_builder.body(body_bytes);
    }

    // Execute request
    match req_builder.send().await {
        Ok(response) => {
            let status_code = response.status().as_u16();
            let headers = headers_to_map(response.headers());
            let body_bytes = response
                .bytes()
                .await
                .map_err(|e| TunnelError::HttpError(e.to_string()))?;
            let body = encode_body(&body_bytes);

            let processing_time = start_time.elapsed().as_millis() as u64;

            debug!("Response: {} ({}ms)", status_code, processing_time);

            let http_response = HttpResponse {
                request_id,
                status_code,
                headers,
                body,
                processing_time_ms: processing_time,
            };

            let response_message = Message::HttpResponse(http_response);
            let response_json = serde_json::to_string(&response_message)
                .map_err(|e| TunnelError::InvalidMessage(e.to_string()))?;

            outgoing_tx
                .send(WsMessage::Text(response_json.into()))
                .await
                .map_err(|e| TunnelError::WebSocketError(e.to_string()))?;
        }
        Err(e) => {
            error!("Local service error: {}", e);

            let error_message = Message::Error {
                request_id: Some(request_id),
                code: ErrorCode::LocalServiceUnavailable,
                message: e.to_string(),
            };

            let error_json = serde_json::to_string(&error_message)
                .map_err(|e| TunnelError::InvalidMessage(e.to_string()))?;

            outgoing_tx
                .send(WsMessage::Text(error_json.into()))
                .await
                .map_err(|e| TunnelError::WebSocketError(e.to_string()))?;
        }
    }

    Ok(())
}

/// Heartbeat task sends periodic ping messages
async fn spawn_heartbeat_task(
    outgoing_tx: mpsc::Sender<WsMessage>,
    interval: Duration,
) -> Result<()> {
    let mut ticker = tokio::time::interval(interval);

    loop {
        ticker.tick().await;

        let ping_message = Message::Ping;
        let ping_json = serde_json::to_string(&ping_message)
            .map_err(|e| TunnelError::InvalidMessage(e.to_string()))?;

        if let Err(e) = outgoing_tx.send(WsMessage::Text(ping_json.into())).await {
            error!("Failed to send heartbeat: {}", e);
            break;
        }

        debug!("Sent heartbeat");
    }

    debug!("Heartbeat task exiting");
    Ok(())
}

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
    info!("Tunnel endpoint: {}", args.endpoint);

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
            info!("Received Ctrl-C, shutting down gracefully...");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_args_without_token() {
        let args = Args {
            port: 8080,
            host: "localhost".to_string(),
            endpoint: "wss://example.com".to_string(),
            token: None,
            verbose: false,
            connect_timeout: 10,
            request_timeout: 25,
        };

        let config = Config::from_args(args);
        assert_eq!(config.local_address, "http://localhost:8080");
        assert_eq!(config.websocket_url, "wss://example.com");
        assert_eq!(config.connect_timeout, Duration::from_secs(10));
        assert_eq!(config.request_timeout, Duration::from_secs(25));
    }

    #[test]
    fn test_config_from_args_with_token() {
        let args = Args {
            port: 3000,
            host: "127.0.0.1".to_string(),
            endpoint: "wss://example.com".to_string(),
            token: Some("test_token_123".to_string()),
            verbose: true,
            connect_timeout: 15,
            request_timeout: 30,
        };

        let config = Config::from_args(args);
        assert_eq!(config.local_address, "http://127.0.0.1:3000");
        assert_eq!(config.websocket_url, "wss://example.com");
        assert_eq!(config.token, Some("test_token_123".to_string()));
        assert_eq!(config.connect_timeout, Duration::from_secs(15));
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(
            config.heartbeat_interval,
            Duration::from_secs(HEARTBEAT_INTERVAL_SECS)
        );
    }

    #[test]
    fn test_reconnect_config_defaults() {
        let args = Args {
            port: 3000,
            host: "127.0.0.1".to_string(),
            endpoint: "wss://example.com".to_string(),
            token: None,
            verbose: false,
            connect_timeout: 10,
            request_timeout: 25,
        };

        let config = Config::from_args(args);
        let reconnect = &config.reconnect_config;

        assert_eq!(
            reconnect.min_delay,
            Duration::from_millis(RECONNECT_MIN_DELAY_MS)
        );
        assert_eq!(
            reconnect.max_delay,
            Duration::from_millis(RECONNECT_MAX_DELAY_MS)
        );
        assert_eq!(reconnect.multiplier, RECONNECT_MULTIPLIER);
        assert_eq!(reconnect.max_attempts, None);
    }

    #[test]
    fn test_connection_state_variants() {
        let state = ConnectionState::Disconnected;
        assert!(matches!(state, ConnectionState::Disconnected));

        let state = ConnectionState::Connecting;
        assert!(matches!(state, ConnectionState::Connecting));

        let state = ConnectionState::Connected {
            connection_id: "test".to_string(),
            public_url: "https://test.example.com".to_string(),
        };
        assert!(matches!(state, ConnectionState::Connected { .. }));

        let state = ConnectionState::Reconnecting {
            attempt: 1,
            next_delay: Duration::from_secs(1),
        };
        assert!(matches!(state, ConnectionState::Reconnecting { .. }));
    }
}
