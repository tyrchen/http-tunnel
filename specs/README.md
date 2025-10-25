# HTTP Tunnel Specifications

This directory contains the complete specification and implementation plan for the HTTP Tunnel service - a serverless, Rust-based alternative to ngrok built on AWS Lambda.

## Document Index

### 📋 [0001-idea.md](./0001-idea.md)
**Comprehensive Architecture Design Report**

The foundational document that describes the complete system architecture based on AWS serverless services. This 24-section document covers:
- The problem space and tunnel service fundamentals
- Why serverless and the architectural challenges
- The recommended API Gateway + Lambda + DynamoDB solution
- Complete data flow analysis for control and data planes
- Security considerations and alternative architectures
- Detailed comparison of serverless vs. container-based approaches

**Key Takeaway**: Use API Gateway WebSocket API as the stateful connection layer, Lambda for event-driven compute, and DynamoDB for state management.

---

### 🔧 [0002-common.md](./0002-common.md)
**Common Data Structures and Utilities**

Defines the shared Rust library (`crates/common`) used by both client and server:
- **Protocol**: Message envelope, HTTP request/response structures
- **Models**: Connection metadata, pending request tracking
- **Utilities**: ID generation, Base64 encoding, header conversion, timestamps
- **Error Types**: Comprehensive error handling with TunnelError enum
- **Constants**: Timeouts, intervals, size limits

**Implementation**: Phase 1 (1-2 days)

---

### 💻 [0003-forwarder.md](./0003-forwarder.md)
**Local Forwarder (Client Agent) Specification**

Detailed specification for the Rust CLI application that runs locally:
- **CLI Interface**: Argument parsing with clap
- **Connection Manager**: WebSocket connection with auto-reconnect
- **Message Handler**: Async message routing and processing
- **HTTP Request Handler**: Forward requests to local service using reqwest
- **Heartbeat Task**: Keep WebSocket alive with periodic pings
- **Error Handling**: Graceful degradation and reconnection logic

**Implementation**: Phase 2 (3-4 days)

---

### ☁️ [0004-lambda.md](./0004-lambda.md)
**Lambda Functions Specification**

Defines the four serverless backend functions:

1. **ConnectHandler** (`$connect` route)
   - Generate unique subdomain
   - Register connection in DynamoDB
   - Return connection metadata

2. **DisconnectHandler** (`$disconnect` route)
   - Clean up connection records
   - Automatic garbage collection

3. **ForwardingHandler** (HTTP API)
   - Receive public HTTP requests
   - Look up connection by subdomain
   - Forward to agent via WebSocket
   - Poll for response with timeout
   - Return response to caller

4. **ResponseHandler** (`$default` route)
   - Receive responses from agents
   - Update pending request status
   - Enable ForwardingHandler to complete

**Implementation**: Phase 3 (4-5 days)

---

### 🏗️ [0005-iac.md](./0005-iac.md)
**Infrastructure as Code with Pulumi**

Complete AWS infrastructure deployment using Pulumi (TypeScript):
- **DynamoDB**: Connections and pending requests tables with TTL
- **IAM**: Least-privilege roles for each Lambda function
- **Lambda**: Four function definitions with proper configuration
- **API Gateway**: WebSocket API for agents, HTTP API for public requests
- **Custom Domain**: Optional wildcard domain with ACM certificate
- **Deployment Process**: Build, package, and deploy workflow

**Implementation**: Phase 4 (3-4 days)

---

### 📝 [0006-implementation-plan.md](./0006-implementation-plan.md)
**Concrete Implementation Plan**

The master implementation guide that ties everything together:
- **Project Structure**: Complete directory layout
- **6 Implementation Phases**: Step-by-step task breakdown with acceptance criteria
- **Development Workflow**: Daily cycle, testing commands, deployment procedures
- **Risk Management**: Technical and operational risk mitigation
- **Success Metrics**: Functional, performance, and quality targets
- **Timeline**: 15-20 days total with parallel work opportunities

**Phases**:
1. Foundation - Common Library (2 days)
2. Local Forwarder (4 days)
3. Lambda Functions (5 days)
4. Infrastructure (4 days)
5. Integration Testing (3 days)
6. Documentation & Polish (2 days)

