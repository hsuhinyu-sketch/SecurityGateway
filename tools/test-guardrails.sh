#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

echo "==> Running vehicle guardrails interface tests"
cargo test -p agentgateway --lib guardrails::tests

echo
echo "==> Running vehicle guardrails demo"
cargo run -p agentgateway --example vehicle_guardrails_demo
