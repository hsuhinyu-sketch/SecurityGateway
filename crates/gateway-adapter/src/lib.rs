use security_contracts::{Action, ActionRequest, ActionType, Resource, ResourceType, Subject};

pub fn model_invoke(
    request_id: impl Into<String>,
    user_id: Option<String>,
    agent_id: Option<String>,
    model_id: impl Into<String>,
) -> ActionRequest {
    ActionRequest {
        request_id: request_id.into(),
        subject: Subject { user_id, agent_id },
        action: Action {
            action_type: ActionType::ModelInvoke,
            name: "invoke".into(),
        },
        resource: Resource {
            id: model_id.into(),
            resource_type: ResourceType::Model,
        },
    }
}

pub fn tool_invoke(
    request_id: impl Into<String>,
    user_id: Option<String>,
    agent_id: Option<String>,
    tool_name: impl Into<String>,
) -> ActionRequest {
    let tool_name = tool_name.into();
    ActionRequest {
        request_id: request_id.into(),
        subject: Subject { user_id, agent_id },
        action: Action {
            action_type: ActionType::ToolInvoke,
            name: tool_name.clone(),
        },
        resource: Resource {
            id: tool_name,
            resource_type: ResourceType::Tool,
        },
    }
}
