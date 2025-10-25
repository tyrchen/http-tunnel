import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { tags } from "./config";

export interface DynamoDBTables {
  connectionsTable: aws.dynamodb.Table;
  pendingRequestsTable: aws.dynamodb.Table;
}

export function createDynamoDBTables(): DynamoDBTables {
  // Connections table with GSI for tunnel ID lookup (path-based routing)
  const connectionsTable = new aws.dynamodb.Table("connections-table", {
    name: pulumi.interpolate`http-tunnel-connections-${tags.Environment}`,
    billingMode: "PAY_PER_REQUEST",
    hashKey: "connectionId",
    attributes: [
      { name: "connectionId", type: "S" },
      { name: "tunnelId", type: "S" },  // Changed from publicSubdomain for path-based routing
    ],
    globalSecondaryIndexes: [
      {
        name: "tunnel-id-index",  // Changed from subdomain-index
        hashKey: "tunnelId",       // Changed from publicSubdomain
        projectionType: "ALL",
      },
    ],
    ttl: {
      attributeName: "ttl",
      enabled: true,
    },
    tags: {
      ...tags,
      Name: "HTTP Tunnel Connections",
    },
  });

  // Pending requests table
  const pendingRequestsTable = new aws.dynamodb.Table("pending-requests-table", {
    name: pulumi.interpolate`http-tunnel-pending-requests-${tags.Environment}`,
    billingMode: "PAY_PER_REQUEST",
    hashKey: "requestId",
    attributes: [
      { name: "requestId", type: "S" },
    ],
    ttl: {
      attributeName: "ttl",
      enabled: true,
    },
    streamEnabled: true,
    streamViewType: "NEW_AND_OLD_IMAGES",
    tags: {
      ...tags,
      Name: "HTTP Tunnel Pending Requests",
    },
  });

  return {
    connectionsTable,
    pendingRequestsTable,
  };
}
