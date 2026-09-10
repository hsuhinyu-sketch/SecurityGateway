# Standalone Runtime

The standalone gateway is organized around a single configuration-to-runtime
flow:

```text
CLI / YAML -> runtime::config -> runtime::state_manager -> store -> proxy
                                      |                     |
                                      +-> resource_manager --+
```

- `runtime::config` parses process configuration and records the local YAML
  source.
- `runtime::state_manager` loads, normalizes, and watches that source.
- `resource_manager` tracks file and remote resources referenced by the active
  configuration.
- `store` owns the normalized runtime state: binds, routes, policies, backends,
  workloads, and services.
- `proxy`, `http`, `mcp`, `llm`, and `a2a` consume state from `store`; they do
  not own configuration loading.

The legacy root modules remain public for compatibility. New runtime
composition code should use the `runtime` namespace so the configuration
boundary remains explicit.
