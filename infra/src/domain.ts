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
  // Wildcard subdomain for subdomain-based routing (optional)
  wildcardDomainName?: aws.apigatewayv2.DomainName;
  wildcardApiMapping?: aws.apigatewayv2.ApiMapping;
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
 *
 * For subdomain-based routing (*.tunnel.example.com):
 * 1. Request wildcard ACM certificate for *.tunnel.example.com
 * 2. Create additional DomainName resource with wildcard domain
 * 3. Create Route53 A record (or CNAME) for *.tunnel.example.com
 * 4. Both path-based and subdomain-based routing will work simultaneously
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

  // Create Route53 DNS records for base domains if hosted zone is configured
  if (appConfig.hostedZoneId) {
    // Base HTTP domain record (for path-based routing)
    new aws.route53.Record("http-domain-record", {
      zoneId: appConfig.hostedZoneId,
      name: httpDomain,
      type: "A",
      aliases: [{
        name: httpDomainName.domainNameConfiguration.targetDomainName,
        zoneId: httpDomainName.domainNameConfiguration.hostedZoneId,
        evaluateTargetHealth: false,
      }],
    });

    // WebSocket domain record
    new aws.route53.Record("websocket-domain-record", {
      zoneId: appConfig.hostedZoneId,
      name: websocketDomain,
      type: "A",
      aliases: [{
        name: websocketDomainName.domainNameConfiguration.targetDomainName,
        zoneId: websocketDomainName.domainNameConfiguration.hostedZoneId,
        evaluateTargetHealth: false,
      }],
    });

    pulumi.log.info(`Route53 A records created for ${httpDomain} and ${websocketDomain}`);
  } else {
    pulumi.log.warn(
      `No hosted zone configured. Manual DNS setup required for:\n` +
      `  - ${httpDomain}\n` +
      `  - ${websocketDomain}`
    );
  }

  // Wildcard subdomain for subdomain-based routing (optional)
  // Only create if subdomain routing is enabled and certificate supports wildcards
  let wildcardDomainName: aws.apigatewayv2.DomainName | undefined;
  let wildcardApiMapping: aws.apigatewayv2.ApiMapping | undefined;

  if (appConfig.enableSubdomainRouting) {
    const wildcardDomain = `*.${httpDomain}`; // e.g., *.tunnel.example.com

    wildcardDomainName = new aws.apigatewayv2.DomainName("wildcard-custom-domain", {
      domainName: wildcardDomain,
      domainNameConfiguration: {
        certificateArn: appConfig.certificateArn,
        endpointType: "REGIONAL",
        securityPolicy: "TLS_1_2",
      },
      tags: {
        ...tags,
        Name: "HTTP Tunnel Wildcard Domain",
      },
    });

    wildcardApiMapping = new aws.apigatewayv2.ApiMapping("wildcard-api-mapping", {
      apiId: httpApiId,
      domainName: wildcardDomainName.id,
      stage: httpStageId,
    });

    pulumi.log.info(`Wildcard subdomain configured: ${wildcardDomain}`);

    // Create Route53 DNS record for wildcard domain if hosted zone is configured
    if (appConfig.hostedZoneId) {
      new aws.route53.Record("wildcard-domain-record", {
        zoneId: appConfig.hostedZoneId,
        name: wildcardDomain,
        type: "A",
        aliases: [{
          name: wildcardDomainName.domainNameConfiguration.targetDomainName,
          zoneId: wildcardDomainName.domainNameConfiguration.hostedZoneId,
          evaluateTargetHealth: false,
        }],
      });

      pulumi.log.info(`Route53 A record created for ${wildcardDomain}`);
    } else {
      pulumi.log.warn(
        `No hosted zone configured. Manual DNS setup required for ${wildcardDomain}`
      );
    }
  }

  return {
    httpDomainName,
    httpApiMapping,
    websocketDomainName,
    websocketApiMapping,
    httpCustomEndpoint,
    websocketCustomEndpoint,
    wildcardDomainName,
    wildcardApiMapping,
  };
}
