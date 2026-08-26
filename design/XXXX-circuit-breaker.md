# EP-XXXX: Policy-Driven Circuit Breaker for AgentGateway

- Issue: N/A
- Related: `XXXX-vehicle-edge-cloud-routing-failover.md`
- Status: proposed
- Date: 8/18/2026

> **Note:** This design reflects the proposal as of the date above. The current implementation may differ as the design
> is implemented, reviewed, or revised.

## Scope Revision (Latest)

Based on the latest architecture discussion, the circuit breaker is intentionally **simplified**:

- **Only focus on cloud/backend unavailable fallback routing.**
- Primary goal: when the preferred cloud provider/model/backend is unavailable, fail over to edge/local fallback.
- Active safety/capability/cost-triggered circuit breaking is **out of scope for the initial implementation**.
- Edge PII protection uses a **rule engine only**, not an edge-side ONNX model.

Simplified state machine remains:

```text
closed -> open -> half-open -> closed
```

Removed from v1 scope:

- safety-triggered active breaking
- model capability degradation breaking
- cost anomaly breaking
- manual approval/reset flows

These can be added later as separate external policy components if needed.

## Summary

Design a standalone circuit breaker subsystem for AgentGateway to support **cloud/backend unavailable fallback routing**.

Scope:

- Detect that the preferred cloud provider/model/backend is unavailable.
- Open the breaker so routing skips it and falls back to edge/local targets.
- Use half-open probes to recover automatically when the cloud becomes available again.

The circuit breaker is separated from routing and failover, but its v1 scope is intentionally limited to
availability-based fallback routing. Safety/capability/cost-based active breaking is explicitly out of scope.

## Background

AgentGateway currently has:

- `EndpointSet` with health/latency scoring and eviction
- retry / timeout / outlier detection
- guardrails for prompt/response safety
- virtual model routing with failover

These mechanisms mainly react to **service health**. They do not yet provide a unified policy-controlled circuit
breaker that can react to **safety risk** or **model capability degradation**.

## Goals

- Provide a standalone circuit breaker for **cloud/backend unavailable fallback routing**.
- Detect unavailable preferred targets through passive signals: consecutive failures, error rate, timeout.
- Open breaker so routing can fall back to edge/local targets.
- Recover automatically through half-open probes.
- Integrate with existing routing/failover and telemetry.
- Remain optional and backward compatible.

## Non-Goals

- Safety-triggered active breaking.
- Model capability degradation breaking.
- Cost anomaly breaking.
- Manual approval/reset flows.
- Implementing model evaluation/benchmarking.
- Replacing existing `EndpointSet` health/eviction.

## API

### Circuit Breaker Policy

```yaml
policies:
  circuitBreaker:
    enabled: true
    scope: model                 # model | provider | backend | route | user | task
    key: vehicle-assistant
    initialState: closed
    cooldown: 30s
    halfOpen:
      maxProbes: 5
      successRate: 0.8

    triggers:
      - name: consecutiveFailures
        window: 60s
        threshold: 5

      - name: timeoutRate
        window: 60s
        threshold: 0.3

      - name: safetyViolation
        window: 5m
        threshold: 1
        reasons: [prompt_injection, harmful_content, pii_leak, dangerous_tool_call]

      - name: capabilityDrop
        window: 10m
        metric: taskAccuracy
        operator: lt
        threshold: 0.8
        minSamples: 20

      - name: costAnomaly
        window: 10m
        metric: costPerRequest
        operator: gt
        threshold: 5.0
        minSamples: 10

    actions:
      open:
        - block
        - fallback: edge-lite-llm
      halfOpen:
        - allowLimited
      recovery:
        - close
```

### Scope and Key

Circuit breaker state is keyed by:

```text
scope:key
```

Examples:

```text
model:vehicle-assistant
provider:cloud-llm
backend:my-llm
route:chat
user:driver-123
task:complex-reasoning
```

### Triggers

