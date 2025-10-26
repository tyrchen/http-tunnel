import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import * as path from "path";
import * as fs from "fs";
import { appConfig, jwtSecret, jwksSecret, tags } from "./config";

// Use infra/lambda directory for Lambda code
const lambdaCodePath = process.env.LAMBDA_CODE_PATH ||
  path.join(__dirname, "../lambda/handler");

// Validate Lambda code exists before deployment
if (!fs.existsSync(path.join(lambdaCodePath, "bootstrap"))) {
  throw new Error(
    `Lambda code not found at ${lambdaCodePath}. ` +
    `Run 'cargo lambda build --release --arm64 --bin handler' first.`
  );
}

// Load JWKS from file if it exists (optional)
let jwksContent: string | undefined;
const jwksPath = path.join(__dirname, "../jwks.json");
if (fs.existsSync(jwksPath)) {
  jwksContent = fs.readFileSync(jwksPath, "utf-8");
  console.log("✓ JWKS file found, will be included in Lambda environment");
}

/**
 * Create the unified Lambda handler that handles all routes
 */
export function createLambdaHandler(
  role: aws.iam.Role,
  connectionsTableName: pulumi.Output<string>,
  pendingRequestsTableName: pulumi.Output<string>,
  websocketApiEndpoint: pulumi.Output<string>,
  eventBusName?: pulumi.Output<string>
): aws.lambda.Function {
  const architecture = appConfig.lambdaArchitecture === "arm64" ? "arm64" : "x86_64";

  const handler = new aws.lambda.Function("unified-handler", {
    name: pulumi.interpolate`http-tunnel-handler-${appConfig.environment}`,
    runtime: "provided.al2023", // Use AL2023 for better performance
    handler: "bootstrap",
    role: role.arn,
    architectures: [architecture],
    memorySize: appConfig.lambdaMemorySize,
    timeout: appConfig.lambdaTimeout,
    code: new pulumi.asset.FileArchive(lambdaCodePath),
    environment: {
      variables: pulumi.all([
        connectionsTableName,
        pendingRequestsTableName,
        websocketApiEndpoint,
        eventBusName,
        jwtSecret,
        jwksSecret
      ]).apply(([connTable, reqTable, wsEndpoint, busName, secret, jwks]) => {
        const vars: Record<string, string> = {
          RUST_LOG: "info",
          CONNECTIONS_TABLE_NAME: connTable,
          PENDING_REQUESTS_TABLE_NAME: reqTable,
          DOMAIN_NAME: appConfig.domainName,
          WEBSOCKET_API_ENDPOINT: wsEndpoint,
          EVENT_BUS_NAME: busName || `http-tunnel-events-${appConfig.environment}`,
          USE_EVENT_DRIVEN: appConfig.useEventDriven ? "true" : "false",
          // Authentication
          REQUIRE_AUTH: appConfig.requireAuth ? "true" : "false",
          JWT_SECRET: secret || process.env.JWT_SECRET || "default-secret-change-in-production",
          // Rate limiting
          PER_TUNNEL_RATE_LIMIT: String(appConfig.perTunnelRateLimit || 1000),
        };

        // Add JWKS - priority: Pulumi secret > file content > not set
        if (jwks) {
          vars.JWKS = jwks;
        } else if (jwksContent) {
          vars.JWKS = jwksContent;
        }

        return vars;
      }),
    },
    tags: {
      ...tags,
      Name: "HTTP Tunnel Unified Handler",
    },
  });

  return handler;
}