---

## Quick Start

### For Implementers

1. **Read in order**:
   - Start with `0001-idea.md` to understand the architecture
   - Review `0006-implementation-plan.md` for the execution strategy
   - Reference specific specs (`0002-0005`) as you implement each phase

2. **Begin implementation**:
   ```bash
   # Phase 1: Common library
   cd crates/common
   # Follow tasks in 0002-common.md

   # Phase 2: Forwarder
   cd apps/forwarder
   # Follow tasks in 0003-forwarder.md

   # Phase 3: Lambda functions
   cd apps/handler
   # Follow tasks in 0004-lambda.md

   # Phase 4: Infrastructure
   cd infra
   # Follow tasks in 0005-iac.md
   ```

3. **Track progress**:
   - Use checkboxes in `0006-implementation-plan.md`
   - Create GitHub issues for each phase
   - Update acceptance criteria as you complete tasks

### For Reviewers

1. **Architecture Review**: Read `0001-idea.md` sections 1-3 for high-level design
2. **Technical Review**: Review specific component specs (`0002-0005`)
3. **Feasibility Review**: Check `0006-implementation-plan.md` timeline and risks

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    Internet User/Service                     │
└────────────────────────────┬────────────────────────────────┘
                             │ HTTPS
                             ▼
                  ┌──────────────────────┐
                  │  API Gateway HTTP    │ ← Public endpoint
                  │  *.tunnel.domain.com │
                  └──────────┬───────────┘
                             │
                             ▼
                  ┌──────────────────────┐
                  │ ForwardingHandler    │ ← Lambda
                  │  - Lookup connection │
                  │  - PostToConnection  │
                  │  - Wait for response │
                  └──────────┬───────────┘
                             │
                ┌────────────┼────────────┐
                │            │            │
                ▼            ▼            ▼
         ┌──────────┐ ┌──────────┐ ┌──────────────┐
         │ DynamoDB │ │ DynamoDB │ │  WebSocket   │
         │  Conn    │ │ Pending  │ │  API Gateway │
         └──────────┘ └──────────┘ └──────┬───────┘
                                           │ WSS
                                           ▼
                              ┌────────────────────────┐
                              │  Local Forwarder Agent │
                              │  - Connection Manager  │
                              │  - Message Handler     │
                              │  - HTTP Client         │
                              └──────────┬─────────────┘
                                         │ HTTP
                                         ▼
                              ┌────────────────────────┐
                              │  Local Service         │
                              │  (localhost:3000)      │
                              └────────────────────────┘
```

---

## Key Design Decisions

### 1. **Serverless Architecture** ✅
- **Chosen**: API Gateway + Lambda + DynamoDB
- **Alternative Considered**: Container-based (Fargate)
- **Rationale**: Cost-effective for intermittent testing use, automatic scaling, zero operational overhead
- **Trade-off**: Higher complexity, cold starts, connection time limits

### 2. **WebSocket for Agent Connection** ✅
- **Chosen**: API Gateway WebSocket API
- **Alternative Considered**: HTTP/2 Server-Sent Events, polling
- **Rationale**: Built-in persistent connection management, event-driven Lambda triggers
- **Trade-off**: 10-minute idle timeout (mitigated with heartbeat), 2-hour max connection

### 3. **Request/Response Correlation via DynamoDB** ✅
- **Chosen**: Pending requests table with polling
- **Alternative Considered**: SQS, SNS, Step Functions
- **Rationale**: Simplest approach, fast key-value lookups
- **Trade-off**: Polling adds latency (100ms intervals)

### 4. **Rust for Performance** ✅
- **Chosen**: Rust for both client and Lambda functions
- **Alternative Considered**: Go, TypeScript
- **Rationale**: Fast cold starts, memory safety, excellent async support
- **Trade-off**: Longer development time, steeper learning curve

---

## Success Criteria

### Functional
- ✅ Agent connects to WebSocket API and receives subdomain
- ✅ HTTP requests to `https://<subdomain>.domain.com` are forwarded to local service
- ✅ Responses are returned to original caller
- ✅ Connection survives 10-minute idle periods (heartbeat)
- ✅ Automatic reconnection on network interruption

