# AI Gateway Platform workspace reorganization plan

## Goal

Keep AI gateway protocols independently composable while ensuring security controls remain
protocol-neutral. The desired structure follows the Rust workspace pattern used by LibAFL:
small crates own stable contracts and mechanics; runtime-specific adapters live at the edge.

```text
crates/
  gateway/
    runtime/                         # ai-gateway-runtime compatibility facade
    app/                             # ai-gateway-app / ai-gateway binary
    protocol-llm/                    # future extraction
    protocol-mcp/                    # future extraction
    protocol-a2a/                    # future extraction
    inference-routing/               # future extraction
  security/
    types/                           # current security-contracts
    policy/                          # current security-policy
    engine/                          # current security-engine
    audit/                           # current audit-core
    pipeline/                        # current gateway-adapter
    integration-agentgateway/        # runtime adapter extraction
  platform/
    core/, pool/, hbone/, protos/, celx/
```

The first-party `ai-gateway-runtime` and `ai-gateway-app` entry points are in place. Legacy
`agentgateway*` packages remain compatible implementation dependencies until each protocol API
is extracted and stabilized.

## Dependency rules

```text
security/types -> security/policy -> security/engine -> security/pipeline
                                                        ^
gateway/* ---------------------------------------------|
gateway/runtime -> security/integration-agentgateway -> security/pipeline
```

- Security core crates must not depend on `agentgateway`, HTTP, MCP, LLM, or A2A types.
- Protocol normalization belongs in gateway adapters; policy evaluation and audit event creation
  belong in security crates.
- `agentgateway` remains the composition runtime until protocol crates have stable standalone APIs.
- Applications depend on gateway runtime, never directly on security implementation crates.

## Migration phases

### Phase 0 — boundary inventory (complete)

- Record ownership and dependency rules in this document.
- Mark workspace members by domain without changing code paths.
- Preserve every package name and existing configuration schema.

### Phase 1 — extract security runtime integration

- Move audit dispatch and audit-only/enforce mode handling from `agentgateway/src/security.rs`
  into `security/integration-agentgateway`.
- Keep JWT-claim extraction in the gateway runtime initially; it depends on request extensions.
- Make LLM, A2A, MCP, and inference-routing adapters call one shared security integration API.

### Phase 1.5 — compile-time gateway profiles (complete)

- `gateway-composition` owns the protocol/security capability vocabulary and the profile trait.
- `agentgateway` and `agentgateway-app` forward `gateway-llm`, `gateway-a2a`, `gateway-mcp`,
  `inference-routing`, and `security-s1` Cargo features.
- The runtime rejects local `llm`, `mcp`, and `aiSystems` configuration that requests a capability
  omitted from the compiled profile. This establishes deployable composition contracts without
  prematurely extracting protocol implementation modules.

### Phase 2 — stabilize the security pipeline

- Rename the package directory for `gateway-adapter` to `security/pipeline` while retaining its
  package name through a compatibility period.
- Add explicit `audit`, `shadow`, and `enforce` modes to the shared integration API.
- Move capability broker implementations behind a durable-broker trait.

### Phase 3 — extract protocol boundaries only when justified

- Extract `protocol-mcp`, `protocol-llm`, or `protocol-a2a` only after each has a stable request,
  response, configuration, and test surface.
- Do not split modules merely to match the target directory diagram.

### Phase 4 — physical rename and compatibility cleanup

- Move package paths to `crates/gateway/*`, `crates/security/*`, and `crates/platform/*`.
- Update workspace paths, CI, examples, and documentation in one compatibility release.
- Remove temporary re-export crates only after downstream users migrate.

## Acceptance criteria for every phase

- `cargo check` succeeds for the default workspace members and schema feature.
- Configuration behavior and serialized field names remain backward compatible.
- No security-core crate gains a dependency on gateway protocol code.
- Integration tests cover every moved adapter and at least one composed `aiSystems` topology.
