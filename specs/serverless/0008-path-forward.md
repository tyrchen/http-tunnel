# Path-Based Routing Solution

## Problem Statement

The current implementation uses subdomain-based routing (`https://zg2mltenpvlu.tunnel.example.com`), which requires DNS wildcard configuration for `*.tunnel.example.com`. Since you don't have control of wildcard DNS, we need to switch to path-based routing.

**Target URL Format**: `https://tunnel.example.com/zg2mltenpvlu/api/users`
- Base domain: `tunnel.example.com` (controlled)
- Tunnel ID: `zg2mltenpvlu` (random identifier)
- Actual path: `/api/users` (forwarded to local service)

## Current Architecture

```
Request: https://zg2mltenpvlu.tunnel.example.com/api/users
         ↓
HTTP API Gateway → ForwardingHandler Lambda
         ↓
extract_subdomain("zg2mltenpvlu.tunnel.example.com") → "zg2mltenpvlu"
         ↓
DynamoDB lookup via subdomain-index GSI
         ↓
Forward to agent via WebSocket
```

## New Architecture (Path-Based)

```
Request: https://tunnel.example.com/zg2mltenpvlu/api/users
         ↓
HTTP API Gateway → ForwardingHandler Lambda
         ↓
extract_tunnel_id_from_path("/zg2mltenpvlu/api/users") → "zg2mltenpvlu"
strip_tunnel_id_from_path("/zg2mltenpvlu/api/users") → "/api/users"
         ↓
DynamoDB lookup via tunnel-id-index GSI
         ↓
Forward "/api/users" to agent via WebSocket
```

## Implementation Plan

### 1. DynamoDB Changes

**Current Schema**:
- Primary Key: `connectionId` (String)
- GSI: `subdomain-index` on `publicSubdomain`
- Attributes: `publicUrl`, `publicSubdomain`, `createdAt`, `ttl`

**New Schema**:
- Primary Key: `connectionId` (String) - **unchanged**
- GSI: `tunnel-id-index` on `tunnelId` (replaces subdomain-index)
- Attributes:
  - `tunnelId` (String) - random identifier (same as current `publicSubdomain`)
  - `publicUrl` (String) - path-based URL: `https://tunnel.example.com/zg2mltenpvlu`
  - `createdAt` (Number)
  - `ttl` (Number)

### 2. Lambda Handler Changes

#### A. ConnectHandler (`apps/handler/src/handlers/connect.rs`)

**Current**:
```rust
let public_subdomain = generate_subdomain(); // e.g., "zg2mltenpvlu"
let public_url = format!("https://{}.{}", public_subdomain, domain);
// → https://zg2mltenpvlu.tunnel.example.com
```

**New**:
```rust
let tunnel_id = generate_subdomain(); // reuse same function, just rename
let public_url = format!("https://{}/{}", domain, tunnel_id);
// → https://tunnel.example.com/zg2mltenpvlu
```

**DynamoDB attributes**:
- Store `tunnelId` instead of `publicSubdomain`
- Update `ConnectionMetadata` struct in common crate

#### B. ForwardingHandler (`apps/handler/src/handlers/forwarding.rs`)

**Current**:
```rust
fn extract_subdomain(host: &str) -> Result<String> {
    let parts: Vec<&str> = host.split('.').collect();
    Ok(parts[0].to_string()) // Returns "zg2mltenpvlu"
}
```

**New**:
```rust
fn extract_tunnel_id_from_path(path: &str) -> Result<String> {
    // Path format: /zg2mltenpvlu/api/users
    // Extract: zg2mltenpvlu
    let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    if parts.is_empty() || parts[0].is_empty() {
        return Err(anyhow!("Missing tunnel ID in path"));
    }
    Ok(parts[0].to_string())
}

fn strip_tunnel_id_from_path(path: &str) -> String {
    // Path format: /zg2mltenpvlu/api/users
    // Return: /api/users
    let parts: Vec<&str> = path.trim_start_matches('/').splitn(2, '/').collect();
    if parts.len() > 1 {
        format!("/{}", parts[1])
    } else {
        "/".to_string()
    }
}
```

**Update request building**:
- Extract tunnel ID from path
- Strip tunnel ID from path before forwarding to local service
- Look up connection using new GSI

**Example**:
```
Incoming: GET /zg2mltenpvlu/api/users?id=123
Extract tunnel_id: "zg2mltenpvlu"
Stripped path: "/api/users?id=123"
Forward to agent: GET /api/users?id=123
```

