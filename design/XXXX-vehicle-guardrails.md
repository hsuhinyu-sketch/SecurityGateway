# EP-XXXX: Vehicle Guardrails as an Independent Safety Component

- Issue: N/A
- Related: `XXXX-circuit-breaker.md`, `XXXX-vehicle-edge-cloud-routing-failover.md`
- Status: proposed
- Date: 8/18/2026

> **Note:** This design reflects the proposal as of the date above. The current implementation may differ as the design
> is implemented, reviewed, or revised.

## Scope Revision (Latest)

Based on the latest architecture discussion:

- **Edge PII/secret filtering uses a rule engine only.**
- No edge-side ONNX/PII model will be introduced for v1.
- Hard fingerprint rules + structured field rules + regex/CEL rules are the primary mechanisms.
- A heavier ML-based filter may be considered only on the cloud side in the future.

## Summary

Design Guardrails as an independent safety component for vehicle edge-cloud scenarios. The component combines a
static rule engine with dynamic vehicle context, classifies safety events into levels, and produces decisions that
can block, mask, defer, degrade, fall back, reroute, require approval, or trigger fail-safe behavior.

Guardrails is designed to be used by AgentGateway but is not coupled to AgentGateway's routing, provider, or storage
internals. It communicates through stable safety event and decision interfaces, and it can feed the independent
circuit breaker component.

## Background

AgentGateway currently has content-oriented guardrails:

- regex
- webhook
- OpenAI moderation
- AWS Bedrock Guardrails
- Google Model Armor
- Azure Content Safety

These are useful for general content filtering but do not satisfy vehicle safety requirements:

- tool calls must be checked against vehicle state (e.g. speed, gear, autopilot)
- risk is not binary; it depends on vehicle context
- actions may need to be fail-safe, not just "reject"
- safety events must be auditable and must be able to actively open a circuit breaker

## Goals

- Provide a standalone Guardrails component with a stable API.
- Support static rule evaluation.
- Support dynamic vehicle context in rules.
- Define a safety level model (L0-L4).
- Define safety decision and action model.
- Integrate with AgentGateway request/tool/response flow.
- Emit safety events to the circuit breaker.
- Preserve audit and observability.
- Remain optional and backward compatible.

## Non-Goals

- Implementing vehicle sensor acquisition itself.
- Implementing model quality evaluation itself.
- Replacing existing content guardrails entirely.
- Defining vehicle control fail-safe behavior (that belongs to vehicle safety layer).
- Implementing a full human-approval workflow UI.

## API

### Guardrails Trait

```rust
#[async_trait]
pub trait VehicleGuardrails: Send + Sync {
    async fn check_request(
        &self,
        ctx: &RequestContext,
        vehicle: &VehicleState,
    ) -> GuardrailDecision;

    async fn check_tool_call(
        &self,
        ctx: &ToolCallContext,
        vehicle: &VehicleState,
    ) -> GuardrailDecision;

    async fn check_response(
        &self,
        ctx: &ResponseContext,
        vehicle: &VehicleState,
    ) -> GuardrailDecision;
}
```

### Decision

```rust
pub enum GuardrailDecision {
    Allow,
    Reject {
        reason: String,
        action: SafetyAction,
    },
    Mask {
        field: String,
        reason: String,
    },
    Defer {
        reason: String,
    },
    RequireApproval {
        reason: String,
    },
    FailSafe {
        reason: String,
    },
}
```

### Safety Action

```rust
pub enum SafetyAction {
    Block,
    Fallback { target: String },
    Degrade { model: String },
    Reroute { target: String },
    Queue { queue: String },
    RequireApproval { reason: String },
    FailSafe { reason: String },
}
```

## Safety Level Model

| Level | Category | Example |
|---|---|---|
| L0 | Content risk | mild disallowed text, low-risk output style |
| L1 | Privacy/data risk | PII, location leakage, in-cabin recording |
| L2 | Authorization/tool risk | unauthorized tool call, abnormal frequency |
| L3 | Vehicle motion risk | steering/brake/power control in unsafe state |
| L4 | Physical safety risk | direct conflict with vehicle safety rules |

## Vehicle Context

```rust
pub struct VehicleState {
    pub speed_kmh: f64,
    pub gear: Gear,
    pub autopilot_active: bool,
    pub passenger_present: bool,
    pub environment: EnvironmentInfo,
    pub safety_mode: SafetyMode,
}

pub enum Gear {
    Park,
    Reverse,
    Neutral,
    Drive,
}

pub struct EnvironmentInfo {
    pub weather: Weather,
    pub road_type: RoadType,
    pub visibility: Visibility,
}

pub enum SafetyMode {
    Normal,
    Reduced,
    Emergency,
    Factory,
}
```

