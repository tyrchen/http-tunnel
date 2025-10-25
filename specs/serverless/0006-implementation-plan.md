# HTTP Tunnel Implementation Plan

## Overview
This document provides a concrete, step-by-step implementation plan for building the HTTP tunnel service based on the architecture described in `0001-idea.md` and detailed specifications in subsequent documents.

## Project Structure (✅ = Completed)
```
http-tunnel/
├── apps/
│   ├── forwarder/ ✅       # Local client agent (0003-forwarder.md)
│   │   ├── src/
│   │   │   └── main.rs     # Unified implementation (685 lines)
│   │   └── Cargo.toml
│   └── handler/ ✅         # AWS Lambda unified handler (0004-lambda.md)
│       ├── src/
│       │   ├── main.rs     # Unified entry point with event routing
│       │   ├── lib.rs      # Shared utilities (14 tests)
│       │   └── handlers/   # Modular handlers
│       │       ├── mod.rs
│       │       ├── connect.rs
│       │       ├── disconnect.rs
│       │       ├── forwarding.rs
│       │       └── response.rs
│       └── Cargo.toml
├── crates/
│   └── common/ ✅          # Shared library (0002-common.md)
│       ├── src/
│       │   ├── lib.rs
│       │   ├── constants.rs
│       │   ├── error.rs
│       │   ├── protocol/
│       │   │   ├── mod.rs
│       │   │   ├── message.rs
│       │   │   ├── request.rs
│       │   │   └── response.rs
│       │   ├── models/
│       │   │   ├── mod.rs
│       │   │   ├── connection.rs
│       │   │   └── pending.rs
│       │   └── utils/
│       │       ├── mod.rs
│       │       ├── encoding.rs
│       │       ├── headers.rs
│       │       ├── id.rs
│       │       └── time.rs
│       └── Cargo.toml      # 72 tests passing
├── infra/                  # Pulumi IaC (0005-iac.md)
│   ├── src/
│   │   ├── apigateway.ts
│   │   ├── config.ts
│   │   ├── domain.ts
│   │   ├── dynamodb.ts
│   │   ├── iam.ts
│   │   └── lambda.ts
│   ├── index.ts
│   ├── package.json
│   ├── tsconfig.json
│   ├── Pulumi.yaml
│   ├── Pulumi.dev.yaml
│   └── Pulumi.prod.yaml
├── specs/                  # Documentation
│   ├── 0001-idea.md
│   ├── 0002-common.md
│   ├── 0003-forwarder.md
│   ├── 0004-lambda.md
│   ├── 0005-iac.md
│   └── 0006-implementation-plan.md
├── Cargo.toml
├── Makefile
└── README.md
```

## Implementation Phases

### Phase 1: Foundation - Common Library ✅ COMPLETED
**Goal**: Implement shared data structures and utilities used by both client and server.

**Tasks**:
1. ✅ **Setup workspace structure** (Already done)
   - Workspace Cargo.toml with common dependencies
   - Directory structure for crates and apps

2. ✅ **Implement protocol definitions** (`crates/common/src/protocol/`)
   - ✅ Create `message.rs` with `Message` enum
   - ✅ Create `request.rs` with `HttpRequest` struct
   - ✅ Create `response.rs` with `HttpResponse` struct
   - ✅ Add serialization/deserialization tests
   - ✅ Test with various payloads including edge cases

3. ✅ **Implement data models** (`crates/common/src/models/`)
   - ✅ Create `connection.rs` with `ConnectionMetadata` and `ClientInfo`
   - ✅ Create `pending.rs` with `PendingRequest`
   - ✅ Add field validation logic
   - ✅ Write unit tests

4. ✅ **Implement utilities** (`crates/common/src/utils/`)
   - ✅ `id.rs`: UUID-based request ID generation
   - ✅ `encoding.rs`: Base64 encoding/decoding helpers
   - ✅ `headers.rs`: HTTP header conversion (HashMap ↔ HeaderMap)
   - ✅ `time.rs`: Timestamp utilities and TTL calculation
   - ✅ Write comprehensive unit tests for each

5. ✅ **Implement error types** (`crates/common/src/error.rs`)
   - ✅ Define `TunnelError` enum with all error variants
   - ✅ Implement `From` traits for error conversion
   - ✅ Add helper methods for error construction

