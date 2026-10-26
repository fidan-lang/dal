#!/usr/bin/env bash
# Creates the object-storage bucket and SQS FIFO queue for the test stack.
# Invoked automatically by LocalStack on container startup.
set -euo pipefail

AWS_OPTS="--endpoint-url http://localhost:4566 --region eu-central-1"

echo "[localstack-test] Creating storage bucket: dal-test"
if ! aws $AWS_OPTS s3api head-bucket --bucket dal-test >/dev/null 2>&1; then
  aws $AWS_OPTS s3 mb s3://dal-test
fi

echo "[localstack-test] Creating SQS FIFO queue: dal-jobs-test.fifo"
aws $AWS_OPTS sqs create-queue \
  --queue-name dal-jobs-test.fifo \
  --attributes FifoQueue=true,ContentBasedDeduplication=true

echo "[localstack-test] Done."
