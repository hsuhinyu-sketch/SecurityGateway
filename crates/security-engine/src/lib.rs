use security_contracts::{ActionRequest, Decision, DecisionEffect};
use security_policy::{evaluate, Policy};

pub fn decide(policies: &[Policy], request: &ActionRequest) -> Decision {
    match evaluate(policies, request) {
        Some(policy) => Decision {
            request_id: request.request_id.clone(),
            effect: policy.effect,
            policy_id: Some(policy.id.clone()),
        },
        None => Decision {
            request_id: request.request_id.clone(),
            effect: DecisionEffect::Allow,
            policy_id: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use security_contracts::{Action, ActionType, Resource, ResourceType, Subject};

    #[test]
    fn deny_matching_tool_call() {
        let request = ActionRequest {
            request_id: "req-1".into(),
            subject: Subject { user_id: Some("alice".into()), agent_id: None },
            action: Action { action_type: ActionType::ToolInvoke, name: "db.delete".into() },
            resource: Resource { id: "prod-db".into(), resource_type: ResourceType::Tool },
        };
        let policies = vec![Policy {
            id: "deny-prod-delete".into(),
            user_id: Some("alice".into()),
            agent_id: None,
            action_type: Some("ToolInvoke".into()),
            action_name: Some("db.delete".into()),
            resource_id: Some("prod-db".into()),
            effect: DecisionEffect::Deny,
            enabled: true,
        }];
        assert_eq!(decide(&policies, &request).effect, DecisionEffect::Deny);
    }
}
