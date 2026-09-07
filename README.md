# SecurityGateway

SecurityGateway is a standalone, AI-native gateway for securing and governing
agent-to-model, agent-to-tool, and agent-to-agent traffic.

## Capabilities

- Unified LLM routing through an OpenAI-compatible API.
- MCP and A2A connectivity for tools, services, and agents.
- Authentication, authorization, rate limiting, TLS, guardrails, and
  OpenTelemetry observability.
- Local YAML configuration with runtime reload support.

## Architecture

The standalone runtime loads local YAML configuration, normalizes it into the
runtime store, and serves traffic through the proxy layer:

```text
YAML -> runtime -> store -> proxy
```

See [architecture/standalone-runtime.md](architecture/standalone-runtime.md)
for module responsibilities and [examples/](examples/) for configuration
samples.

## Build and test

```bash
cargo check -p agentgateway-app
cargo test -p agentgateway
```

## License

Licensed under the [Apache License 2.0](LICENSE).