#### C. ResponseHandler (`apps/handler/src/handlers/response.rs`)

**Changes**:
- Update DynamoDB query to use `tunnelId` instead of `publicSubdomain`
- Update ConnectionEstablished message to use new field name

### 3. Infrastructure Changes (`infra/src/dynamodb.ts`)

```typescript
const connectionsTable = new aws.dynamodb.Table("connections-table", {
  name: pulumi.interpolate`http-tunnel-connections-${tags.Environment}`,
  billingMode: "PAY_PER_REQUEST",
  hashKey: "connectionId",
  attributes: [
    { name: "connectionId", type: "S" },
    { name: "tunnelId", type: "S" },  // Changed from publicSubdomain
  ],
  globalSecondaryIndexes: [
    {
      name: "tunnel-id-index",  // Changed from subdomain-index
      hashKey: "tunnelId",      // Changed from publicSubdomain
      projectionType: "ALL",
    },
  ],
  ttl: {
    attributeName: "ttl",
    enabled: true,
  },
  tags: {
    ...tags,
    Name: "HTTP Tunnel Connections",
  },
});
```

### 4. Common Types Changes (`crates/common/src`)

**Update ConnectionMetadata**:
```rust
pub struct ConnectionMetadata {
    pub connection_id: String,
    pub tunnel_id: String,           // Renamed from public_subdomain
    pub public_url: String,          // Now path-based
    pub created_at: u64,
    pub ttl: u64,
    pub client_info: Option<ClientInfo>,
}
```

**Update Message::ConnectionEstablished**:
```rust
ConnectionEstablished {
    connection_id: String,
    tunnel_id: String,              // Renamed from public_subdomain
    public_url: String,
},
```

### 5. API Gateway HTTP API Configuration

**Current**: Uses custom domain `tunnel.example.com` ✓ (no change needed)

**Routing**: All requests to `https://tunnel.example.com/*` will hit the ForwardingHandler Lambda ✓

## Migration Strategy

Since this changes the DynamoDB schema, we have two options:

### Option A: Destructive Update (Recommended for Dev)
1. Delete existing DynamoDB table
2. Create new table with updated schema
3. All existing connections will be lost (acceptable for dev/testing)

### Option B: Blue-Green Migration
1. Create new GSI alongside existing one
2. Update code to write to both fields
3. Migrate existing data
4. Switch readers to new field
5. Remove old GSI

**Recommendation**: Use Option A for dev environment since this is still in testing phase.

## Testing Plan

1. **Start forwarder**: `ttf --endpoint wss://...`
   - Should display: `Tunnel established: https://tunnel.example.com/zg2mltenpvlu`

2. **Send test request**:
   ```bash
   curl https://tunnel.example.com/zg2mltenpvlu/
   ```
   - Should forward to `http://127.0.0.1:3000/`

3. **Send request with path**:
   ```bash
   curl https://tunnel.example.com/zg2mltenpvlu/api/todos
   ```
   - Should forward to `http://127.0.0.1:3000/api/todos`

4. **Verify path stripping**:
   - The local service should receive `/api/todos`, NOT `/zg2mltenpvlu/api/todos`

## Rollout Steps

1. ✅ Document solution (this file)
2. Update common crate types
3. Update DynamoDB infrastructure
4. Update ConnectHandler to generate path-based URLs
5. Update ForwardingHandler to extract tunnel ID from path and strip it
6. Update ResponseHandler to use new field names
7. Clean rebuild all components
8. Deploy infrastructure (will recreate DynamoDB table)
9. Deploy Lambda functions
10. Test complete flow

## Benefits of Path-Based Routing

1. **No DNS Wildcard Required**: Only need `tunnel.example.com` A/CNAME record
2. **Simpler DNS Management**: Single domain vs wildcard certificate
3. **Better URL Visibility**: User can see tunnel ID in path
4. **Standard HTTP Routing**: Works with all HTTP clients and proxies
5. **Future-Proof**: Easier to add features like `/tunnel-id/admin` for tunnel management

## Potential Issues & Solutions

**Issue**: Path collision if user requests `/zg2mltenpvlu/zg2mltenpvlu/foo`
**Solution**: Document that tunnel IDs are reserved in the first path segment. The actual user path starts from second segment.

**Issue**: Root path handling (`https://tunnel.example.com/zg2mltenpvlu`)
**Solution**: Strip tunnel ID → forward as `/` to local service

**Issue**: Query parameters
**Solution**: Preserve them when stripping path: `/zg2mltenpvlu/api?foo=bar` → `/api?foo=bar`