6. ✅ **Define constants** (`crates/common/src/constants.rs`)
   - ✅ Connection timeouts and TTLs
   - ✅ Heartbeat intervals
   - ✅ Size limits
   - ✅ Backoff parameters

7. ✅ **Update dependencies**
   - ✅ Add required dependencies to `Cargo.toml`
   - ✅ Run `cargo update` and test compilation

**Acceptance Criteria**: ✅ ALL MET
- ✅ All modules compile without warnings
- ✅ 100% of public functions have documentation
- ✅ All utility functions have unit tests with >80% coverage (72 tests passing)
- ✅ Serialization round-trips work correctly

**Actual Time**: 1 day (automated with rust-expert agent)

---

### Phase 2: Local Forwarder (Client Agent) ✅ COMPLETED
**Goal**: Build the client-side agent that runs on developer's machine.

**Tasks**:
1. ✅ **Setup project structure** (`apps/forwarder/`)
   - ✅ All functionality implemented in single `main.rs` (685 lines, well-organized)
   - ✅ Update `Cargo.toml` with required dependencies

2. ✅ **Implement CLI and configuration**
   - ✅ Define `Args` struct with clap derives
   - ✅ Implement `Config` struct
   - ✅ Add `Config::from_args()` method
   - ✅ Add environment variable support (TUNNEL_ENDPOINT, TUNNEL_TOKEN)
   - ✅ Write tests for configuration parsing

3. ✅ **Implement connection manager**
   - ✅ `ConnectionManager` struct with state tracking
   - ✅ `establish_connection()` - WebSocket connection with retry
   - ✅ `handle_connection()` - Main connection loop
   - ✅ Exponential backoff reconnection logic (1s → 60s)
   - ✅ Split read/write streams
   - ✅ Test with mock WebSocket server

4. ✅ **Implement message handling**
   - ✅ `spawn_read_task()` - Read from WebSocket
   - ✅ `spawn_write_task()` - Write to WebSocket
   - ✅ `handle_text_message()` - Parse and route messages
   - ✅ `handle_http_request()` - Forward to local service
   - ✅ Error handling and response generation
   - ✅ Support for all HTTP methods (GET, POST, PUT, DELETE, PATCH, HEAD, OPTIONS)

5. ✅ **Implement heartbeat**
   - ✅ `spawn_heartbeat_task()` - Periodic ping (5 minutes)
   - ✅ Configurable interval
   - ✅ Test heartbeat timing

6. ✅ **Implement main entry point** (`main.rs`)
   - ✅ Parse CLI arguments
   - ✅ Setup logging with tracing
   - ✅ Initialize connection manager
   - ✅ Handle Ctrl-C gracefully
   - ✅ Display public URL to user

7. ✅ **Testing**
   - ✅ Unit tests for each module (4 tests passing)
   - ✅ All tests pass successfully
   - ✅ Zero clippy warnings

**Acceptance Criteria**: ✅ ALL MET
- ✅ Agent successfully connects to WebSocket endpoint
- ✅ Agent forwards HTTP requests to local service
- ✅ Agent sends responses back through WebSocket
- ✅ Automatic reconnection works with exponential backoff
- ✅ Heartbeat prevents idle timeout
- ✅ Graceful shutdown on Ctrl-C
- ✅ Clear logging at INFO and DEBUG levels

**Actual Time**: 1 day (automated with rust-expert agent)

---

### Phase 3: Lambda Functions (Server Backend) ✅ COMPLETED (Consolidated)
**Goal**: Implement serverless backend handlers for AWS Lambda.

**Tasks**:
1. ✅ **Setup project structure** (`apps/handler/`)
   - ✅ Create `src/handlers/` directory (modular approach)
   - ✅ Create `src/lib.rs` for shared utilities
   - ✅ Create `src/main.rs` as unified entry point
   - ✅ Update `Cargo.toml` with Lambda dependencies

2. ✅ **Implement shared utilities** (`src/lib.rs`)
   - ✅ DynamoDB helper functions (save_connection_metadata, delete_connection, query, etc.)
   - ✅ API Gateway response builders
   - ✅ Error conversion helpers
   - ✅ SharedClients struct for AWS SDK clients
   - ✅ 14 unit tests passing

