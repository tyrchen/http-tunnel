import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import { createDynamoDBTables } from "./src/dynamodb";
import { createLambdaRole } from "./src/iam";
import { createLambdaHandler } from "./src/lambda";
import { createCustomDomains } from "./src/domain";
import { createMonitoringDashboard, createAlarms, createBudget } from "./src/monitoring";
import { createEventBus } from "./src/eventbridge";
import { createStreamMapping } from "./src/streaming";
import { appConfig, tags } from "./src/config";

// Configure AWS provider with profile from environment
const awsProfile = process.env.AWS_PROFILE || "default";
const awsRegion = process.env.AWS_REGION || "us-east-1";
const awsProvider = new aws.Provider("aws-provider", {
  profile: awsProfile,
  region: awsRegion,
});

// Step 1: Create DynamoDB tables
const { connectionsTable, pendingRequestsTable } = createDynamoDBTables();

// Step 1b: Create EventBridge event bus for event-driven responses
const eventBus = createEventBus();

// Step 2: Create IAM role (without WebSocket API ARN policy initially)
const handlerRole = createLambdaRole(
  connectionsTable.arn,
  pendingRequestsTable.arn,
  eventBus.arn
);

// Step 3: Create WebSocket API first (without routes) to get the endpoint
const preliminaryWebsocketApi = new aws.apigatewayv2.Api("websocket-api", {
  name: pulumi.interpolate`http-tunnel-ws-${appConfig.environment}`,
  protocolType: "WEBSOCKET",
  routeSelectionExpression: "$request.body.action",
  tags: {
    ...tags,
    Name: "HTTP Tunnel WebSocket API",
  },
});

const preliminaryWebsocketStage = new aws.apigatewayv2.Stage("websocket-stage", {
  apiId: preliminaryWebsocketApi.id,
  name: appConfig.environment,
  autoDeploy: true,
  tags: {
    ...tags,
    Name: "HTTP Tunnel WebSocket Stage",
  },
});

const websocketEndpoint = pulumi.interpolate`wss://${preliminaryWebsocketApi.id}.execute-api.${appConfig.awsRegion}.amazonaws.com/${preliminaryWebsocketStage.name}`;

// Step 4: Create Lambda handler with the WebSocket endpoint
const handler = createLambdaHandler(
  handlerRole,
  connectionsTable.name,
  pendingRequestsTable.name,
  websocketEndpoint,
  eventBus.name
);

// Step 5: Add WebSocket API permissions to the IAM role
new aws.iam.RolePolicy("handler-websocket-policy", {
  role: handlerRole,
  policy: preliminaryWebsocketApi.executionArn.apply((wsApiExecArn: string) =>
    JSON.stringify({
      Version: "2012-10-17",
      Statement: [
        {
          Sid: "ApiGatewayWebSocketManagement",
          Effect: "Allow",
          Action: ["execute-api:ManageConnections"],
          Resource: `${wsApiExecArn}/*/*/@connections/*`,
        },
      ],
    })
  ),
});

// Step 6: Create WebSocket API routes
// $connect route
const connectIntegration = new aws.apigatewayv2.Integration(
  "connect-integration",
  {
    apiId: preliminaryWebsocketApi.id,
    integrationType: "AWS_PROXY",
    integrationUri: handler.invokeArn,
  }
);

new aws.apigatewayv2.Route("connect-route", {
  apiId: preliminaryWebsocketApi.id,
  routeKey: "$connect",
  target: pulumi.interpolate`integrations/${connectIntegration.id}`,
});

new aws.lambda.Permission("connect-lambda-permission", {
  action: "lambda:InvokeFunction",
  function: handler.name,
  principal: "apigateway.amazonaws.com",
  sourceArn: pulumi.interpolate`${preliminaryWebsocketApi.executionArn}/*/$connect`,
});

// $disconnect route
const disconnectIntegration = new aws.apigatewayv2.Integration(
  "disconnect-integration",
  {
    apiId: preliminaryWebsocketApi.id,
    integrationType: "AWS_PROXY",
    integrationUri: handler.invokeArn,
  }
);

new aws.apigatewayv2.Route("disconnect-route", {
  apiId: preliminaryWebsocketApi.id,
  routeKey: "$disconnect",
  target: pulumi.interpolate`integrations/${disconnectIntegration.id}`,
});

new aws.lambda.Permission("disconnect-lambda-permission", {
  action: "lambda:InvokeFunction",
  function: handler.name,
  principal: "apigateway.amazonaws.com",
  sourceArn: pulumi.interpolate`${preliminaryWebsocketApi.executionArn}/*/$disconnect`,
});

// $default route
const responseIntegration = new aws.apigatewayv2.Integration(
  "response-integration",
  {
    apiId: preliminaryWebsocketApi.id,
    integrationType: "AWS_PROXY",
    integrationUri: handler.invokeArn,
  }
);

new aws.apigatewayv2.Route("default-route", {
  apiId: preliminaryWebsocketApi.id,
  routeKey: "$default",
  target: pulumi.interpolate`integrations/${responseIntegration.id}`,
});