### Performance
- Added latency < 500ms (P95)
- Support 100+ concurrent agent connections
- Support 10+ concurrent requests per agent
- Lambda cold start < 1 second

### Quality
- Test coverage > 70%
- Zero compiler warnings
- All public APIs documented
- Comprehensive error handling

---

## Dependencies

### Development Tools
- Rust 1.70+ with cargo
- cargo-lambda for Lambda builds
- Node.js 18+ for Pulumi
- Pulumi CLI
- AWS CLI with configured credentials

### AWS Services
- API Gateway (WebSocket + HTTP)
- AWS Lambda
- Amazon DynamoDB
- Amazon Route 53 (for custom domain)
- AWS Certificate Manager (for TLS)
- CloudWatch (for logging)

### Rust Crates
- **Common**: serde, serde_json, tokio, base64, uuid, http
- **Forwarder**: tokio-tungstenite, reqwest, clap, tracing
- **Handler**: lambda_runtime, aws-lambda-events, aws-sdk-dynamodb, aws-sdk-apigatewaymanagement

---

## Cost Estimation (Monthly)

### Development/Testing (Low Traffic)
- API Gateway: ~$1-5 (few connections, few requests)
- Lambda: ~$0-2 (free tier covers most)
- DynamoDB: ~$0-1 (on-demand, low reads/writes)
- **Total**: ~$1-8/month

### Production (Moderate Traffic)
Assuming: 10 active tunnels, 1000 requests/day
- API Gateway: ~$50-100 (connection minutes + requests)
- Lambda: ~$20-40 (invocations + compute time)
- DynamoDB: ~$5-10 (on-demand reads/writes)
- **Total**: ~$75-150/month

**Cost Optimization**:
- Use ARM64 Lambda for 20% savings
- Enable DynamoDB TTL for automatic cleanup
- Use reserved capacity for predictable workloads (production)

---

## Security Considerations

### Authentication
- JWT-based authentication for agent connections
- Lambda Authorizer on `$connect` route
- Token passed as query parameter

### Network Security
- End-to-end TLS encryption (HTTPS + WSS)
- API Gateway with AWS WAF (optional)
- VPC endpoints for Lambda-DynamoDB traffic (optional)

### Access Control
- IAM least-privilege policies for each Lambda
- Connection records isolated by connection ID
- No cross-tenant data access

### Data Protection
- No persistent storage of request/response bodies
- Short TTL on pending requests (30 seconds)
- CloudWatch logs exclude sensitive data

---

## Monitoring and Operations

### Key Metrics
- WebSocket connection count
- HTTP request rate and latency
- Lambda errors and cold starts
- DynamoDB throttles

### CloudWatch Alarms
- Lambda error rate > 10/5min
- DynamoDB throttles > 5/5min
- API Gateway 5xx errors > 10/5min

### Logging
- Structured logging with tracing
- Request ID correlation across services
- Log retention: 7 days (dev), 30 days (prod)

---

## Next Steps

1. **Review**: Team reviews all specification documents
2. **Setup**: Configure development environment (Rust, Pulumi, AWS)
3. **Implement**: Follow Phase 1 → Phase 6 in `0006-implementation-plan.md`
4. **Test**: Comprehensive integration testing
5. **Deploy**: Production deployment with custom domain
6. **Monitor**: Set up CloudWatch dashboards and alarms

---

## Contributing

When implementing:
1. Follow the phase order in `0006-implementation-plan.md`
2. Reference the relevant spec document for details
3. Write tests as you go
4. Update checklists in implementation plan
5. Document any deviations from specs

---

## Questions or Issues?

- Architecture questions → Review `0001-idea.md`
- Implementation questions → Check specific component spec (`0002-0005`)
- Process questions → See `0006-implementation-plan.md`
- Open questions → Add to "Open Questions" section in `0006-implementation-plan.md`

---

**Last Updated**: 2025-10-24
**Status**: ✅ Specification Complete - Ready for Implementation
