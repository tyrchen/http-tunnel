import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import * as dotenv from "dotenv";

// Load .env file before reading config
dotenv.config({ path: __dirname + "/../.env" });

const config = new pulumi.Config();
const awsConfig = new pulumi.Config("aws");

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
  awsRegion: awsConfig.require("region"),
  awsProfile: awsConfig.require("profile"),
};

export const tags = {
  Environment: appConfig.environment,
  Project: "http-tunnel",
  ManagedBy: "pulumi",
};