new aws.lambda.Permission("response-lambda-permission", {
  action: "lambda:InvokeFunction",
  function: handler.name,
  principal: "apigateway.amazonaws.com",
  sourceArn: pulumi.interpolate`${preliminaryWebsocketApi.executionArn}/*/$default`,
});

// Step 7: Create HTTP API
const httpApi = new aws.apigatewayv2.Api("http-api", {
  name: pulumi.interpolate`http-tunnel-http-${appConfig.environment}`,
  protocolType: "HTTP",
  tags: {
    ...tags,
    Name: "HTTP Tunnel HTTP API",
  },
});

const forwardingIntegration = new aws.apigatewayv2.Integration(
  "forwarding-integration",
  {
    apiId: httpApi.id,
    integrationType: "AWS_PROXY",
    integrationUri: handler.invokeArn,
    payloadFormatVersion: "1.0",
    timeoutMilliseconds: 29000,
  }
);

new aws.apigatewayv2.Route("catchall-route", {
  apiId: httpApi.id,
  routeKey: "$default",
  target: pulumi.interpolate`integrations/${forwardingIntegration.id}`,
});

new aws.lambda.Permission("forwarding-lambda-permission", {
  action: "lambda:InvokeFunction",
  function: handler.name,
  principal: "apigateway.amazonaws.com",
  sourceArn: pulumi.interpolate`${httpApi.executionArn}/*`,
});

const httpStage = new aws.apigatewayv2.Stage("http-stage", {
  apiId: httpApi.id,
  name: appConfig.environment,
  autoDeploy: true,
  defaultRouteSettings: {
    throttlingBurstLimit: 100,  // Burst capacity
    throttlingRateLimit: 50,     // Steady-state requests/sec
  },
  tags: {
    ...tags,
    Name: "HTTP Tunnel HTTP Stage",
  },
});

const httpEndpoint = pulumi.interpolate`https://${httpApi.id}.execute-api.${appConfig.awsRegion}.amazonaws.com`;

// Step 8: Wire DynamoDB Stream to Lambda for event-driven responses
const streamMapping = createStreamMapping(handler, pendingRequestsTable);

// Step 9: Create EventBridge rule for scheduled cleanup
const cleanupRule = new aws.cloudwatch.EventRule("cleanup-schedule", {
  name: pulumi.interpolate`http-tunnel-cleanup-${appConfig.environment}`,
  description: "Triggers Lambda to clean up expired connections and pending requests every 12 hours",
  scheduleExpression: "rate(12 hours)",
  tags: {
    ...tags,
    Name: "HTTP Tunnel Cleanup Schedule",
  },
});

new aws.cloudwatch.EventTarget("cleanup-target", {
  rule: cleanupRule.name,
  arn: handler.arn,
});

new aws.lambda.Permission("cleanup-permission", {
  action: "lambda:InvokeFunction",
  function: handler.name,
  principal: "events.amazonaws.com",
  sourceArn: cleanupRule.arn,
});

// Step 9: Create custom domains (optional)
const customDomains = createCustomDomains(
  httpApi.id,
  httpStage.id,
  preliminaryWebsocketApi.id,
  preliminaryWebsocketStage.id
);

// Step 10: Create monitoring resources (optional)
let dashboard: aws.cloudwatch.Dashboard | undefined;
let budget: aws.budgets.Budget | undefined;

if (appConfig.enableMonitoring) {
  dashboard = createMonitoringDashboard(
    handler.name,
    httpApi.id,
    preliminaryWebsocketApi.id,
    connectionsTable.name,
    pendingRequestsTable.name
  );

  createAlarms(
    handler.name,
    httpApi.id,
    preliminaryWebsocketApi.id,
    connectionsTable.name
  );

  if (appConfig.alertEmail) {
    budget = createBudget(appConfig.alertEmail);
  }
}

// Exports
export const connectionsTableName = connectionsTable.name;
export const pendingRequestsTableName = pendingRequestsTable.name;
export const websocketApiEndpoint = websocketEndpoint;
export const httpApiEndpoint = httpEndpoint;
export const websocketApiId = preliminaryWebsocketApi.id;
export const httpApiId = httpApi.id;
export const lambdaFunctionName = handler.name;
export const lambdaFunctionArn = handler.arn;

// Export custom domain info if enabled
export const httpCustomDomain = customDomains?.httpCustomEndpoint;
export const websocketCustomDomain = customDomains?.websocketCustomEndpoint;
export const httpDomainTarget = customDomains?.httpDomainName.domainNameConfiguration.targetDomainName;
export const websocketDomainTarget = customDomains?.websocketDomainName.domainNameConfiguration.targetDomainName;

// Export usage instructions
export const forwarderCommand = customDomains
  ? pulumi.interpolate`http-tunnel-forwarder --endpoint ${customDomains.websocketCustomEndpoint}`
  : pulumi.interpolate`http-tunnel-forwarder --endpoint ${websocketEndpoint}`;

// Export monitoring info if enabled
export const dashboardName = dashboard?.dashboardName;
export const budgetName = budget?.name;
