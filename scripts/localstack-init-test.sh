#!/usr/bin/env bash
# Creates S3 bucket and SQS FIFO queue for the test stack.
# Invoked automatically by LocalStack on container startup.
set -euo pipefail

AWS_OPTS="--endpoint-url http://localhost:4566 --region eu-central-1 --no-cli-pager"

echo "[localstack-test] Creating S3 bucket: dal-test"
aws $AWS_OPTS s3 mb s3://dal-test || true

echo "[localstack-test] Creating SQS FIFO queue: dal-jobs-test.fifo"
aws $AWS_OPTS sqs create-queue \
  --queue-name dal-jobs-test.fifo \
  --attributes FifoQueue=true,ContentBasedDeduplication=true || true

echo "[localstack-test] Done."
