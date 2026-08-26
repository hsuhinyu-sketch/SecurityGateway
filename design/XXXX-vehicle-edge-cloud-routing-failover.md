# EP-XXXX: Vehicle Edge-Cloud Routing and Failover

- Issue: N/A
- Related: N/A
- Status: proposed
- Date: 8/18/2026

> **Note:** This design reflects the proposal as of the date above. The current implementation may differ as the design
> is implemented, reviewed, or revised.

## Summary

Enhance AgentGateway's existing routing and failover capabilities to support vehicle edge-cloud collaboration.
The design adds network-quality-aware routing, priority-based edge/cloud failover, circuit breaking for weak
network conditions, and an optional offline queue for delayed requests.

The design builds on the existing `VirtualModelRouting` (weighted / failover / conditional) and the existing
`EndpointSet` health-aware load balancer rather than replacing them.

## Background

AgentGateway already supports:

- HTTP routing via `Bind` / `Listener` / `Route`
- LLM virtual models with weighted, conditional, and priority failover routing
- AI provider endpoint pools with health/latency scoring, eviction, and recovery
- timeout, retry, rate limiting, and stream forwarding

However, vehicle edge-cloud scenarios require additional awareness:

- network quality can change rapidly (4G/5G/WiFi, tunnels, underground parking)
- edge models are preferred for latency/privacy/cost but may be unavailable
- cloud models are fallback but may be unreachable in weak/offline networks
- failed cloud requests should not repeatedly block real-time vehicle functions
- some non-real-time requests can be queued locally and replayed when connectivity recovers

## Goals

- Add network-quality context to routing decisions.
- Allow edge-first / cloud-fallback routing with conditions.
- Add circuit breaking for cloud/edge provider endpoints.
- Support optional offline queueing for non-real-time requests.
- Preserve existing routing behavior when the new features are not configured.
- Keep the solution usable in standalone/embedded deployments without Kubernetes.

## Non-Goals

- Implementing full cloud-edge configuration synchronization.
- Building a general-purpose edge job scheduler.
- Changing the core LLM provider adapters.
- Replacing the existing `EndpointSet` load balancer.

## API

### Network Context

Expose a new CEL context object:

```text
network.interface      string   // "ethernet", "wifi", "5g", "4g", "offline"
network.quality        string   // "excellent", "good", "medium", "poor", "offline"
network.rttMs          int
network.bandwidthKbps  int
network.signalStrength int      // optional, 0-100
network.costTier       string   // "low", "medium", "high"
```

The network context can be populated by:

- environment variables on the edge device
- a local network status file
- a pluggable local hook (Unix socket / HTTP endpoint)
- future vehicle middleware

### Failover Target Conditions

Extend failover targets with an optional `when` CEL condition:

```yaml
virtualModels:
- name: vehicle-assistant
  routing:
    failover:
      targets:
      - model: edge-llm
        priority: 0
        when: 'network.quality == "good" || network.quality == "medium"'
      - model: cloud-llm
        priority: 1
        when: 'network.quality != "offline"'
      - model: edge-lite-llm
        priority: 2
```

Semantics:

1. Evaluate targets by priority order.
2. Within the same priority, use the existing health/latency-aware `EndpointSet`.
3. Skip targets whose `when` condition is false.
4. If all targets in a priority group are unhealthy or filtered out, move to the next priority group.
5. If no target matches, return the existing virtual-model resolution error or a configurable fallback response.

### Offline Queue

Add an optional route/backend policy:

```yaml
policies:
  offlineQueue:
    enabled: true
    maxEntries: 1000
    ttl: 24h
    storage: memory   # or file: /var/lib/agentgateway/queue
    replay:
      maxConcurrency: 4
      minRetryInterval: 30s
```

Only requests explicitly marked as queueable should be queued. A request can be marked via:

- route-level policy
- CEL condition
- request header, e.g. `X-AgentGateway-Queue: allowed`

### Adaptive Timeout

Extend timeout policy to support network-aware overrides:

```yaml
policies:
  timeout:
    request: 30s
    network:
      excellent: 10s
      good: 15s
      medium: 20s
      poor: 30s
      offline: 0s
```

## Runtime Design

### Network Quality Provider

Introduce a `NetworkQualityProvider` trait:

```rust
#[async_trait]
pub trait NetworkQualityProvider: Send + Sync {
    async fn current(&self) -> NetworkQuality;
}
```

Default implementation reads from local sources. Users can provide a custom implementation through configuration or a local IPC hook.

### Route Selection Flow

```text
Request
  -> match Route
  -> resolve VirtualModel
  -> read NetworkQuality
  -> select routing strategy
       conditional: evaluate CEL conditions in order
       weighted: existing weighted selection
       failover:
         for priority groups in ascending order:
           filter targets by `when`
           filter targets by health/circuit state
           select best provider in group
           if none healthy, try next priority group
  -> if selected backend unavailable:
       apply offline queue policy or return fallback
  -> forward
```

### Circuit Breaker

Add per-model/provider circuit breaker state:

```text
closed      normal, allow requests
open        fail fast, do not attempt upstream for cooldown period
half-open   allow limited probe requests
```

State transitions:

- consecutive failures or timeout threshold -> `open`
- after cooldown -> `half-open`
- successful probe -> `closed`
- failed probe -> back to `open`

This complements the existing `EndpointSet` eviction by preventing repeated cloud calls in poor network conditions.

### Offline Queue

- Queue entries are stored in memory or a small local file.
- On network recovery or next successful health probe, replay queue entries.
- Requests with hard real-time requirements must not be queued.
- Queue size and TTL are bounded.

### Config and xDS

For standalone local config, all new fields are plain YAML/JSON.

If xDS is used later, add equivalent protobuf fields to the existing `Policy` and `VirtualModel` resources.

## Compatibility and Migration

- No behavior change when network context and new fields are absent.
- Existing `failover` configs without `when` continue to work unchanged.
- Existing `conditional` routing remains the primary way to express arbitrary conditions.
- Offline queue is opt-in.

## Risks and Tradeoffs

- Network quality source may be inaccurate; use it as a routing hint, not a hard correctness guarantee.
- Adding circuit breakers can increase configuration complexity.
- Offline queue needs storage and lifecycle management.
- Dynamic network-aware timeouts may make behavior harder to debug without good audit logging.

## Test Plan

- Unit tests for failover target `when` filtering and priority ordering.
- Unit tests for circuit breaker state transitions.
- Unit tests for network-aware timeout selection.
- Integration tests with simulated network quality providers.
- End-to-end test for edge-first -> cloud fallback -> offline queue replay.
- Validation tests for new YAML/JSON config shapes.
- Performance tests to measure routing decision overhead.

## Open Questions

- Should offline queue be part of the gateway or a separate vehicle middleware?
- Should network quality be pushed from the vehicle bus or pulled by the gateway?
- Should circuit breaker state be persisted across gateway restarts?