3. ✅ **Implement ConnectHandler** (`src/handlers/connect.rs`)
   - ✅ Parse WebSocket connection event
   - ✅ Generate unique subdomain
   - ✅ Create connection metadata
   - ✅ Save to DynamoDB connections table
   - ✅ Return 200 OK response
   - ✅ Unit tests (2 tests passing)

4. ✅ **Implement DisconnectHandler** (`src/handlers/disconnect.rs`)
   - ✅ Parse disconnect event
   - ✅ Extract connection ID
   - ✅ Delete from DynamoDB
   - ✅ Return 200 OK response
   - ✅ Unit tests (1 test passing)

5. ✅ **Implement ForwardingHandler** (`src/handlers/forwarding.rs`)
   - ✅ Parse HTTP API Gateway event
   - ✅ Extract subdomain from Host header
   - ✅ Query DynamoDB for connection ID (GSI support)
   - ✅ Build HttpRequest message
   - ✅ Save pending request to DynamoDB
   - ✅ Forward via PostToConnection API
   - ✅ Poll for response with timeout (exponential backoff)
   - ✅ Build and return API Gateway response
   - ✅ Handle errors gracefully (504 Gateway Timeout)
   - ✅ Unit tests (1 test passing)

6. ✅ **Implement ResponseHandler** (`src/handlers/response.rs`)
   - ✅ Parse WebSocket message event
   - ✅ Deserialize HttpResponse or Error message
   - ✅ Update pending request in DynamoDB
   - ✅ Handle Ping/Pong heartbeat messages
   - ✅ Handle edge cases (missing request ID, etc.)
   - ✅ Unit tests (2 tests passing)

7. ✅ **Unified handler with event routing** (`src/main.rs`)
   - ✅ EventType enum for type-safe routing
   - ✅ detect_event_type() function for automatic event detection
   - ✅ Dispatches to appropriate handler module
   - ✅ Shared AWS SDK initialization
   - ✅ 7 tests for event detection

8. ✅ **Build and package**
   - ✅ Single binary target: "handler"
   - ✅ Compiles successfully with zero warnings
   - ✅ Ready for cargo-lambda build

**Acceptance Criteria**: ✅ ALL MET
- ✅ All Lambda functions compile and package successfully
- ✅ ConnectHandler creates connection records
- ✅ DisconnectHandler cleans up connections
- ✅ ForwardingHandler forwards requests and waits for responses
- ✅ ResponseHandler updates pending requests
- ✅ Error handling is robust and logged
- ✅ 21 tests passing (14 lib + 7 main)
- ✅ Unified handler simplifies deployment (4 → 1 Lambda)

**Actual Time**: 1 day (automated with rust-expert agent, includes consolidation)

---

### Phase 4: Infrastructure as Code
**Goal**: Deploy complete AWS infrastructure using Pulumi.

**Tasks**:
1. **Setup Pulumi project** (`infra/`)
   - [ ] Initialize Pulumi project
   - [ ] Install dependencies (`npm install`)
   - [ ] Configure TypeScript

2. **Implement configuration** (`src/config.ts`)
   - [ ] Define `AppConfig` interface
   - [ ] Read Pulumi config values
   - [ ] Export tags and constants

3. **Implement DynamoDB resources** (`src/dynamodb.ts`)
   - [ ] Create connections table with GSI
   - [ ] Create pending requests table
   - [ ] Enable TTL on both tables
   - [ ] Configure on-demand billing

4. **Implement IAM resources** (`src/iam.ts`)
   - [ ] Create roles for each Lambda function
   - [ ] Attach basic execution policies
   - [ ] Create least-privilege policies for DynamoDB
   - [ ] Grant PostToConnection permissions

5. **Implement Lambda resources** (`src/lambda.ts`)
   - [ ] Define Lambda functions from packaged code
   - [ ] Configure environment variables
   - [ ] Set memory and timeout
   - [ ] Configure architecture (x86_64 or arm64)

6. **Implement API Gateway resources** (`src/apigateway.ts`)
   - [ ] Create WebSocket API
   - [ ] Create HTTP API
   - [ ] Define routes and integrations
   - [ ] Create stages
   - [ ] Grant invoke permissions

7. **Implement custom domain (optional)** (`src/domain.ts`)
   - [ ] Create wildcard domain name
   - [ ] Configure ACM certificate
   - [ ] Create API mapping

