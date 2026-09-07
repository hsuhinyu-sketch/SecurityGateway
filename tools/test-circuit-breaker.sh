#!/usr/bin/env bash
set -euo pipefail

# Runs the circuit breaker unit tests and the standalone demo example.
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running circuit breaker unit tests"
cargo test -p agentgateway --lib circuit::tests

echo
echo "==> Running circuit breaker demo"
cargo run -p agentgateway --example circuit_breaker_demo