This state is provided by the vehicle middleware/adaptor and injected into Guardrails at evaluation time.

## Rule Engine

Guardrails uses a hybrid engine:

### Static Rules

- tool allow/deny lists
- high-risk tool list
- PII patterns
- prompt/response regex rules
- fixed CEL policies
- model capability constraints

### Dynamic Rules

Rules can reference vehicle context:

```text
tool.call.name == "steering.set_torque"
  && vehicle.speed_kmh > 20
  && vehicle.autopilot_active
```

```text
tool.call.name == "brake.apply"
  && vehicle.safety_mode == "emergency"
```

## Component Structure

```text
crates/agentgateway-guardrails/
├── api/               # traits, events, decisions
├── engine/            # static + CEL rule engine
├── vehicle/           # vehicle state model
├── policy/            # policy loading and compilation
├── executor/          # action execution
├── audit/             # safety audit log
└── circuit/           # circuit breaker event integration
```

## Runtime Design

### Evaluation Pipeline

```text
RequestContext
   + VehicleState
        │
        ▼
check_request()
   │
   ├── Allow
   ├── Reject / Mask / Defer / RequireApproval / FailSafe
   └── SafetyEvent -> CircuitBreaker
        │
        ▼
ToolCallContext
   + VehicleState
        │
        ▼
check_tool_call()
   │
   ├── Allow
   ├── Reject / RequireApproval / FailSafe
   └── SafetyEvent -> CircuitBreaker
        │
        ▼
ResponseContext
   + VehicleState
        │
        ▼
check_response()
   │
   ├── Allow
   ├── Mask / Reject / Degrade
   └── SafetyEvent -> CircuitBreaker
```

### Event Emission

When Guardrails rejects or restricts a request, it emits:

```rust
pub struct SafetyEvent {
    pub level: SafetyLevel,
    pub category: SafetyCategory,
    pub reason: String,
    pub scope: String,
    pub key: String,
    pub action: SafetyAction,
    pub occurred_at: SystemTime,
}
```

This event is sent to:

- audit log
- metrics
- `CircuitBreaker.on_safety_violation()`

For L2 and above, the circuit breaker should open with manual reset by default.

## Configuration Example

```yaml
guardrails:
  enabled: true
  engine:
    mode: hybrid
    vehicleContextProvider: local

  requestRules:
    - name: prompt-injection
      type: regex
      action: reject
      level: L2

  toolRules:
    - name: steering-speed-limit
      type: cel
      level: L3
      when: 'tool.call.name == "steering.set_torque" && vehicle.speed_kmh > 20'
      action: requireApproval

    - name: brake-emergency
      type: cel
      level: L4
      when: 'tool.call.name == "brake.apply" && vehicle.safety_mode == "emergency"'
      action: failSafe

  responseRules:
    - name: driving-unsafe-advice
      type: cel
      level: L3
      when: 'response.contains("ignore red light")'
      action: reject
```

## Integration with AgentGateway

AgentGateway calls Guardrails at three points:

```text
Request  -> check_request()
Tool     -> check_tool_call()
Response -> check_response()
```

If decision is `Reject` / `Defer` / `RequireApproval` / `FailSafe`, AgentGateway:

- stops forwarding
- applies the action
- records the decision in access log
- forwards the safety event to the circuit breaker

## Compatibility and Migration

- Guardrails is disabled by default.
- Existing content guardrails continue to work.
- The new component can be introduced incrementally.
- No changes to existing routing when Guardrails is not configured.

## Risks and Tradeoffs

- Vehicle state may be inaccurate; fail-safe actions should be conservative.
- Too many dynamic rules may increase latency.
- Safety rules need separate review and versioning.
- Circuit breaker triggered by Guardrails should default to manual reset.
- Need to avoid double evaluation with existing content guardrails.

## Test Plan

- Unit tests for static rules.
- Unit tests for CEL dynamic rules with vehicle context.
- Unit tests for safety level classification.
- Unit tests for decision-to-action mapping.
- Integration tests for request/tool/response pipeline.
- Integration test for safety event -> circuit breaker open -> manual reset.
- Audit log tests.
- Performance tests for rule evaluation latency.

## Open Questions

- Should Guardrails be a separate crate now or first modularized inside agentgateway?
- Should vehicle state be pushed by middleware or pulled by Guardrails?
- Should L4 fail-safe actions be delegated to the vehicle safety controller instead of the gateway?
- Should safety rules be updatable from the cloud, and if so, how?