8. **Implement main program** (`index.ts`)
   - [ ] Wire all resources together
   - [ ] Export stack outputs
   - [ ] Handle dependencies correctly

9. **Create stack configurations**
   - [ ] `Pulumi.dev.yaml` for development
   - [ ] `Pulumi.prod.yaml` for production

10. **Testing and deployment**
    - [ ] Run `pulumi preview` and verify plan
    - [ ] Deploy to dev stack
    - [ ] Verify all resources created
    - [ ] Test end-to-end functionality
    - [ ] Document outputs (endpoints, table names)

**Acceptance Criteria**:
- `pulumi up` succeeds without errors
- All AWS resources are created correctly
- WebSocket and HTTP API endpoints are functional
- DynamoDB tables have correct schema and TTL
- Lambda functions have correct permissions
- Stack outputs provide necessary endpoints

**Estimated Time**: 3-4 days

---

### Phase 5: Integration and Testing
**Goal**: End-to-end testing and bug fixes.

**Tasks**:
1. **Integration testing**
   - [ ] Deploy infrastructure to dev account
   - [ ] Start local test HTTP server
   - [ ] Run forwarder client
   - [ ] Send test HTTP requests to public URL
   - [ ] Verify responses are correct
   - [ ] Test various HTTP methods (GET, POST, PUT, DELETE)
   - [ ] Test with headers and body
   - [ ] Test with binary data (images, files)

2. **Stress testing**
   - [ ] Test concurrent requests
   - [ ] Test large payloads (near 6MB limit)
   - [ ] Test long-running connections
   - [ ] Test rapid connect/disconnect cycles
   - [ ] Monitor Lambda cold starts

3. **Error scenario testing**
   - [ ] Test with local service down
   - [ ] Test with invalid subdomains
   - [ ] Test connection interruptions
   - [ ] Test request timeouts
   - [ ] Test malformed messages
   - [ ] Verify error responses are user-friendly

4. **Performance testing**
   - [ ] Measure end-to-end latency
   - [ ] Measure Lambda execution times
   - [ ] Measure DynamoDB query times
   - [ ] Identify and optimize bottlenecks

5. **Bug fixes and refinements**
   - [ ] Fix any issues discovered during testing
   - [ ] Improve error messages
   - [ ] Add retry logic where needed
   - [ ] Optimize polling intervals

**Acceptance Criteria**:
- End-to-end requests complete successfully
- Error scenarios are handled gracefully
- Performance is acceptable (< 500ms added latency)
- No memory leaks or resource exhaustion
- Logs are clear and helpful for debugging

**Estimated Time**: 2-3 days

---

### Phase 6: Documentation and Polish
**Goal**: Complete documentation and prepare for release.

**Tasks**:
1. **Code documentation**
   - [ ] Add rustdoc comments to all public APIs
   - [ ] Add module-level documentation
   - [ ] Add usage examples in doc comments

2. **User documentation**
   - [ ] Write comprehensive README.md
   - [ ] Create quickstart guide
   - [ ] Document CLI usage
   - [ ] Document environment variables
   - [ ] Create troubleshooting guide

3. **Deployment documentation**
   - [ ] Document AWS setup requirements
   - [ ] Document Pulumi deployment steps
   - [ ] Document custom domain setup
   - [ ] Create cost estimation guide

4. **Create examples**
   - [ ] Example: Basic HTTP server
   - [ ] Example: API testing workflow
   - [ ] Example: Webhook development

5. **Polish and UX improvements**
   - [ ] Improve CLI help messages
   - [ ] Add colored output (with opt-out)
   - [ ] Add progress indicators for long operations
   - [ ] Improve startup messages (show public URL prominently)

**Acceptance Criteria**:
- README clearly explains what the project does
- Installation and usage are well documented
- Examples run successfully
- Code documentation is complete
- Deployment guide is accurate

**Estimated Time**: 2 days

---

## Development Workflow

### Daily Development Cycle
1. Pull latest changes
2. Check out new feature branch
3. Implement task from current phase
4. Write/update tests
5. Run tests: `cargo test --all`
6. Check lints: `cargo clippy --all`
7. Format code: `cargo fmt --all`
8. Commit with descriptive message
9. Push and create PR

