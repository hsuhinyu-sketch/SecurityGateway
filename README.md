# SecurityGateway

Minimal Rust-first security gateway PoC.

## Current scope

- Gateway: reuse AgentGateway for LLM and MCP traffic.
- Security engine: normalize requests and return ALLOW / DENY decisions.
- Policy: minimal subject/action/resource matching.
- Audit: create security events from decisions.

## Core crates

```text
crates/contracts
crates/policy
crates/security-engine
crates/audit-core
crates/gateway-adapter
crates/agentgateway
crates/agentgateway-app
```

## Build and test

```bash
cargo check -p security-contracts -p security-policy -p security-engine -p audit-core -p gateway-adapter -p agentgateway-app
cargo test -p security-engine -p security-policy -p audit-core -p gateway-adapter
```

## License

Apache License 2.0. See [LICENSE](LICENSE).
