#!/usr/bin/env bash
# Run the full Dal integration test suite.
# Usage: ./scripts/test.sh [--with-cognito]
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Starting test infrastructure (docker-compose.test.yml)..."
docker compose -f docker-compose.test.yml up -d --wait --quiet-pull

# Tear down on exit
cleanup() { docker compose -f docker-compose.test.yml down; }
trap cleanup EXIT

# Copy .env.test.example → .env.test if not already present
if [ ! -f .env.test ]; then
    cp .env.test.example .env.test
    echo "==> Created .env.test from .env.test.example (edit it to customise)"
fi

# Optionally start cognito-local if --with-cognito flag given
EXTRA_ARGS=""
if [[ "${1:-}" == "--with-cognito" ]]; then
    echo "==> Starting cognito-local..."
    npx --yes @jagreehal/cognito-local &
    COGNITO_PID=$!
    export TEST_COGNITO_ENDPOINT_URL=http://localhost:9229
    sleep 2
    EXTRA_ARGS="-- --include-ignored"
    trap "kill $COGNITO_PID 2>/dev/null; cleanup" EXIT
fi

echo "==> Running integration tests..."
cargo test -p dal-server $EXTRA_ARGS -- --test-threads=1

echo "==> All tests passed."
