# Load Testing for HTTP Tunnel

This directory contains load tests for the HTTP Tunnel system using K6.

## Prerequisites

Install K6:
```bash
# macOS
brew install k6

# Linux
sudo gpg -k
sudo gpg --no-default-keyring --keyring /usr/share/keyrings/k6-archive-keyring.gpg --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
echo "deb [signed-by=/usr/share/keyrings/k6-archive-keyring.gpg] https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6

# Windows
choco install k6
```

## Running Load Tests

### Basic Test
```bash
k6 run tests/load/k6-tunnel-test.js
```

### With Custom Endpoint
```bash
k6 run --env WS_ENDPOINT=wss://your-websocket-endpoint tests/load/k6-tunnel-test.js
```

### With Authentication
```bash
export TTF_TOKEN="your-jwt-token-here"
k6 run --env TOKEN=$TTF_TOKEN tests/load/k6-tunnel-test.js
```

### Custom Load Profile
```bash
# Light load (5 concurrent tunnels)
k6 run --vus 5 --duration 2m tests/load/k6-tunnel-test.js

# Heavy load (50 concurrent tunnels)
k6 run --vus 50 --duration 5m tests/load/k6-tunnel-test.js
```

## Test Scenarios

The K6 test performs the following:

1. **Tunnel Establishment**: Creates WebSocket tunnel connections
2. **Connection Verification**: Validates connection_established message
3. **HTTP Proxying**: Makes HTTP requests through each established tunnel
4. **Load Progression**: Gradually ramps up from 5 to 20 concurrent tunnels

## Metrics

The test tracks:
- **tunnel_establishment_success**: Success rate of tunnel connections
- **tunnel_establishment_time**: Time to establish tunnel (p95, avg)
- **http_request_success**: Success rate of HTTP requests
- **http_request_time**: HTTP request latency (p95, avg)
- **total_http_requests**: Total number of HTTP requests made

## Success Criteria

- Tunnel establishment success rate: >95%
- Tunnel establishment time (p95): <5 seconds
- HTTP request success rate: >95%
- HTTP request time (p95): <1 second

## Output

Results are saved to:
- **Console**: Real-time metrics during test
- **tests/load/summary.json**: Detailed JSON results

## Interpreting Results

### Good Performance
```
Tunnel Metrics:
  Establishment Success Rate: 98.50%
  Establishment Time (p95): 2.1s
  HTTP Request Time (p95): 450ms
```

### Performance Issues
```
Tunnel Metrics:
  Establishment Success Rate: 85.20%  ← Below threshold
  HTTP Request Time (p95): 2500ms    ← Above threshold
```

If tests fail, check:
1. CloudWatch Lambda metrics for errors/throttling
2. DynamoDB capacity and throttling
3. API Gateway throttling limits
4. WebSocket connection limits

## Advanced Usage

### Smoke Test (Quick Validation)
```bash
k6 run --vus 2 --duration 30s tests/load/k6-tunnel-test.js
```

### Stress Test (Find Limits)
```bash
k6 run --vus 100 --duration 10m tests/load/k6-tunnel-test.js
```

### CI/CD Integration
```bash
# Exit with error code if thresholds not met
k6 run --quiet tests/load/k6-tunnel-test.js
echo $?  # 0 = success, 99 = thresholds failed
```

## Monitoring During Tests

While tests run, monitor:
```bash
# Lambda metrics
aws cloudwatch get-metric-statistics \
  --namespace AWS/Lambda \
  --metric-name Duration \
  --dimensions Name=FunctionName,Value=http-tunnel-handler-dev \
  --start-time $(date -u -d '5 minutes ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 60 \
  --statistics Average,Maximum \
  --region us-east-1

# DynamoDB metrics
aws cloudwatch get-metric-statistics \
  --namespace AWS/DynamoDB \
  --metric-name ConsumedReadCapacityUnits \
  --dimensions Name=TableName,Value=http-tunnel-pending-requests-dev \
  --start-time $(date -u -d '5 minutes ago' +%Y-%m-%dT%H:%M:%S) \
  --end-time $(date -u +%Y-%m-%dT%H:%M:%S) \
  --period 60 \
  --statistics Sum \
  --region us-east-1
```

## Troubleshooting

### Connection Failures
- Check WebSocket endpoint is correct
- Verify authentication token if required
- Check network connectivity

### High Latency
- Check Lambda cold starts
- Verify DynamoDB isn't throttled
- Check polling vs event-driven mode

### Timeouts
- Increase K6 timeout: `timeout: '30s'` in HTTP requests
- Check API Gateway 29s timeout limit
- Verify local services respond quickly