| Trigger | Type | Meaning |
|---|---|---|
| `consecutiveFailures` | passive | upstream consecutive failures |
| `errorRate` | passive | error rate in window |
| `timeoutRate` | passive | timeout ratio in window |
| `latencyDegradation` | passive | p95 latency increases beyond threshold |
| `safetyViolation` | active | guardrails / policy engine reports dangerous call |
| `capabilityDrop` | active | model quality/accuracy metric drops below threshold |
| `costAnomaly` | active | cost per request / total cost exceeds threshold |
| `refusalRate` | active | model refusal rate increases abnormally |

## Runtime Design

### Components

```text
Request / Upstream / Guardrails / Quality Monitor
        │
        ▼
CircuitBreakerRegistry
  ├── per-key state machines
  ├── event counters / sliding windows
  └── trigger evaluator
        │
        ▼
Routing / Failover / Action Executor
```

### State Machine

```text
        ┌──────────┐
        │  closed  │
        └────┬─────┘
             │ trigger threshold reached
             ▼
        ┌──────────┐
        │   open   │
        └────┬─────┘
             │ cooldown elapsed
             ▼
        ┌──────────┐
        │ half-open│
        └────┬─────┘
             │ success rate ok
             ▼
        ┌──────────┐
        │  closed  │
        └──────────┘
```

### Event Sources

- Proxy pipeline: success, failure, timeout, retry, status code
- Guardrails: prompt injection, harmful content, PII, dangerous tool call
- Model quality monitor: task accuracy, refusal rate, hallucination rate
- Cost monitor: cost per request, token usage anomaly
- External policy engine: manual/active open command

### Actions

| Action | Description |
|---|---|
| `block` | reject request with a configured response |
| `fallback` | route to another model/backend |
| `degrade` | switch to lower capability/lower cost model |
| `reroute` | send to another provider or edge/cloud path |
| `queue` | put non-real-time request into offline queue |
| `requireApproval` | require human/operator approval before proceeding |

## Component Design

### Object Model

#### CircuitBreakerKey

A breaker is uniquely identified by a scope and a name:

```rust
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum CircuitBreakerScope {
    Global,
    Model,
    Provider,
    Backend,
    Route,
    User,
    Task,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct CircuitBreakerKey {
    pub scope: CircuitBreakerScope,
    pub name: String,
}
```

Examples:

```text
CircuitBreakerKey { scope: Model,    name: "vehicle-assistant" }
CircuitBreakerKey { scope: Provider, name: "cloud-llm" }
CircuitBreakerKey { scope: User,     name: "driver-123" }
```

#### CircuitState

```rust
#[derive(Clone, Debug, Serialize)]
pub enum CircuitState {
    Closed,
    Open {
        opened_at: SystemTime,
        reason: OpenReason,
        trigger: String,
        cooldown_until: SystemTime,
    },
    HalfOpen {
        entered_at: SystemTime,
        permitted_probes: usize,
        succeeded_probes: usize,
        failed_probes: usize,
    },
}

#[derive(Clone, Debug, Serialize)]
pub enum OpenReason {
    ConsecutiveFailures,
    ErrorRate,
    TimeoutRate,
    LatencyDegradation,
    SafetyViolation,
    CapabilityDrop,
    CostAnomaly,
    RefusalRate,
    ManualOpen,
}
```

#### CircuitConfig

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitConfig {
    pub enabled: bool,
    pub scope: CircuitBreakerScope,
    pub key: String,
    pub cooldown: Duration,
    pub half_open: HalfOpenConfig,
    pub triggers: Vec<TriggerConfig>,
    pub actions: ActionConfig,
    pub reset_policy: ResetPolicy,
}

pub struct HalfOpenConfig {
    pub max_probes: usize,
    pub success_rate: f64,
    pub max_concurrent_probes: usize,
}

pub enum ResetPolicy {
    Auto,
    Manual,
}
```

#### TriggerConfig

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TriggerConfig {
    pub name: String,
    pub kind: TriggerKind,
    pub window: Duration,
    pub threshold: f64,
    pub min_samples: Option<u64>,
    pub reasons: Option<Vec<String>>,
    pub metric: Option<String>,
    pub operator: Option<ComparisonOperator>,
}

pub enum TriggerKind {
    ConsecutiveFailures,
    ErrorRate,
    TimeoutRate,
    LatencyDegradation,
    SafetyViolation,
    CapabilityDrop,
    CostAnomaly,
    RefusalRate,
    ManualOpen,
}
```

