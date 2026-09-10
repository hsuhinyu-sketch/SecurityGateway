//! First-party runtime namespace for the AI Gateway Platform.
//!
//! The implementation is re-exported from the AgentGateway-derived compatibility runtime during
//! migration. New platform crates must depend on this package instead of `agentgateway` directly.

pub use agentgateway::*;
