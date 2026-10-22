#!/usr/bin/env bash
# Smoke test: run the agent binary behind a real Zentinel (fail-closed) with
# the shared harness from zentinelproxy/.github (agent-smoke/smoke.py).
#
#   AGENT_SMOKE=<path to smoke.py> scripts/smoke-test.sh [<agent-binary> <zentinel-binary>]
#
# The binaries default to $AGENT_BIN and $ZENTINEL_BIN, which the shared
# release workflow sets. Run by that workflow on every platform before any
# asset is published, and by CI on every push and pull request.
#
# With scripts/smoke/api-deprecation.yaml, over the Unix socket and over gRPC:
#   * the v2 handshake works and a path that is not deprecated is forwarded (200);
#   * a deprecated endpoint whose sunset is in the future is still forwarded (200);
#   * a removed endpoint (also with a query string) and one past its sunset
#     get 410 Gone and never reach the upstream;
#   * a deprecated endpoint with a redirect action gets 301 from the agent;
#   * with the agent stopped the clean request is refused (agent on the path).
# (The harness cannot check response headers, so the Deprecation / Sunset
# headers are covered by the unit tests only.)

set -euo pipefail

AGENT_BIN=${1:-${AGENT_BIN:?usage: smoke-test.sh <agent-binary> <zentinel-binary>}}
ZENTINEL_BIN=${2:-${ZENTINEL_BIN:?usage: smoke-test.sh <agent-binary> <zentinel-binary>}}
AGENT_SMOKE=${AGENT_SMOKE:?set AGENT_SMOKE to agent-smoke/smoke.py from zentinelproxy/.github}

CONFIG="$(cd "$(dirname "$0")" && pwd)/smoke/api-deprecation.yaml"

cases=(
    --clean '{"name": "not deprecated", "path": "/smoke"}'
    --case '{"name": "deprecated, sunset ahead (warn)", "path": "/api/v1/users", "expect": 200}'
    --case '{"name": "removed endpoint", "path": "/auth/legacy", "expect": 410}'
    --case '{"name": "removed endpoint with a query string", "path": "/auth/legacy?x=1", "expect": 410}'
    --case '{"name": "past its sunset", "path": "/api/v1/orders", "expect": 410}'
    --case '{"name": "redirect to replacement", "path": "/api/v1/products?page=2", "expect": 301}'
)

for transport in uds grpc; do
    echo "=== api-deprecation agent over $transport"
    python3 "$AGENT_SMOKE" --agent "$AGENT_BIN" --zentinel "$ZENTINEL_BIN" \
        --name api-deprecation --transport "$transport" \
        --agent-arg=--config --agent-arg "$CONFIG" \
        "${cases[@]}"
done
