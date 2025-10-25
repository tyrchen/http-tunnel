import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { appConfig, tags } from "./config";

const lambdaCodePath = process.env.HOME + "/.target/lambda/handler";

/**
 * Create the unified Lambda handler that handles all routes
 */
export function createLambdaHandler(
  role: aws.iam.Role,
  connectionsTableName: pulumi.Output<string>,
  pendingRequestsTableName: pulumi.Output<string>,
  websocketApiEndpoint: pulumi.Output<string>
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
      variables: {
        RUST_LOG: "info",
        CONNECTIONS_TABLE_NAME: connectionsTableName,
        PENDING_REQUESTS_TABLE_NAME: pendingRequestsTableName,
        DOMAIN_NAME: appConfig.domainName,
        WEBSOCKET_API_ENDPOINT: websocketApiEndpoint,
      },
    },
    tags: {
      ...tags,
      Name: "HTTP Tunnel Unified Handler",
    },
  });

  return handler;
}
