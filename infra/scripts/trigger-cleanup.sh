#!/bin/bash
# Script to manually trigger the TTL cleanup Lambda function
# This simulates the EventBridge scheduled event

set -e

# AWS Configuration
export AWS_PROFILE=sandbox-account-admin
export AWS_REGION=us-east-1

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get Lambda function name from Pulumi stack output or use default
FUNCTION_NAME="${1:-http-tunnel-handler-dev}"

echo -e "${YELLOW}Triggering cleanup for Lambda function: ${FUNCTION_NAME}${NC}"

# Create EventBridge scheduled event payload
CLEANUP_EVENT=$(cat <<'EOF'
{
  "version": "0",
  "id": "manual-cleanup-trigger",
  "detail-type": "Scheduled Event",
  "source": "aws.events",
  "account": "123456789012",
  "time": "2025-10-25T13:37:00Z",
  "region": "us-east-1",
  "resources": [
    "arn:aws:events:us-east-1:123456789012:rule/http-tunnel-cleanup-dev"
  ],
  "detail": {}
}
EOF
)

echo -e "${YELLOW}Invoking cleanup Lambda...${NC}"

# Invoke Lambda function with the cleanup event
RESPONSE=$(aws lambda invoke \
  --function-name "$FUNCTION_NAME" \
  --payload "$CLEANUP_EVENT" \
  --cli-binary-format raw-in-base64-out \
  /tmp/cleanup-response.json 2>&1)

# Check if invocation was successful
if [ $? -eq 0 ]; then
  echo -e "${GREEN}✓ Lambda invoked successfully${NC}"

  # Parse and display the response
  if [ -f /tmp/cleanup-response.json ]; then
    echo -e "\n${GREEN}Cleanup Results:${NC}"
    cat /tmp/cleanup-response.json | jq '.' || cat /tmp/cleanup-response.json
    echo ""

    # Extract metrics if available
    CONNECTIONS_DELETED=$(cat /tmp/cleanup-response.json | jq -r '.connectionsDeleted' 2>/dev/null || echo "N/A")
    REQUESTS_DELETED=$(cat /tmp/cleanup-response.json | jq -r '.requestsDeleted' 2>/dev/null || echo "N/A")

    echo -e "${GREEN}Summary:${NC}"
    echo "  Connections deleted: $CONNECTIONS_DELETED"
    echo "  Pending requests deleted: $REQUESTS_DELETED"
  fi
else
  echo -e "${RED}✗ Lambda invocation failed${NC}"
  echo "$RESPONSE"
  exit 1
fi

# View recent logs
echo -e "\n${YELLOW}Recent CloudWatch Logs (last 2 minutes):${NC}"
aws logs tail "/aws/lambda/$FUNCTION_NAME" \
  --region "$AWS_REGION" \
  --profile "$AWS_PROFILE" \
  --since 2m \
  --format short \
  --filter-pattern "cleanup" \
  2>&1 | grep -E "INFO|ERROR|cleanup|deleted" || true

echo -e "\n${YELLOW}All recent Lambda logs (last 3 minutes):${NC}"
aws logs tail "/aws/lambda/$FUNCTION_NAME" \
  --region "$AWS_REGION" \
  --profile "$AWS_PROFILE" \
  --since 3m \
  --format short \
  2>&1 | tail -30

# Cleanup temp file
rm -f /tmp/cleanup-response.json

echo -e "\n${GREEN}✓ Cleanup test complete${NC}"
