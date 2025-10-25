# HTTP/2 and gRPC Support Analysis

## Executive Summary

**Current Architecture Verdict**: ❌ **Poor HTTP/2 support, Broken gRPC support**

The current architecture has fundamental limitations for HTTP/2 and gRPC:

1. **API Gateway HTTP API** doesn't support HTTP/2 for backend integrations
2. **Request serialization to JSON** breaks HTTP/2 framing and multiplexing
3. **No bidirectional streaming** - gRPC requires full-duplex communication
4. **No header frame preservation** - HTTP/2 headers are fundamentally different
5. **Polling-based response** adds unacceptable latency for streaming

## Detailed Problem Analysis

### 1. HTTP/2 Protocol Characteristics

HTTP/2 is fundamentally different from HTTP/1.1:

```
HTTP/1.1 Request:
GET /api/users HTTP/1.1
Host: example.com
[headers]
[body]

HTTP/2 Frames (binary):
HEADERS frame (stream 1)
  :method: GET
  :path: /api/users
  :scheme: https
  :authority: example.com
DATA frame (stream 1)
  [payload]
```

**Key HTTP/2 Features**:
- Binary framing protocol (not text)
- Multiplexing (multiple streams over one connection)
- Server push
- Header compression (HPACK)
- Stream prioritization
- Frame-level flow control

### 2. gRPC Protocol Requirements

gRPC builds on HTTP/2 and requires:

```
gRPC Request Flow:
1. HEADERS frame with:
   :method: POST
   :path: /package.Service/Method
   content-type: application/grpc
   grpc-encoding: gzip
   [custom metadata]

2. DATA frames (length-prefixed messages):
   [1-byte compressed flag][4-byte length][protobuf payload]
   ... multiple DATA frames for streaming ...

3. HEADERS frame (trailers):
   grpc-status: 0
   grpc-message: "OK"
```

**Critical gRPC Requirements**:
- Full HTTP/2 support
- Bidirectional streaming
- Trailer headers (sent after body)
- Length-prefixed message framing
- Custom metadata in headers
- Keep-alive pings
- Flow control

### 3. Current Architecture Limitations

#### Limitation 1: API Gateway HTTP API
```
Problem: API Gateway HTTP API only supports HTTP/1.1 backend integrations
Impact: Cannot forward HTTP/2 frames to Lambda
Workaround: None - architectural limitation
```

#### Limitation 2: Request Serialization
```rust
// Current approach - breaks HTTP/2
pub struct HttpRequest {
    pub method: String,        // ❌ Loses HTTP/2 pseudo-headers
    pub uri: String,
    pub headers: HashMap<String, Vec<String>>,  // ❌ No HPACK, no frame info
    pub body: String,          // ❌ Loses DATA frame boundaries
}
```

**What's Lost**:
- Stream IDs and multiplexing
- Frame types (HEADERS, DATA, SETTINGS, etc.)
- HTTP/2 pseudo-headers (`:method`, `:path`, `:scheme`, `:authority`)
- HPACK compression context
- Frame flags (END_STREAM, END_HEADERS, etc.)
- Priority and dependency information

#### Limitation 3: No Bidirectional Streaming
```
Current: Request → Wait → Response (half-duplex)

Required for gRPC:
Client → Server (streaming)
Server → Client (streaming)
Simultaneously (full-duplex)
```

The polling-based response mechanism cannot handle:
- Server sending multiple messages before client finishes
- Client sending multiple messages while server responds
- Interleaved bidirectional communication

#### Limitation 4: No Trailer Support
```
HTTP/1.1: Headers → Body (trailers rare)

HTTP/2/gRPC: Headers → Body → Trailers (common)
                              ↑
                         grpc-status
                         grpc-message
```

Current architecture has no concept of trailer headers.

#### Limitation 5: WebSocket Message Semantics
```
WebSocket: Message-oriented (complete messages)
HTTP/2: Stream-oriented (frames, flow control)

Impedance mismatch!
```

## Architectural Solutions

### Solution 1: Raw TCP Tunnel (Best for Full Protocol Support)

**Architecture**: Bypass API Gateway entirely, use raw TCP forwarding.

