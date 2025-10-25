# HTTP Tunnel Documentation

## Technical Blog Posts

This directory contains comprehensive technical documentation about building and debugging a Serverless HTTP tunnel using AWS Lambda, API Gateway WebSocket, and DynamoDB.

### Available Documents

#### 1. [Solving the Path Rewriting Challenge in HTTP Tunnels](./content-rewriting-for-path-based-routing.md) (English)

A comprehensive guide to implementing transparent content rewriting for path-based HTTP tunnels:

- **The Challenge**: How absolute paths in HTML/CSS/JavaScript break tunneled connections
- **Three-Pronged Solution**: Static rewriting, JavaScript literals, context injection
- **Implementation Details**: Rust regex patterns, lazy compilation, edge case handling
- **Real-World Testing**: Swagger UI and SPA compatibility
- **Performance Optimization**: Zero-allocation patterns, idempotent operations
- **30 Unit Tests**: Comprehensive coverage of all rewriting scenarios

**Key Sections**:
- Content-Type Detection Strategy
- HTML Attribute Rewriting (href, src, action)
- CSS url() Rewriting (three quote styles)
- Conservative JavaScript String Rewriting
- Tunnel Context Injection for Dynamic URLs
- Performance: Lazy Regex Compilation

#### 2. [Fixing WebSocket Dispatch Failures in AWS API Gateway](./fixing-websocket-dispatch-failures.md) (English)

Deep dive into solving race conditions when sending messages over AWS WebSocket connections:

- **The Problem**: "dispatch failure" errors after connection establishment
- **Root Cause**: Connection state transition timing (PENDING → ACTIVE)
- **The Solution**: Exponential backoff retry logic (100ms, 200ms, 400ms)
- **Impact**: 75% reduction in Lambda invocations, 100x faster connections
- **Production Metrics**: 94% first-attempt success rate
- **Best Practices**: When and how to implement retries for PostToConnection

**Key Sections**:
- WebSocket Connection Lifecycle Analysis
- Race Condition Diagnosis with Logs
- Retry Logic Implementation in Rust
- Alternative Solutions Evaluated
- Cost Impact Analysis (before/after)
- Generic Retry Helper Pattern

#### 3. [Debugging a Serverless WebSocket HTTP Tunnel](./debugging-serverless-websocket-tunnel.md) (English)

A deep technical dive into the complete debugging journey, covering:

- **7 Critical Issues Resolved**: From Lambda permissions to event routing
- **Systematic Debugging Methodology**: How to diagnose serverless issues
- **AWS WebSocket API Deep Dive**: Understanding $connect, PostToConnection, and lifecycle
- **Path-Based Routing Migration**: From subdomain to path-based URLs
- **Production Architecture**: Complete system design with Mermaid diagrams
- **Performance & Cost Analysis**: Real-world metrics and pricing breakdown

**Key Sections**:
- Lambda Permission Misconfiguration (SourceArn format)
- WebSocket Connection Handshake Protocol Design
- Event Type Detection Logic (HTTP vs WebSocket)
- HTTP API Payload Format Versioning
- DynamoDB GSI Migration Strategy

#### 4. [调试 Serverless WebSocket HTTP 隧道：深度技术剖析](./debugging-serverless-websocket-tunnel-zh.md) (中文)

English content translated into idiomatic Chinese with proper technical terminology.

**核心内容**:
- 系统性调试方法论
- AWS 服务行为深入理解
- 基于路径的路由迁移方案
- 完整架构设计与数据流分析
- 性能特征与成本分析

### Who Should Read This?

- **Backend Engineers** building serverless WebSocket applications
- **DevOps Engineers** debugging AWS Lambda and API Gateway issues
- **Solution Architects** designing HTTP tunnel or proxy services
- **Rust Developers** working with AWS SDK and Lambda runtime

### Key Takeaways

1. **No Lambda Logs = Permission Issue**: When Lambda isn't invoked, check IAM policies and SourceArn format first
2. **WebSocket Lifecycle Matters**: Connection IDs aren't "active" until after $connect handler returns - implement retry logic
3. **Event Detection Order**: Check most specific fields first when multiple event types share common fields
4. **API Versioning**: Match payload format versions between infrastructure config and code
5. **Path-Based Routing**: Simpler DNS management than wildcard subdomains
6. **Content Rewriting is Essential**: Path-based routing requires transparent URL rewriting for real-world apps
7. **Retry with Exponential Backoff**: Handle WebSocket dispatch failures with 100ms/200ms/400ms retries
8. **Conservative JavaScript Rewriting**: Only rewrite obvious patterns, provide context API for dynamic URLs

### Architecture Patterns Demonstrated

- **Unified Lambda Handler**: Single function handling multiple event types
- **Ready/Response Handshake**: Post-connection data exchange protocol
- **Path Stripping**: Transparent tunnel ID removal for local service
- **DynamoDB as Routing Table**: GSI-based connection lookup
- **Async Request Correlation**: Polling pattern for Lambda-to-Lambda communication
- **Transparent Content Rewriting**: Regex-based path rewriting for HTML/CSS/JavaScript
- **Tunnel Context Injection**: Global JavaScript API for dynamic URL construction
- **Retry with Exponential Backoff**: Robust WebSocket message delivery

### Tools & Technologies

- **Runtime**: Rust with AWS Lambda Runtime
- **Infrastructure**: Pulumi (TypeScript)
- **AWS Services**: Lambda, API Gateway (WebSocket & HTTP), DynamoDB
- **Build Tool**: cargo-lambda for ARM64 cross-compilation
- **Monitoring**: CloudWatch Logs, CloudWatch Metrics

---

**Last Updated**: October 25, 2025
**Status**: Production Ready
**License**: MIT
