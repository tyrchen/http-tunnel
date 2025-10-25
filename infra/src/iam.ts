import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { tags } from "./config";

const lambdaAssumeRolePolicy = aws.iam.assumeRolePolicyForPrincipal({
  Service: "lambda.amazonaws.com",
});

/**
 * Create a unified IAM role for the single Lambda handler
 * This role has permissions for DynamoDB operations
 * WebSocket API permissions are added separately after the API is created
 */
export function createLambdaRole(
  connectionsTableArn: pulumi.Output<string>,
  pendingRequestsTableArn: pulumi.Output<string>
): aws.iam.Role {
  // Unified handler role with all permissions
  const handlerRole = new aws.iam.Role("handler-lambda-role", {
    assumeRolePolicy: lambdaAssumeRolePolicy,
    tags: {
      ...tags,
      Name: "HTTP Tunnel Unified Handler Role",
    },
  });

  // Attach basic execution role for CloudWatch Logs
  new aws.iam.RolePolicyAttachment("handler-lambda-basic-execution", {
    role: handlerRole,
    policyArn: "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole",
  });

  // DynamoDB permissions policy
  new aws.iam.RolePolicy("handler-dynamodb-policy", {
    role: handlerRole,
    policy: pulumi.all([
      connectionsTableArn,
      pendingRequestsTableArn,
    ]).apply(([connTableArn, pendingTableArn]) =>
      JSON.stringify({
        Version: "2012-10-17",
        Statement: [
          {
            Sid: "DynamoDBConnectionsTable",
            Effect: "Allow",
            Action: [
              "dynamodb:PutItem",
              "dynamodb:GetItem",
              "dynamodb:DeleteItem",
            ],
            Resource: connTableArn,
          },
          {
            Sid: "DynamoDBConnectionsTableGSI",
            Effect: "Allow",
            Action: ["dynamodb:Query"],
            Resource: `${connTableArn}/index/tunnel-id-index`,
          },
          {
            Sid: "DynamoDBPendingRequestsTable",
            Effect: "Allow",
            Action: [
              "dynamodb:PutItem",
              "dynamodb:GetItem",
              "dynamodb:UpdateItem",
              "dynamodb:DeleteItem",
            ],
            Resource: pendingTableArn,
          },
        ],
      })
    ),
  });

  return handlerRole;
}
