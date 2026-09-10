# Configuration

Agentgateway has three forms of configuration:

## Static Configuration

**Static configuration** is set exactly once early in the process lifecycle.
This is set by environment variables or a YAML/JSON configuration (typically a file, but can also be passed as inline bytes into the command line).
This information is really about global settings like logging configurations, ports to use, etc.
All routing, policies, backends, etc are not configurable here.

## Local Configuration

**Local configuration** is configured via a file (YAML/JSON) and can define the full feature set of agentgateway (backends, routes, policies, etc).
The local configuration uses a file watch to dynamically reload changes.
The local configuration is translated into the internal representation (IR) used by the proxy at runtime.

In some cases, the IR and the local configuration are identical. In other cases, there are trivial re-mappings to make the usage more ergonomic.
Others are more broad differences, allow things like fetching JWKS from URLs, or creating a backend + policy with a simple expression like `host: https://example.com`.