#### CircuitEvent

All internal and external signals are normalized into `CircuitEvent`:

```rust
#[derive(Clone, Debug)]
pub enum CircuitEvent {
    RequestStarted,
    RequestSucceeded { latency: Duration },
    RequestFailed { reason: FailureReason },
    RequestTimedOut { latency: Duration },
    SafetyViolation { reason: String },
    CapabilitySample { metric: String, value: f64 },
    CostSample { metric: String, value: f64 },
    ManualOpen { reason: String },
    ManualClose { reason: String },
}

pub enum FailureReason {
    ConnectError,
    Http5xx,
    UpstreamTimeout,
    InvalidResponse,
    GuardrailRejected,
    DangerousToolCall,
    Other(String),
}
```

### State Machine Model

```text
                    ┌──────────┐
       threshold    │          │   cooldown elapsed
      ─────────────▶│  OPEN    │──────────────────┐
                    │          │                   │
                    └──────────┘                   ▼
                         ▲                  ┌──────────┐
                         │                  │ HALF-OPEN │
                         │                  └──────────┘
                         │                       │
              failed probe /                     │ success rate ok
              threshold reached                  ▼
                    ┌─────────────────────────────────────┐
                    │               CLOSED                 │
                    └─────────────────────────────────────┘
```

#### Closed

- All requests are allowed.
- Events update sliding-window counters.
- If any trigger threshold is reached, transition to `Open`.
- If `reset_policy == Manual` for safety triggers, the breaker stays `Open` until manual close.

#### Open

- `allow()` returns `Err(CircuitOpen)`.
- Routing/failover should skip this target.
- After `cooldown`, transition to `HalfOpen`.
- Safety/manual-open breakers may require manual reset and never auto-enter half-open unless configured.

#### HalfOpen

- Allow a limited number of probe requests (`max_probes`, bounded by `max_concurrent_probes`).
- Successful probes increase success count.
- Failed probes increase failure count and may immediately reopen.
- When enough probes complete:
  - `success_rate >= half_open.success_rate` → `Closed`
  - otherwise → `Open` again

### Sliding Window Counters

Each trigger maintains a bounded window:

```rust
pub struct SlidingWindow {
    window: Duration,
    buckets: VecDeque<WindowBucket>,
    total: u64,
    errors: u64,
    timeouts: u64,
    safety_events: u64,
}
```

Bucket granularity is typically 1s or 5s. Old buckets are pruned on access.

For metric triggers such as `capabilityDrop` / `costAnomaly`, use a separate metric sample buffer:

```rust
pub struct MetricWindow {
    window: Duration,
    samples: VecDeque<MetricSample>,
}

pub struct MetricSample {
    pub timestamp: SystemTime,
    pub value: f64,
}
```

### Circuit Breaking Logic

#### allow()

```rust
pub fn allow(&self) -> Result<(), CircuitOpen> {
    match self.state {
        Closed => Ok(()),
        Open { .. } => Err(CircuitOpen { key, reason, trigger }),
        HalfOpen { permitted_probes, succeeded_probes, failed_probes, max_concurrent_probes, .. } => {
            if permitted_probes < max_concurrent_probes {
                Ok(())
            } else {
                Err(CircuitOpen { .. })
            }
        }
    }
}
```

#### on_success()

```rust
match self.state {
    HalfOpen { .. } => {
        record probe success;
        if enough probes completed {
            if success_rate >= config.half_open.success_rate {
                transition_to(Closed);
            } else {
                transition_to(Open { reason: HalfOpenFailed });
            }
        }
    }
    _ => {
        record success in sliding window;
        // Some triggers recover immediately on success, e.g. consecutiveFailures.
        reset consecutive failure counter;
    }
}
```

#### on_failure()

```rust
match self.state {
    HalfOpen { .. } => {
        record probe failure;
        transition_to(Open { reason: HalfOpenFailed });
    }
    _ => {
        record failure in sliding window;
        evaluate passive triggers;
        if open condition matched {
            transition_to(Open);
        }
    }
}
```

#### on_event()

