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
    mkdir -p LOCAL
    export PORT=9229
    npx --yes cognito-local > LOCAL/cognito-local.log 2>&1 &
    COGNITO_PID=$!
    export TEST_COGNITO_ENDPOINT_URL=http://127.0.0.1:9229
    export TEST_COGNITO_POOL_ID=local_0dPm2L0N
    export TEST_COGNITO_CLIENT_ID=bs5obcfdxmvh7g6ldnqmeahae

    for _ in {1..20}; do
        if curl -fsS "http://127.0.0.1:9229/local_0dPm2L0N/.well-known/jwks.json" >/dev/null; then
            break
        fi
        sleep 1
    done

    if ! curl -fsS "http://127.0.0.1:9229/local_0dPm2L0N/.well-known/jwks.json" >/dev/null; then
        echo "FAILED to start cognito-local. See LOCAL/cognito-local.log"
        if [ -f LOCAL/cognito-local.log ]; then
            echo "==> cognito-local.log"
            tail -n 200 LOCAL/cognito-local.log || true
        fi
        exit 1
    fi

    EXTRA_ARGS="-- --include-ignored"
    trap "kill $COGNITO_PID 2>/dev/null; cleanup" EXIT
fi

echo "==> Running integration tests..."
cargo test -p dal-server $EXTRA_ARGS -- --test-threads=1

echo "==> All tests passed."
