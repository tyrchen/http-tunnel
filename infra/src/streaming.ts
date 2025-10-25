import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { tags } from "./config";

/**
 * Wire up DynamoDB Stream to Lambda for event-driven response notifications
 */
export function createStreamMapping(
  handler: aws.lambda.Function,
  pendingRequestsTable: aws.dynamodb.Table
): aws.lambda.EventSourceMapping {
  return new aws.lambda.EventSourceMapping("pending-requests-stream", {
    eventSourceArn: pendingRequestsTable.streamArn,
    functionName: handler.arn,
    startingPosition: "LATEST",
    batchSize: 100,
    maximumBatchingWindowInSeconds: 0, // Process immediately for low latency
    filterCriteria: {
      filters: [
        {
          // Only process records where status is "completed"
          // This reduces unnecessary Lambda invocations
          pattern: JSON.stringify({
            dynamodb: {
              NewImage: {
                status: {
                  S: ["completed"],
                },
              },
            },
          }),
        },
      ],
    },
  });
}
