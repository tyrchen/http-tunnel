import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import * as dotenv from "dotenv";

// Load .env file before reading config
dotenv.config({ path: __dirname + "/../.env" });

const config = new pulumi.Config();

export interface AppConfig {
  environment: string;
  domainName: string;
  websocketDomainName?: string;
  enableCustomDomain: boolean;
  certificateArn?: string;
  lambdaArchitecture: "x86_64" | "arm64";
  lambdaMemorySize: number;
  lambdaTimeout: number;
  awsRegion: string;
  awsProfile: string;
  enableMonitoring?: boolean;
  alertEmail?: string;
  monthlyBudget?: number;
  // Security settings
  requireAuth?: boolean;
  jwtSecret?: string;
  // Rate limiting
  rateLimitPerSecond?: number;
  rateLimitBurst?: number;
  perTunnelRateLimit?: number;
}

export const appConfig: AppConfig = {
  environment: config.get("environment") || "dev",
  // Read from env vars (from .env file) with fallback to Pulumi config
  domainName: process.env.TUNNEL_DOMAIN_NAME || config.get("domainName") || "tunnel.example.com",
  websocketDomainName: process.env.TUNNEL_WEBSOCKET_DOMAIN_NAME || config.get("websocketDomainName"),
  enableCustomDomain: config.getBoolean("enableCustomDomain") ?? false,
  certificateArn: process.env.TUNNEL_CERTIFICATE_ARN || config.get("certificateArn"),
  lambdaArchitecture: (config.get("lambdaArchitecture") as "x86_64" | "arm64") ?? "x86_64",
  lambdaMemorySize: config.getNumber("lambdaMemorySize") ?? 256,
  lambdaTimeout: config.getNumber("lambdaTimeout") ?? 30,
  awsRegion: process.env.AWS_REGION || config.get("awsRegion") || "us-east-1",
  awsProfile: process.env.AWS_PROFILE || config.get("awsProfile") || "default",
  enableMonitoring: config.getBoolean("enableMonitoring") ?? true,
  alertEmail: config.get("alertEmail"),
  monthlyBudget: config.getNumber("monthlyBudget") ?? 50,
};

export const tags = {
  Environment: appConfig.environment,
  Project: "http-tunnel",
  ManagedBy: "pulumi",
};