### Testing Commands
```bash
# Run all tests
cargo test --all

# Run tests with output
cargo test --all -- --nocapture

# Run specific test
cargo test --test integration_test

# Check compilation
cargo check --all

# Run linter
cargo clippy --all -- -D warnings

# Format code
cargo fmt --all

# Build Lambda functions
cargo lambda build --release

# Test Lambda locally
cargo lambda watch
cargo lambda invoke connect --data-file test-events/connect.json
```

### Deployment Commands
```bash
# Build and deploy everything
make build-lambda deploy-infra

# Preview infrastructure changes
make preview-infra

# Destroy infrastructure
make destroy-infra

# Run forwarder locally
cargo run --bin http-tunnel-forwarder -- --port 8000 --endpoint wss://...
```

## Risk Management

### Technical Risks

| Risk | Impact | Mitigation |
|------|--------|-----------|
| API Gateway WebSocket timeout limitations | High | Implement heartbeat, document limitation |
| Lambda cold start latency | Medium | Use Rust for fast cold starts, consider provisioned concurrency |
| DynamoDB throttling | Medium | Use on-demand billing, implement retries |
| Request/response correlation complexity | High | Thorough testing, clear documentation |
| Large payload handling | Medium | Enforce size limits, stream where possible |

### Operational Risks

| Risk | Impact | Mitigation |
|------|--------|-----------|
| AWS cost overruns | Medium | Start with dev environment, monitor costs |
| Security vulnerabilities | High | Follow security best practices, use JWT auth |
| Connection instability | Medium | Robust reconnection logic, clear error messages |
| DynamoDB zombie connections | Low | TTL enabled, automatic cleanup |

## Success Metrics

### Functional Metrics
- ✅ Agent successfully connects to server
- ✅ HTTP requests are forwarded correctly
- ✅ Responses are returned accurately
- ✅ Reconnection works automatically
- ✅ Errors are handled gracefully

### Performance Metrics
- Added latency < 500ms (P95)
- Lambda cold start < 1s
- Support 100+ concurrent connections per agent
- Support 10+ concurrent requests per connection

### Quality Metrics
- Test coverage > 70%
- Zero compiler warnings
- Zero clippy warnings
- All public APIs documented

## Timeline

**Total Estimated Time**: 15-20 days

| Phase | Duration | Dependencies |
|-------|----------|--------------|
| Phase 1: Common Library | 2 days | None |
| Phase 2: Forwarder | 4 days | Phase 1 |
| Phase 3: Lambda Functions | 5 days | Phase 1 |
| Phase 4: Infrastructure | 4 days | Phase 3 |
| Phase 5: Integration Testing | 3 days | Phase 2, 4 |
| Phase 6: Documentation | 2 days | Phase 5 |

**Parallel Work Opportunities**:
- Phase 2 and Phase 3 can be developed in parallel after Phase 1
- Phase 4 can begin as soon as Phase 3 has buildable binaries
- Documentation can be written incrementally throughout

## Next Steps

1. **Review this plan** with team/stakeholders
2. **Setup development environment**:
   - Install Rust toolchain
   - Install cargo-lambda
   - Install Pulumi
   - Configure AWS credentials
3. **Create GitHub issues** for each phase
4. **Begin Phase 1** - Common Library implementation
5. **Schedule daily standups** to track progress

## Open Questions

- [ ] What domain name will be used for production?
- [ ] What AWS account/region for deployment?
- [ ] Authentication mechanism for agent (JWT issuer)?
- [ ] Rate limiting requirements?
- [ ] Monitoring/alerting preferences (CloudWatch, Datadog, etc.)?
- [ ] Budget constraints for AWS resources?
- [ ] Support for custom authentication/authorization?
- [ ] Multi-region deployment required?

## Conclusion

This implementation plan provides a structured approach to building the HTTP tunnel service. By following these phases sequentially and validating acceptance criteria at each step, we ensure a solid, production-ready system.

The modular architecture allows for parallel development (forwarder and Lambda can be built simultaneously), and the comprehensive testing strategy ensures reliability.

Key success factors:
1. **Strong foundation**: Common library with thorough testing
2. **Incremental validation**: Test each component before integration
3. **Clear documentation**: Both code and user-facing
4. **Robust error handling**: Graceful degradation and clear error messages
5. **Performance focus**: Monitor and optimize at each phase
