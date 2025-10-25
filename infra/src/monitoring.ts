import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import { appConfig, tags } from "./config";

/**
 * Create CloudWatch Dashboard for HTTP Tunnel monitoring
 */
export function createMonitoringDashboard(
  lambdaFunctionName: pulumi.Output<string>,
  httpApiId: pulumi.Output<string>,
  websocketApiId: pulumi.Output<string>,
  connectionsTableName: pulumi.Output<string>,
  pendingRequestsTableName: pulumi.Output<string>
): aws.cloudwatch.Dashboard {
  return new aws.cloudwatch.Dashboard("tunnel-dashboard", {
    dashboardName: `http-tunnel-${appConfig.environment}`,
    dashboardBody: pulumi
      .all([lambdaFunctionName, httpApiId, websocketApiId, connectionsTableName, pendingRequestsTableName])
      .apply(([funcName, httpApi, wsApi, connTable, reqTable]) =>
        JSON.stringify({
          widgets: [
            // Lambda metrics
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/Lambda", "Invocations", { stat: "Sum", label: "Total Invocations" }],
                  [".", "Errors", { stat: "Sum", label: "Errors", color: "#d62728" }],
                  [".", "Throttles", { stat: "Sum", label: "Throttles", color: "#ff7f0e" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "Lambda Invocations & Errors",
                period: 300,
                dimensions: { FunctionName: funcName },
              },
            },
            // Lambda duration
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/Lambda", "Duration", { stat: "Average", label: "Avg Duration" }],
                  ["...", { stat: "p99", label: "p99 Duration", color: "#ff7f0e" }],
                  ["...", { stat: "Maximum", label: "Max Duration", color: "#d62728" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "Lambda Duration (ms)",
                period: 300,
                yAxis: { left: { min: 0 } },
                dimensions: { FunctionName: funcName },
              },
            },
            // HTTP API metrics
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/ApiGateway", "Count", { stat: "Sum", label: "HTTP Requests", id: "m1" }],
                  [".", "4XXError", { stat: "Sum", label: "4XX Errors", color: "#ff7f0e" }],
                  [".", "5XXError", { stat: "Sum", label: "5XX Errors", color: "#d62728" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "HTTP API Requests & Errors",
                period: 300,
                dimensions: { ApiId: httpApi },
              },
            },
            // WebSocket metrics
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/ApiGatewayV2", "ConnectCount", { stat: "Sum", label: "Connections" }],
                  [".", "MessageCount", { stat: "Sum", label: "Messages" }],
                  [".", "IntegrationError", { stat: "Sum", label: "Errors", color: "#d62728" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "WebSocket Connections & Messages",
                period: 300,
                dimensions: { ApiId: wsApi },
              },
            },
            // DynamoDB connections table
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/DynamoDB", "ConsumedReadCapacityUnits", { stat: "Sum", label: "Read Capacity" }],
                  [".", "ConsumedWriteCapacityUnits", { stat: "Sum", label: "Write Capacity" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "DynamoDB Connections Table Capacity",
                period: 300,
                dimensions: { TableName: connTable },
              },
            },
            // DynamoDB throttling
            {
              type: "metric",
              width: 12,
              height: 6,
              properties: {
                metrics: [
                  ["AWS/DynamoDB", "UserErrors", { stat: "Sum", label: "Connections Table", id: "m1" }],
                  ["...", { stat: "Sum", label: "Requests Table", id: "m2" }],
                ],
                view: "timeSeries",
                stacked: false,
                region: appConfig.awsRegion,
                title: "DynamoDB Throttling/Errors",
                period: 300,
                annotations: {
                  horizontal: [
                    {
                      label: "Throttling Threshold",
                      value: 0,
                      fill: "above",
                      color: "#d62728",
                    },
                  ],
                },
              },
            },
          ],
        })
      ),
  });
}

/**
 * Create CloudWatch Alarms for critical metrics
 */