```
┌─────────────────────────────────────────────────────────┐
│                     Client (Internet)                    │
└────────────────────────┬────────────────────────────────┘
                         │ HTTP/2 or HTTP/1.1 (transparent)
                         ▼
              ┌──────────────────────┐
              │ NLB (Network LB)     │ ← TCP/TLS passthrough
              │ Static IP addresses  │
              └──────────┬───────────┘
                         │ Raw TCP
                         ▼
              ┌──────────────────────┐
              │ Fargate/ECS Service  │ ← Long-running proxy
              │ - TCP Proxy          │
              │ - Connection Registry│
              └──────────┬───────────┘
                         │ TCP tunnel
                         ▼
              ┌──────────────────────┐
              │ Local Agent          │
              │ - Raw TCP forwarding │
              └──────────┬───────────┘
                         │ Raw TCP
                         ▼
              ┌──────────────────────┐
              │ Local Service        │
              │ (HTTP/2, gRPC, etc.) │
              └──────────────────────┘
```

**Implementation**:
```rust
// Pure TCP forwarding - protocol-agnostic
async fn tunnel_connection(
    client: TcpStream,
    agent_connection: TcpStream,
) -> Result<()> {
    let (mut client_read, mut client_write) = client.split();
    let (mut agent_read, mut agent_write) = agent_connection.split();

    tokio::select! {
        result = tokio::io::copy(&mut client_read, &mut agent_write) => {
            result?;
        }
        result = tokio::io::copy(&mut agent_read, &mut client_write) => {
            result?;
        }
    }

    Ok(())
}
```

**Pros**:
- ✅ Full HTTP/2 support (transparent)
- ✅ Full gRPC support (all streaming modes)
- ✅ Protocol-agnostic (works with any TCP protocol)
- ✅ No serialization overhead
- ✅ Preserves all protocol semantics

**Cons**:
- ❌ Not serverless (must run always-on proxy)
- ❌ Fixed costs (Fargate tasks run continuously)
- ❌ More complex deployment
- ❌ Need Network Load Balancer
- ❌ No API Gateway benefits (throttling, auth, etc.)

**Cost**: ~$50-100/month for always-on Fargate tasks

---

### Solution 2: HTTP/2-Aware WebSocket Tunneling (Hybrid)

**Architecture**: Keep serverless, but make protocol-aware.

```
┌─────────────────────────────────────────────────────────┐
│                     Client (Internet)                    │
└────────────────────────┬────────────────────────────────┘
                         │ HTTP/2
                         ▼
              ┌──────────────────────┐
              │ ALB (Application LB) │ ← HTTP/2 → HTTP/1.1
              │ - Terminates HTTP/2  │   conversion
              └──────────┬───────────┘
                         │ HTTP/1.1
                         ▼
              ┌──────────────────────┐
              │ API Gateway HTTP API │
              └──────────┬───────────┘
                         │
                         ▼
              ┌──────────────────────┐
              │ ForwardingHandler    │ ← Enhanced for HTTP/2
              │ - Preserve h2 headers│
              │ - Stream management  │
              └──────────┬───────────┘
                         │ Enhanced WebSocket protocol
                         ▼
              ┌──────────────────────┐
              │ Local Agent          │
              │ - HTTP/2 client      │
              │ - Reconstruct frames │
              └──────────┬───────────┘
                         │ HTTP/2
                         ▼
              ┌──────────────────────┐
              │ Local Service        │
              │ (HTTP/2, gRPC)       │
              └──────────────────────┘
```

