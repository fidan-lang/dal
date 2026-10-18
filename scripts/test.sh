#!/usr/bin/env bash
# Node supervises only owned local services and preserves the test exit status.
set -euo pipefail
exec node "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/test-local.mjs" "$@"
