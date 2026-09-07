use security_contracts::{ActionRequest, DecisionEffect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub id: String,
    pub user_id: Option<String>,
    pub agent_id: Option<String>,
    pub action_type: Option<String>,
    pub action_name: Option<String>,
    pub resource_id: Option<String>,
    pub effect: DecisionEffect,
    pub enabled: bool,
}

impl Policy {
    pub fn matches(&self, request: &ActionRequest) -> bool {
        if !self.enabled {
            return false;
        }
        if self.user_id.as_deref().is_some_and(|v| request.subject.user_id.as_deref() != Some(v)) {
            return false;
        }
        if self.agent_id.as_deref().is_some_and(|v| request.subject.agent_id.as_deref() != Some(v)) {
            return false;
        }
        if self.action_type.as_deref().is_some_and(|v| format!("{:?}", request.action.action_type) != v) {
            return false;
        }
        if self.action_name.as_deref().is_some_and(|v| request.action.name != v) {
            return false;
        }
        if self.resource_id.as_deref().is_some_and(|v| request.resource.id != v) {
            return false;
        }
        true
    }
}

pub fn evaluate<'a>(policies: &'a [Policy], request: &ActionRequest) -> Option<&'a Policy> {
    policies.iter().find(|policy| policy.matches(request))
}
