# HTTP Tunnel Documentation

## Technical Blog Posts

This directory contains comprehensive technical documentation about building and debugging a Serverless HTTP tunnel using AWS Lambda, API Gateway WebSocket, and DynamoDB.

### Available Documents

#### 1. [Debugging a Serverless WebSocket HTTP Tunnel](./debugging-serverless-websocket-tunnel.md) (English)

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

#### 2. [调试 Serverless WebSocket HTTP 隧道：深度技术剖析](./debugging-serverless-websocket-tunnel-zh.md) (中文)

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
2. **WebSocket Lifecycle Matters**: Connection IDs aren't "active" until after $connect handler returns
3. **Event Detection Order**: Check most specific fields first when multiple event types share common fields
4. **API Versioning**: Match payload format versions between infrastructure config and code
5. **Path-Based Routing**: Simpler DNS management than wildcard subdomains

### Architecture Patterns Demonstrated

- **Unified Lambda Handler**: Single function handling multiple event types
- **Ready/Response Handshake**: Post-connection data exchange protocol
- **Path Stripping**: Transparent tunnel ID removal for local service
- **DynamoDB as Routing Table**: GSI-based connection lookup
- **Async Request Correlation**: Polling pattern for Lambda-to-Lambda communication

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
