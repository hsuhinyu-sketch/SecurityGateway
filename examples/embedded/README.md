# Embedded / Minimal Runtime Example

This example shows a low-overhead AgentGateway configuration suitable for
embedded Linux or resource-constrained environments.

It reduces runtime footprint by:

- Setting `workerThreads: 1`
- Disabling the admin server (`adminAddr: off`)
- Disabling the metrics server (`statsAddr: off`)
- Disabling the readiness server (`readinessAddr: off`)
- Not configuring SQLite request-log persistence
- Not configuring OpenTelemetry export

## Run

```bash
cargo run --release -- -f examples/embedded/config.yaml
```

or with the existing release binary:

```bash
./target/release/agentgateway -f examples/embedded/config.yaml
```

## Runtime impact

With this configuration, the gateway uses roughly:

- 1 Tokio data-plane worker thread
- About 11 OS threads in total (on the measured Linux environment)
- About 23 MB RSS instead of the default multi-worker startup
- Lower virtual memory usage by avoiding a large per-core runtime

For even smaller binaries, combine this with:

```bash
strip target/release/agentgateway
```

and a release profile using:

```toml
[profile.release]
opt-level = "z"
lto = "thin"
codegen-units = 1
panic = "abort"
strip = true
```
