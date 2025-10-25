import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { appConfig, tags } from "./config";

export interface ApiGateways {
  websocketApi: aws.apigatewayv2.Api;
  httpApi: aws.apigatewayv2.Api;
  websocketStage: aws.apigatewayv2.Stage;
  httpStage: aws.apigatewayv2.Stage;
  websocketEndpoint: pulumi.Output<string>;
  httpEndpoint: pulumi.Output<string>;
}

/**
 * Create both API Gateways (WebSocket and HTTP) with all routes
 * All routes integrate with the single unified Lambda handler
 */
export function createApiGateways(handler: aws.lambda.Function): ApiGateways {
  // ===== WebSocket API =====
  const websocketApi = new aws.apigatewayv2.Api("websocket-api", {
    name: pulumi.interpolate`http-tunnel-ws-${appConfig.environment}`,
    protocolType: "WEBSOCKET",
    routeSelectionExpression: "$request.body.action",
    tags: {
      ...tags,
      Name: "HTTP Tunnel WebSocket API",
    },
  });

  // $connect route
  const connectIntegration = new aws.apigatewayv2.Integration(
    "connect-integration",
    {
      apiId: websocketApi.id,
      integrationType: "AWS_PROXY",
      integrationUri: handler.invokeArn,
    }
  );

  new aws.apigatewayv2.Route("connect-route", {
    apiId: websocketApi.id,
    routeKey: "$connect",
    target: pulumi.interpolate`integrations/${connectIntegration.id}`,
  });

  new aws.lambda.Permission("connect-lambda-permission", {
    action: "lambda:InvokeFunction",
    function: handler.name,
    principal: "apigateway.amazonaws.com",
    sourceArn: pulumi.interpolate`${websocketApi.executionArn}/*/$connect`,
  });

  // $disconnect route
  const disconnectIntegration = new aws.apigatewayv2.Integration(
    "disconnect-integration",
    {
      apiId: websocketApi.id,
      integrationType: "AWS_PROXY",
      integrationUri: handler.invokeArn,
    }
  );

  new aws.apigatewayv2.Route("disconnect-route", {
    apiId: websocketApi.id,
    routeKey: "$disconnect",
    target: pulumi.interpolate`integrations/${disconnectIntegration.id}`,
  });

  new aws.lambda.Permission("disconnect-lambda-permission", {
    action: "lambda:InvokeFunction",
    function: handler.name,
    principal: "apigateway.amazonaws.com",
    sourceArn: pulumi.interpolate`${websocketApi.executionArn}/*/$disconnect`,
  });

  // $default route (for agent messages)
  const responseIntegration = new aws.apigatewayv2.Integration(
    "response-integration",
    {
      apiId: websocketApi.id,
      integrationType: "AWS_PROXY",
      integrationUri: handler.invokeArn,
    }
  );

  new aws.apigatewayv2.Route("default-route", {
    apiId: websocketApi.id,
    routeKey: "$default",
    target: pulumi.interpolate`integrations/${responseIntegration.id}`,
  });

  new aws.lambda.Permission("response-lambda-permission", {
    action: "lambda:InvokeFunction",
    function: handler.name,
    principal: "apigateway.amazonaws.com",
    sourceArn: pulumi.interpolate`${websocketApi.executionArn}/*/$default`,
  });

  // WebSocket stage
  const websocketStage = new aws.apigatewayv2.Stage("websocket-stage", {
    apiId: websocketApi.id,
    name: appConfig.environment,
    autoDeploy: true,
    tags: {
      ...tags,
      Name: "HTTP Tunnel WebSocket Stage",
    },
  });

  // ===== HTTP API =====
  const httpApi = new aws.apigatewayv2.Api("http-api", {
    name: pulumi.interpolate`http-tunnel-http-${appConfig.environment}`,
    protocolType: "HTTP",
    tags: {
      ...tags,
      Name: "HTTP Tunnel HTTP API",
    },
  });

  // Catch-all route for HTTP requests
  const forwardingIntegration = new aws.apigatewayv2.Integration(
    "forwarding-integration",
    {
      apiId: httpApi.id,
      integrationType: "AWS_PROXY",
      integrationUri: handler.invokeArn,
      payloadFormatVersion: "2.0",
      timeoutMilliseconds: 29000, // Just under 30s limit
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

  // HTTP stage
  const httpStage = new aws.apigatewayv2.Stage("http-stage", {
    apiId: httpApi.id,
    name: appConfig.environment,
    autoDeploy: true,
    tags: {
      ...tags,
      Name: "HTTP Tunnel HTTP Stage",
    },
  });

  const websocketEndpoint = pulumi.interpolate`wss://${websocketApi.id}.execute-api.${appConfig.awsRegion}.amazonaws.com/${websocketStage.name}`;
  const httpEndpoint = pulumi.interpolate`https://${httpApi.id}.execute-api.${appConfig.awsRegion}.amazonaws.com`;

  return {
    websocketApi,
    httpApi,
    websocketStage,
    httpStage,
    websocketEndpoint,
    httpEndpoint,
  };
}
