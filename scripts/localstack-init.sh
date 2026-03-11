#!/usr/bin/env bash
# LocalStack init script – runs once the service is ready.
# Creates the S3 bucket and SQS FIFO queue used by dal.
set -euo pipefail

echo "==> Creating S3 bucket: dal-packages-local"
awslocal s3api create-bucket \
  --bucket dal-packages-local \
  --region eu-central-1

echo "==> Creating SQS FIFO queue: dal-jobs.fifo"
awslocal sqs create-queue \
  --queue-name dal-jobs.fifo \
  --attributes FifoQueue=true,ContentBasedDeduplication=true \
  --region eu-central-1

echo "==> LocalStack resources ready."
