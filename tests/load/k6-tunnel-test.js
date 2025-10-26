// K6 Load Test for HTTP Tunnel
// Tests WebSocket tunnel establishment and HTTP proxying under load
//
// Usage:
//   k6 run tests/load/k6-tunnel-test.js
//   k6 run --env WS_ENDPOINT=wss://ws.staging.sandbox.tubi.io tests/load/k6-tunnel-test.js
//   k6 run --env TOKEN=eyJhbGc... tests/load/k6-tunnel-test.js

import ws from 'k6/ws';
import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';

// Custom metrics
const tunnelEstablishmentRate = new Rate('tunnel_establishment_success');
const tunnelEstablishmentTime = new Trend('tunnel_establishment_time');
const httpRequestRate = new Rate('http_request_success');
const httpRequestTime = new Trend('http_request_time');
const totalRequests = new Counter('total_http_requests');

// Test configuration
export const options = {
    stages: [
        { duration: '30s', target: 5 },   // Ramp up to 5 tunnels
        { duration: '2m', target: 20 },   // Ramp up to 20 tunnels
        { duration: '3m', target: 20 },   // Stay at 20 tunnels
        { duration: '30s', target: 0 },   // Ramp down
    ],
    thresholds: {
        'tunnel_establishment_success': ['rate>0.95'], // 95% success rate
        'tunnel_establishment_time': ['p(95)<5000'],   // 95% under 5 seconds
        'http_request_success': ['rate>0.95'],          // 95% success rate
        'http_request_time': ['p(95)<1000'],           // 95% under 1 second
    },
};

// Read configuration from environment
const WS_ENDPOINT = __ENV.TTF_ENDPOINT;
const TOKEN = __ENV.TOKEN || __ENV.TTF_TOKEN || '';
const REQUESTS_PER_TUNNEL = 5; // Number of HTTP requests to make through each tunnel

export default function() {
    const startTime = new Date();
    let publicUrl = '';
    let tunnelId = '';

    // Build WebSocket URL with token if provided
    const wsUrl = TOKEN ? `${WS_ENDPOINT}?token=${TOKEN}` : WS_ENDPOINT;

    // Establish tunnel
    const res = ws.connect(wsUrl, function(socket) {
        socket.on('open', function() {
            console.log('WebSocket connected, sending Ready message');
            socket.send(JSON.stringify({ action: 'ready' }));
        });

        socket.on('message', function(msg) {
            const data = JSON.parse(msg);

            if (data.type === 'connection_established') {
                const establishmentTime = new Date() - startTime;
                tunnelEstablishmentTime.add(establishmentTime);
                tunnelEstablishmentRate.add(true);

                publicUrl = data.public_url;
                tunnelId = data.tunnel_id;

                console.log(`✅ Tunnel established: ${publicUrl} (${establishmentTime}ms)`);

                // Make HTTP requests through the tunnel
                makeHttpRequests(publicUrl, tunnelId);
            } else if (data.type === 'http_request') {
                // Respond to HTTP requests (for testing bidirectional flow)
                const response = {
                    type: 'http_response',
                    request_id: data.request_id,
                    status_code: 200,
                    headers: { 'content-type': ['application/json'] },
                    body: btoa(JSON.stringify({ test: 'response' })),
                };
                socket.send(JSON.stringify(response));
            } else if (data.type === 'ping') {
                socket.send(JSON.stringify({ type: 'pong' }));
            }
        });

        socket.on('close', function() {
            console.log('WebSocket closed');
        });

        socket.on('error', function(e) {
            console.error('WebSocket error:', e);
            tunnelEstablishmentRate.add(false);
        });

        // Keep connection open for testing
        socket.setTimeout(function() {
            socket.close();
        }, 30000); // 30 seconds
    });

    check(res, {
        'WebSocket connection established': (r) => r && r.status === 101,
    });

    sleep(1);
}

function makeHttpRequests(publicUrl, tunnelId) {
    if (!publicUrl) {
        console.log('⚠️  No public URL, skipping HTTP requests');
        return;
    }

    for (let i = 0; i < REQUESTS_PER_TUNNEL; i++) {
        const startTime = new Date();

        try {
            const response = http.get(publicUrl, {
                tags: { tunnel_id: tunnelId, request_num: i },
                timeout: '5s',
            });

            const duration = new Date() - startTime;
            httpRequestTime.add(duration);
            totalRequests.add(1);

            const success = check(response, {
                'HTTP status is 200': (r) => r.status === 200,
                'Response time < 1s': (r) => r.timings.duration < 1000,
            });

            httpRequestRate.add(success);

            if (success) {
                console.log(`✅ HTTP request ${i + 1}/${REQUESTS_PER_TUNNEL}: ${response.status} (${duration}ms)`);
            } else {
                console.log(`❌ HTTP request ${i + 1}/${REQUESTS_PER_TUNNEL}: ${response.status} (${duration}ms)`);
            }
        } catch (e) {
            console.error(`❌ HTTP request ${i + 1} failed:`, e);
            httpRequestRate.add(false);
            totalRequests.add(1);
        }

        sleep(0.5); // Small delay between requests
    }
}

// Summary handler
export function handleSummary(data) {
    return {
        'tests/load/summary.json': JSON.stringify(data, null, 2),
    };
}
