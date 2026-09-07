use chrono::Utc;
use security_contracts::{ActionRequest, Decision, SecurityEvent};

pub fn event_from_decision(event_id: impl Into<String>, request: &ActionRequest, decision: &Decision) -> SecurityEvent {
    SecurityEvent {
        event_id: event_id.into(),
        request_id: request.request_id.clone(),
        subject: request.subject.clone(),
        action: request.action.clone(),
        resource: request.resource.clone(),
        decision: decision.effect,
        policy_id: decision.policy_id.clone(),
        timestamp: Utc::now(),
    }
}
