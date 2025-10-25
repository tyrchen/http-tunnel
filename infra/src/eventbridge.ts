import * as aws from "@pulumi/aws";
import { appConfig, tags } from "./config";

/**
 * Create EventBridge event bus for HTTP response notifications
 */
export function createEventBus(): aws.cloudwatch.EventBus {
  return new aws.cloudwatch.EventBus("http-tunnel-events", {
    name: `http-tunnel-events-${appConfig.environment}`,
    tags: {
      ...tags,
      Name: "HTTP Tunnel Event Bus",
    },
  });
}
