import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { appConfig, tags } from "./config";

export interface CustomDomains {
  httpDomainName: aws.apigatewayv2.DomainName;
  httpApiMapping: aws.apigatewayv2.ApiMapping;
  websocketDomainName: aws.apigatewayv2.DomainName;
  websocketApiMapping: aws.apigatewayv2.ApiMapping;
  httpCustomEndpoint: pulumi.Output<string>;
  websocketCustomEndpoint: pulumi.Output<string>;
}

/**
 * Create custom domains for both HTTP and WebSocket APIs
 * This allows using custom domains like tunnel.sandbox.mydomain.io
 *
 * Setup instructions:
 * 1. Get an ACM certificate for your domain (*.sandbox.mydomain.io or specific subdomain)
 * 2. Set the certificate ARN in Pulumi config
 * 3. After deployment, create DNS records:
 *    - HTTP: tunnel.sandbox.mydomain.io -> <regionalDomainName from output>
 *    - WebSocket: ws.sandbox.mydomain.io -> <regionalDomainName from output>
 */
export function createCustomDomains(
  httpApiId: pulumi.Output<string>,
  httpStageId: pulumi.Output<string>,
  websocketApiId: pulumi.Output<string>,
  websocketStageId: pulumi.Output<string>
): CustomDomains | undefined {
  if (!appConfig.enableCustomDomain) {
    return undefined;
  }

  if (!appConfig.certificateArn) {
    throw new Error("Certificate ARN is required when custom domain is enabled");
  }

  // HTTP API domain (for receiving forwarded requests)
  const httpDomain = appConfig.domainName; // e.g., tunnel.sandbox.mydomain.io
  const httpDomainName = new aws.apigatewayv2.DomainName("http-custom-domain", {
    domainName: httpDomain,
    domainNameConfiguration: {
      certificateArn: appConfig.certificateArn,
      endpointType: "REGIONAL",
      securityPolicy: "TLS_1_2",
    },
    tags: {
      ...tags,
      Name: "HTTP Tunnel HTTP API Domain",
    },
  });

  const httpApiMapping = new aws.apigatewayv2.ApiMapping("http-api-mapping", {
    apiId: httpApiId,
    domainName: httpDomainName.id,
    stage: httpStageId,
  });

  // WebSocket API domain (for agent connections)
  // Use configured websocketDomainName or derive from domainName
  const websocketDomain = appConfig.websocketDomainName
    || `ws.${appConfig.domainName.split('.').slice(1).join('.')}`; // e.g., ws.sandbox.mydomain.io
  const websocketDomainName = new aws.apigatewayv2.DomainName("websocket-custom-domain", {
    domainName: websocketDomain,
    domainNameConfiguration: {
      certificateArn: appConfig.certificateArn,
      endpointType: "REGIONAL",
      securityPolicy: "TLS_1_2",
    },
    tags: {
      ...tags,
      Name: "HTTP Tunnel WebSocket API Domain",
    },
  });

  const websocketApiMapping = new aws.apigatewayv2.ApiMapping("websocket-api-mapping", {
    apiId: websocketApiId,
    domainName: websocketDomainName.id,
    stage: websocketStageId,
  });

  const httpCustomEndpoint = pulumi.interpolate`https://${httpDomain}`;
  const websocketCustomEndpoint = pulumi.interpolate`wss://${websocketDomain}`;

  return {
    httpDomainName,
    httpApiMapping,
    websocketDomainName,
    websocketApiMapping,
    httpCustomEndpoint,
    websocketCustomEndpoint,
  };
}