**Enhanced Message Protocol**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Http2Request {
    pub stream_id: u32,           // HTTP/2 stream ID
    pub request_id: String,        // Internal correlation

    // Pseudo-headers (HTTP/2 specific)
    pub method: String,            // :method
    pub path: String,              // :path
    pub scheme: String,            // :scheme (http or https)
    pub authority: String,         // :authority

    // Regular headers
    pub headers: Vec<(String, Vec<u8>)>,  // Raw bytes for HPACK

    // Body frames
    pub data_frames: Vec<DataFrame>,

    // Flags
    pub end_stream: bool,
    pub end_headers: bool,

    // Trailers (if any)
    pub trailers: Option<Vec<(String, Vec<u8>)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFrame {
    pub data: Vec<u8>,      // Base64 encoded in JSON
    pub end_stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Http2Message {
    RequestHeaders(Http2Request),
    RequestData { stream_id: u32, data: Vec<u8>, end_stream: bool },
    ResponseHeaders(Http2Response),
    ResponseData { stream_id: u32, data: Vec<u8>, end_stream: bool },
    ResetStream { stream_id: u32, error_code: u32 },
}
```

**Agent Implementation**:
```rust
use h2::client;

async fn handle_http2_request(
    request: Http2Request,
    local_uri: Uri,
) -> Result<()> {
    // Connect to local service with HTTP/2
    let tcp = TcpStream::connect(local_uri).await?;
    let (mut client, h2_conn) = client::handshake(tcp).await?;

    tokio::spawn(async move {
        h2_conn.await
    });

    // Build HTTP/2 request
    let mut req = http::Request::builder()
        .method(request.method.as_str())
        .uri(request.path)
        .version(http::Version::HTTP_2);

    // Add headers
    for (name, value) in request.headers {
        req = req.header(name, value);
    }

    let (response, mut send_stream) = client
        .send_request(req.body(()).unwrap(), request.end_stream)
        .await?;

    // Send data frames
    for frame in request.data_frames {
        send_stream.send_data(frame.data.into(), frame.end_stream)?;
    }

    // Handle response
    let (parts, mut body) = response.await?.into_parts();

    // Stream response back
    while let Some(chunk) = body.data().await {
        let chunk = chunk?;
        // Send back through WebSocket
        send_response_data(request.stream_id, chunk.to_vec()).await?;
    }

    // Handle trailers
    if let Some(trailers) = body.trailers().await? {
        send_response_trailers(request.stream_id, trailers).await?;
    }

    Ok(())
}
```

**Pros**:
- ✅ Serverless architecture maintained
- ✅ HTTP/2 features mostly preserved
- ✅ Lower cost than always-on
- ✅ gRPC unary and server-streaming supported

**Cons**:
- ⚠️ ALB terminates HTTP/2 (not true end-to-end)
- ❌ gRPC bidirectional streaming limited
- ❌ Complex message protocol
- ❌ Still has WebSocket message boundaries
- ⚠️ Latency from serialization

**Cost**: Similar to original (~$1-8/month dev, ~$75-150/month prod)

---

### Solution 3: WebSocket as Pure Byte Stream (Better for gRPC)

**Architecture**: Use WebSocket in binary mode as transparent byte pipe.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TunnelMessage {
    // Control plane
    Control(ControlMessage),

    // Data plane - raw bytes
    StreamData {
        connection_id: String,  // Which HTTP/2 connection
        data: Vec<u8>,          // Raw HTTP/2 frames (binary)
    },
}

// Agent reads raw HTTP/2 frames and forwards as-is
async fn tunnel_http2_connection(
    ws_stream: WebSocketStream,
    local_addr: SocketAddr,
) -> Result<()> {
    let local_tcp = TcpStream::connect(local_addr).await?;

    let (mut ws_read, mut ws_write) = ws_stream.split();
    let (mut tcp_read, mut tcp_write) = local_tcp.split();

    // Bidirectional forwarding
    tokio::spawn(async move {
        while let Some(msg) = ws_read.next().await {
            match msg? {
                Message::Binary(data) => {
                    tcp_write.write_all(&data).await?;
                }
                _ => {}
            }
        }
        Ok::<_, Error>(())
    });

    tokio::spawn(async move {
        let mut buffer = [0u8; 16384];
        loop {
            let n = tcp_read.read(&mut buffer).await?;
            if n == 0 { break; }
            ws_write.send(Message::Binary(buffer[..n].to_vec())).await?;
        }
        Ok::<_, Error>(())
    });

    Ok(())
}
```

**Pros**:
- ✅ Transparent HTTP/2 frame forwarding
- ✅ Full gRPC support (all modes)
- ✅ No protocol parsing overhead
- ✅ Serverless maintained

**Cons**:
- ❌ Still limited by API Gateway HTTP API (HTTP/1.1 only)
- ❌ WebSocket message boundaries may fragment frames
- ⚠️ Need careful buffer management

---

## Recommended Solution: Hybrid Approach

**For production-grade HTTP/2 and gRPC support, use a two-tier architecture:**

### Tier 1: HTTP/1.1 Tunneling (Serverless)
Use the current architecture for HTTP/1.1 traffic:
- API Gateway + Lambda for HTTP/1.1
- Cost-effective for simple use cases
- Full serverless benefits

### Tier 2: Raw TCP Tunneling (Fargate)
Use TCP proxy for HTTP/2 and gRPC:
- Network Load Balancer + Fargate
- Full protocol support
- Always-on, but can scale to zero instances when not needed

### Hybrid Architecture

```
                    ┌─────────────────┐
                    │   Route 53      │
                    │  DNS Routing    │
                    └────────┬────────┘
                             │
              ┌──────────────┴──────────────┐
              │                             │
              ▼                             ▼
   ┌─────────────────────┐      ┌─────────────────────┐
   │ *.h1.tunnel.com     │      │ *.h2.tunnel.com     │
   │ (HTTP/1.1 traffic)  │      │ (HTTP/2/gRPC)       │
   └──────────┬──────────┘      └──────────┬──────────┘
              │                             │
              ▼                             ▼
   ┌─────────────────────┐      ┌─────────────────────┐
   │ API Gateway HTTP    │      │ Network LB          │
   │ + Lambda            │      │ + Fargate Proxy     │
   │ (Serverless)        │      │ (Always-on)         │
   └─────────────────────┘      └─────────────────────┘
```

**CLI Usage**:
```bash
# For HTTP/1.1 services (default)
forwarder --port 8080
# → https://abc123.h1.tunnel.example.com

# For HTTP/2 / gRPC services
forwarder --port 50051 --protocol h2
# → https://abc123.h2.tunnel.example.com
```

**Benefits**:
- ✅ Best of both worlds
- ✅ Cost-effective for most use cases (HTTP/1.1)
- ✅ Full protocol support when needed (HTTP/2/gRPC)
- ✅ User chooses based on need

**Costs**:
- HTTP/1.1 tier: ~$1-8/month (serverless)
- HTTP/2 tier: ~$50-100/month (Fargate, but shared across users)

---

## Implementation Recommendations

### Short Term (MVP)
**Focus on HTTP/1.1 + Basic HTTP/2**:
1. Implement current architecture (0002-0006 specs)
2. Document HTTP/2 limitations clearly
3. Add ALB in front of API Gateway for HTTP/2 → HTTP/1.1 conversion
4. Support gRPC unary calls (request/response)

**Changes Needed**:
```rust
// In common/src/protocol/request.rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    // ... existing fields ...

    // HTTP/2 additions
    #[serde(default)]
    pub http_version: HttpVersion,

    #[serde(default)]
    pub pseudo_headers: Option<PseudoHeaders>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HttpVersion {
    Http11,
    Http2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PseudoHeaders {
    pub method: String,    // :method
    pub path: String,      // :path
    pub scheme: String,    // :scheme
    pub authority: String, // :authority
}
```

### Long Term (Production)
**Add Fargate TCP Proxy Tier**:
1. Implement raw TCP tunneling in Fargate
2. Use Network Load Balancer
3. Add separate subdomain for HTTP/2 traffic
4. Support all gRPC streaming modes

**New Components**:
```
apps/
├── forwarder/        # Existing (add --protocol flag)
├── handler/          # Existing (HTTP/1.1 tier)
└── proxy/            # NEW: TCP proxy for HTTP/2
    ├── src/
    │   ├── main.rs
    │   ├── tcp_proxy.rs
    │   └── connection_registry.rs
    └── Cargo.toml

infra/
└── src/
    ├── nlb.ts        # NEW: Network Load Balancer
    ├── fargate.ts    # NEW: Fargate service for proxy
    └── ... existing ...
```

---

## Testing Strategy

### HTTP/2 Testing
```bash
# Test with curl (HTTP/2)
curl --http2 https://abc123.h2.tunnel.example.com/api

# Test with h2load (stress test)
h2load -n 1000 -c 10 https://abc123.h2.tunnel.example.com/
```

### gRPC Testing
```bash
# Test with grpcurl
grpcurl -plaintext abc123.h2.tunnel.example.com:443 \
  package.Service/Method

# Test streaming
grpcurl -d '{"name": "test"}' \
  abc123.h2.tunnel.example.com:443 \
  package.Service/StreamMethod
```

---

## Conclusion

### Current Architecture (0002-0006)
- ✅ Excellent for HTTP/1.1
- ⚠️ Limited HTTP/2 support (via ALB downgrade)
- ❌ Poor gRPC support (unary only, high latency)

### Recommended Path Forward

**Phase 1 (0-2 months)**:
- Implement current specs with HTTP/1.1 focus
- Add basic HTTP/2 via ALB downgrade
- Document limitations

**Phase 2 (2-4 months)**:
- Add Fargate TCP proxy tier
- Support full HTTP/2 and gRPC
- Offer both tiers to users

**Alternative**: If gRPC is a hard requirement from day 1, skip serverless entirely and go with Fargate TCP proxy from the start.

### Decision Matrix

| Use Case | Solution | Cost | Effort |
|----------|----------|------|--------|
| HTTP/1.1 only | Current (serverless) | Low | Low |
| HTTP/2 (no streaming) | Current + ALB | Low | Low |
| gRPC unary | Current + ALB | Low | Medium |
| gRPC streaming | Fargate TCP proxy | Medium | High |
| Full protocol support | Fargate TCP proxy | Medium | High |

**My Recommendation**: Start with current architecture (excellent for 90% of use cases), add Fargate tier if gRPC streaming becomes essential.