export function createAlarms(
  lambdaFunctionName: pulumi.Output<string>,
  httpApiId: pulumi.Output<string>,
  websocketApiId: pulumi.Output<string>,
  connectionsTableName: pulumi.Output<string>,
  snsTopicArn?: pulumi.Output<string>
) {
  const alarmActions = snsTopicArn ? [snsTopicArn] : [];

  // Lambda error rate alarm
  const lambdaErrorAlarm = new aws.cloudwatch.MetricAlarm("lambda-errors", {
    name: pulumi.interpolate`http-tunnel-lambda-errors-${appConfig.environment}`,
    comparisonOperator: "GreaterThanThreshold",
    evaluationPeriods: 2,
    metricName: "Errors",
    namespace: "AWS/Lambda",
    period: 300,
    statistic: "Sum",
    threshold: 10,
    datapointsToAlarm: 2,
    treatMissingData: "notBreaching",
    alarmDescription: "Alert when Lambda function has more than 10 errors in 10 minutes",
    alarmActions,
    tags: {
      ...tags,
      Name: "HTTP Tunnel Lambda Errors",
    },
  });

  pulumi.all([lambdaFunctionName]).apply(([funcName]) => {
    new aws.cloudwatch.MetricAlarm("lambda-errors-dimension", {
      name: pulumi.interpolate`http-tunnel-lambda-errors-${appConfig.environment}`,
      comparisonOperator: "GreaterThanThreshold",
      evaluationPeriods: 2,
      metricName: "Errors",
      namespace: "AWS/Lambda",
      period: 300,
      statistic: "Sum",
      threshold: 10,
      dimensions: { FunctionName: funcName },
      datapointsToAlarm: 2,
      treatMissingData: "notBreaching",
      alarmDescription: "Alert when Lambda has >10 errors in 10 minutes",
      alarmActions,
      tags: {
        ...tags,
        Name: "HTTP Tunnel Lambda Errors",
      },
    });
  });

  // DynamoDB throttling alarm for connections table
  pulumi.all([connectionsTableName]).apply(([tableName]) => {
    new aws.cloudwatch.MetricAlarm("dynamodb-throttles", {
      name: pulumi.interpolate`http-tunnel-dynamodb-throttles-${appConfig.environment}`,
      comparisonOperator: "GreaterThanThreshold",
      evaluationPeriods: 1,
      metricName: "UserErrors",
      namespace: "AWS/DynamoDB",
      period: 300,
      statistic: "Sum",
      threshold: 5,
      treatMissingData: "notBreaching",
      alarmDescription: "Alert when DynamoDB table is being throttled",
      dimensions: { TableName: tableName },
      alarmActions,
      tags: {
        ...tags,
        Name: "HTTP Tunnel DynamoDB Throttles",
      },
    });
  });

  // HTTP API 5XX errors
  pulumi.all([httpApiId]).apply(([apiId]) => {
    new aws.cloudwatch.MetricAlarm("http-api-5xx", {
      name: pulumi.interpolate`http-tunnel-http-5xx-${appConfig.environment}`,
      comparisonOperator: "GreaterThanThreshold",
      evaluationPeriods: 2,
      metricName: "5XXError",
      namespace: "AWS/ApiGateway",
      period: 300,
      statistic: "Sum",
      threshold: 20,
      datapointsToAlarm: 2,
      treatMissingData: "notBreaching",
      alarmDescription: "Alert when HTTP API has >20 5XX errors in 10 minutes",
      dimensions: { ApiId: apiId },
      alarmActions,
      tags: {
        ...tags,
        Name: "HTTP Tunnel API 5XX Errors",
      },
    });
  });

  // WebSocket disconnect rate
  pulumi.all([websocketApiId]).apply(([apiId]) => {
    new aws.cloudwatch.MetricAlarm("websocket-disconnects", {
      name: pulumi.interpolate`http-tunnel-ws-disconnects-${appConfig.environment}`,
      comparisonOperator: "GreaterThanThreshold",
      evaluationPeriods: 1,
      metricName: "IntegrationError",
      namespace: "AWS/ApiGatewayV2",
      period: 300,
      statistic: "Sum",
      threshold: 50,
      treatMissingData: "notBreaching",
      alarmDescription: "Alert when WebSocket has >50 integration errors in 5 minutes",
      dimensions: { ApiId: apiId },
      alarmActions,
      tags: {
        ...tags,
        Name: "HTTP Tunnel WebSocket Errors",
      },
    });
  });

  return {
    lambdaErrorAlarm,
  };
}

/**
 * Create AWS Budget with cost alerts
 */
export function createBudget(alertEmail: string): aws.budgets.Budget {
  return new aws.budgets.Budget("tunnel-budget", {
    name: `http-tunnel-${appConfig.environment}`,
    budgetType: "COST",
    limitAmount: "50",
    limitUnit: "USD",
    timeUnit: "MONTHLY",
    notifications: [
      {
        comparisonOperator: "GREATER_THAN",
        threshold: 80,
        thresholdType: "PERCENTAGE",
        notificationType: "ACTUAL",
        subscriberEmailAddresses: [alertEmail],
      },
      {
        comparisonOperator: "GREATER_THAN",
        threshold: 100,
        thresholdType: "PERCENTAGE",
        notificationType: "ACTUAL",
        subscriberEmailAddresses: [alertEmail],
      },
    ],
  });
}