```rust
match event {
    SafetyViolation { reason } => {
        record safety event;
        if safety trigger threshold matched {
            transition_to(Open {
                reason: SafetyViolation,
                reset_policy: Manual, // unless configured otherwise
            });
        }
    },
    CapabilitySample { metric, value } => {
        record metric sample;
        if metric trigger threshold matched {
            transition_to(Open { reason: CapabilityDrop });
        }
    },
    CostSample { metric, value } => {
        record cost sample;
        if cost trigger threshold matched {
            transition_to(Open { reason: CostAnomaly });
        }
    },
    ManualOpen { reason } => {
        transition_to(Open { reason: ManualOpen, reset_policy: Manual });
    },
    ManualClose { .. } => {
        transition_to(Closed);
    },
    _ => {}
}
```

### Action Model

```rust
#[derive(Clone, Debug, Serialize)]
pub enum CircuitAction {
    Block {
        status: u16,
        body: String,
    },
    Fallback {
        target: String,
    },
    Degrade {
        model: String,
    },
    Reroute {
        target: String,
    },
    Queue {
        queue: String,
    },
    RequireApproval {
        reason: String,
    },
}
```

Action execution is performed by the routing layer. The circuit breaker itself only reports the required action.

### Registry

```rust
pub struct CircuitBreakerRegistry {
    breakers: DashMap<CircuitBreakerKey, Arc<CircuitBreaker>>,
    default_config: CircuitConfig,
}

impl CircuitBreakerRegistry {
    pub fn get_or_create(&self, key: CircuitBreakerKey) -> Arc<CircuitBreaker>;
    pub fn get(&self, key: &CircuitBreakerKey) -> Option<Arc<CircuitBreaker>>;
    pub fn snapshot(&self) -> Vec<CircuitBreakerSnapshot>;
}
```

### Persistence

- Normal service-failure state: optional, can be in-memory only.
- Safety/manual-open state: should be persisted or at least surfaced as an audit event.
- Persistence backend can be a simple JSON file or SQLite when available.

### Observability

Emit metrics and logs on every transition:

```text
circuit_breaker_state{scope,key,state}
circuit_breaker_open_total{scope,key,trigger}
circuit_breaker_probe_total{scope,key,result}
circuit_breaker_action_total{scope,key,action}
```

Log event:

```json
{
  "event": "circuit_breaker_transition",
  "scope": "model",
  "key": "vehicle-assistant",
  "from": "closed",
  "to": "open",
  "trigger": "safetyViolation",
  "reason": "prompt_injection",
  "action": "fallback"
}
```

## Integration Points

### Routing

- Circuit breaker state is consulted before `VirtualModelRouting` and `EndpointSet` selection.
- If a target is `open`, skip it in failover/weighted selection.
- If all targets are `open`, apply the configured action.

### Guardrails

- Dangerous call detection writes circuit breaker events.
- Circuit breaker can also open proactively before sending to upstream.

### Telemetry

Emit structured events:

```text
circuit_breaker.state
circuit_breaker.trigger
circuit_breaker.scope
circuit_breaker.key
circuit_breaker.action
circuit_breaker.reason
```

## Compatibility and Migration

- Default disabled; existing behavior unchanged.
- Existing `EndpointSet` health/eviction continues to work independently.
- Circuit breaker can be added gradually per model/provider/route.

## Risks and Tradeoffs

- Active triggers depend on quality/safety signals; false positives may block good traffic.
- Too many scopes/keys can increase memory and complexity.
- Automatic reopening may be unsafe for safety-triggered breaks; safety-triggered `open` may require manual reset.
- Cost anomaly thresholds need careful tuning.

## Test Plan

- Unit tests for state transitions.
- Unit tests for each trigger type.
- Integration tests for passive failure -> open -> half-open -> close.
- Integration tests for safety violation -> open -> manual reset.
- Integration tests for capability drop -> fallback/degrade.
- Tests for scope/key isolation.
- Validation tests for new YAML config.
- Performance tests for event counting overhead.

## Open Questions

- Should safety-triggered circuit breaker support only manual reset by default?
- Should circuit breaker state be persisted across restarts?
- Should capability metrics be supplied by an external evaluator or a local lightweight evaluator?
- How to avoid conflict between circuit breaker and existing `EndpointSet` eviction?
