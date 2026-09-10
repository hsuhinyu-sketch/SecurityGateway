use audit_core::{AuditSink, event_from_decision};
use chrono::Duration;
use security_contracts::{ActionRequest, ActionType, Decision, DecisionEffect};
use security_engine::decide;
use security_policy::Policy;
use uuid::Uuid;

use crate::{Capability, CapabilityBroker, CapabilityError};

/// A normalized authorization decision provider. It can be backed by local policies,
/// an external PDP, or a signed policy cache without changing the pipeline.
pub trait Authorizer: Send + Sync {
	fn decide(&self, request: &ActionRequest) -> Decision;
}

/// The default PoC authorizer: local policy evaluation with deny-by-default semantics.
pub struct PolicyAuthorizer {
	policies: Vec<Policy>,
}

impl PolicyAuthorizer {
	pub fn new(policies: Vec<Policy>) -> Self {
		Self { policies }
	}
}

impl Authorizer for PolicyAuthorizer {
	fn decide(&self, request: &ActionRequest) -> Decision {
		decide(&self.policies, request)
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlDenial {
	pub control_id: String,
	pub reason: String,
}

/// A composable policy-enforcement stage. Controls are deterministic: they operate on
/// normalized identity, action, resource, and optional Tool arguments, never model output.
pub trait SecurityControl: Send + Sync {
	fn id(&self) -> &str;

	fn check(
		&self,
		request: &ActionRequest,
		arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial>;
}

/// Ensures that the AI request retains the identities required by the deployment.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredIdentity {
	pub user: bool,
	pub agent: bool,
	pub tenant: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolArgumentsConfig {
	pub tool_name: String,
	#[serde(default)]
	pub required_fields: Vec<String>,
}

/// Declares a Tool as high risk. Until an external approval provider is installed, this is a
/// fail-closed gate rather than an implicit approval.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequiredToolApprovalConfig {
	pub tool_name: String,
}

/// A direct, user-to-Agent delegation grant. Its identifier must be present in verified
/// request identity, so a caller cannot select a broader configured grant by itself.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDelegationConfig {
	pub id: String,
	pub user_id: String,
	pub agent_id: String,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub tenant_id: Option<String>,
	/// An empty list grants no operations.
	#[serde(default)]
	pub scopes: Vec<DelegationScope>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub not_before: Option<chrono::DateTime<chrono::Utc>>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One permitted operation inside a delegation grant. Omitted match fields are wildcards;
/// a grant still needs at least one scope to permit anything.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationScope {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub action_type: Option<ActionType>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub action_name: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub resource_id: Option<String>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub resource_type: Option<security_contracts::ResourceType>,
}

/// YAML/JSON-friendly composition of the built-in PoC bricks. The presence of this
/// configuration enables the pipeline; an empty policy list is intentionally fail-closed.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SecurityPipelineConfig {
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub required_identity: Option<RequiredIdentity>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub policies: Vec<Policy>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub required_tool_arguments: Vec<RequiredToolArgumentsConfig>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub required_tool_approvals: Vec<RequiredToolApprovalConfig>,
	/// Enables direct user-to-Agent delegation validation for requests carrying an Agent.
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub delegations: Vec<AgentDelegationConfig>,
}

impl SecurityPipelineConfig {
	pub fn build<S: AuditSink>(&self, audit: S) -> SecurityPipeline<PolicyAuthorizer, S> {
		let mut pipeline = SecurityPipeline::new(PolicyAuthorizer::new(self.policies.clone()), audit);
		if let Some(identity) = self.required_identity {
			pipeline = pipeline.add_identity_control(identity);
		}
		for requirement in &self.required_tool_arguments {
			pipeline = pipeline.add_action_control(RequiredToolArguments::new(
				requirement.tool_name.clone(),
				requirement.required_fields.clone(),
			));
		}
		for requirement in &self.required_tool_approvals {
			pipeline =
				pipeline.add_action_control(RequiredToolApproval::new(requirement.tool_name.clone()));
		}
		if !self.delegations.is_empty() {
			pipeline = pipeline.add_identity_control(AgentDelegation::new(self.delegations.clone()));
		}
		pipeline
	}
}

impl RequiredIdentity {
	pub const fn user_agent_tenant() -> Self {
		Self {
			user: true,
			agent: true,
			tenant: true,
		}
	}
}

/// Validates a direct delegation from the authenticated user to the authenticated Agent.
/// Authorization remains a separate mandatory stage: a delegation only narrows the
/// operations an already-authorized Agent may perform for that user.
#[derive(Debug, Clone)]
pub struct AgentDelegation {
	delegations: Vec<AgentDelegationConfig>,
}

impl AgentDelegation {
	pub fn new(delegations: Vec<AgentDelegationConfig>) -> Self {
		Self { delegations }
	}

	fn matches_grant(
		grant: &AgentDelegationConfig,
		request: &ActionRequest,
		now: chrono::DateTime<chrono::Utc>,
	) -> bool {
		let subject = &request.subject;
		grant.id == subject.delegation_id.as_deref().unwrap_or_default()
			&& grant.user_id == subject.user_id.as_deref().unwrap_or_default()
			&& grant.agent_id == subject.agent_id.as_deref().unwrap_or_default()
			&& grant
				.tenant_id
				.as_deref()
				.is_none_or(|tenant| subject.tenant_id.as_deref() == Some(tenant))
			&& grant.not_before.is_none_or(|not_before| now >= not_before)
			&& grant.expires_at.is_none_or(|expires_at| now < expires_at)
			&& grant.scopes.iter().any(|scope| scope.matches(request))
	}
}

impl DelegationScope {
	fn matches(&self, request: &ActionRequest) -> bool {
		self
			.action_type
			.is_none_or(|action_type| action_type == request.action.action_type)
			&& self
				.action_name
				.as_deref()
				.is_none_or(|name| name == request.action.name)
			&& self
				.resource_id
				.as_deref()
				.is_none_or(|id| id == request.resource.id)
			&& self
				.resource_type
				.is_none_or(|resource_type| resource_type == request.resource.resource_type)
	}
}

impl SecurityControl for AgentDelegation {
	fn id(&self) -> &str {
		"agent-delegation"
	}

	fn check(
		&self,
		request: &ActionRequest,
		_arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial> {
		if request.subject.agent_id.is_none() {
			return Ok(());
		}
		if request.subject.user_id.is_none() {
			return Err(ControlDenial {
				control_id: self.id().into(),
				reason: "an Agent action requires a delegating user identity".into(),
			});
		}
		if request.subject.delegation_id.is_none() {
			return Err(ControlDenial {
				control_id: self.id().into(),
				reason: "an Agent action requires a verified delegationId".into(),
			});
		}
		if self
			.delegations
			.iter()
			.any(|grant| Self::matches_grant(grant, request, chrono::Utc::now()))
		{
			Ok(())
		} else {
			Err(ControlDenial {
				control_id: self.id().into(),
				reason: "no active delegation grant permits this Agent operation".into(),
			})
		}
	}
}

impl SecurityControl for RequiredIdentity {
	fn id(&self) -> &str {
		"required-identity"
	}

	fn check(
		&self,
		request: &ActionRequest,
		_arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial> {
		let missing = [
			(self.user && request.subject.user_id.is_none(), "userId"),
			(self.agent && request.subject.agent_id.is_none(), "agentId"),
			(
				self.tenant && request.subject.tenant_id.is_none(),
				"tenantId",
			),
		]
		.into_iter()
		.filter_map(|(missing, name)| missing.then_some(name))
		.collect::<Vec<_>>();

		if missing.is_empty() {
			Ok(())
		} else {
			Err(ControlDenial {
				control_id: self.id().into(),
				reason: format!(
					"missing required identity attributes: {}",
					missing.join(", ")
				),
			})
		}
	}
}

/// Requires named arguments for a particular Tool. This is intentionally a small,
/// reusable PoC validator; production implementations can replace it with JSON Schema
/// and domain-specific resource ownership checks.
#[derive(Debug, Clone)]
pub struct RequiredToolArguments {
	tool_name: String,
	required_fields: Vec<String>,
}

impl RequiredToolArguments {
	pub fn new(
		tool_name: impl Into<String>,
		required_fields: impl IntoIterator<Item = impl Into<String>>,
	) -> Self {
		Self {
			tool_name: tool_name.into(),
			required_fields: required_fields.into_iter().map(Into::into).collect(),
		}
	}
}

impl SecurityControl for RequiredToolArguments {
	fn id(&self) -> &str {
		"required-tool-arguments"
	}

	fn check(
		&self,
		request: &ActionRequest,
		arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial> {
		if request.action.action_type != ActionType::ToolInvoke || request.action.name != self.tool_name
		{
			return Ok(());
		}
		let Some(arguments) = arguments.and_then(serde_json::Value::as_object) else {
			return Err(ControlDenial {
				control_id: self.id().into(),
				reason: format!(
					"tool '{}' requires an object argument payload",
					self.tool_name
				),
			});
		};
		let missing = self
			.required_fields
			.iter()
			.filter(|field| {
				arguments
					.get(field.as_str())
					.is_none_or(serde_json::Value::is_null)
			})
			.map(String::as_str)
			.collect::<Vec<_>>();
		if missing.is_empty() {
			Ok(())
		} else {
			Err(ControlDenial {
				control_id: self.id().into(),
				reason: format!(
					"tool '{}' is missing required arguments: {}",
					self.tool_name,
					missing.join(", ")
				),
			})
		}
	}
}

/// A high-risk Tool gate used when the deployment has not yet connected an approval authority.
///
/// This is intentionally fail-closed: declaring a Tool as high risk must never silently grant
/// permission merely because an approval integration is absent.
#[derive(Debug, Clone)]
pub struct RequiredToolApproval {
	tool_name: String,
}

impl RequiredToolApproval {
	pub fn new(tool_name: impl Into<String>) -> Self {
		Self {
			tool_name: tool_name.into(),
		}
	}
}

impl SecurityControl for RequiredToolApproval {
	fn id(&self) -> &str {
		"required-tool-approval"
	}

	fn check(
		&self,
		request: &ActionRequest,
		_arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial> {
		if request.action.action_type != ActionType::ToolInvoke || request.action.name != self.tool_name
		{
			return Ok(());
		}
		Err(ControlDenial {
			control_id: self.id().into(),
			reason: format!(
				"high-risk tool '{}' requires an external approval grant",
				self.tool_name
			),
		})
	}
}

/// External approval is another replaceable brick: an implementation may call a human
/// workflow, a vehicle interlock, or a business transaction service.
pub trait ApprovalProvider: Send + Sync {
	fn approved(&self, request: &ActionRequest, arguments: &serde_json::Value) -> bool;
}

#[derive(Debug, Clone, Copy)]
pub struct StaticApproval(pub bool);

impl ApprovalProvider for StaticApproval {
	fn approved(&self, _request: &ActionRequest, _arguments: &serde_json::Value) -> bool {
		self.0
	}
}

pub struct ToolApproval<P> {
	tool_name: String,
	provider: P,
}

impl<P> ToolApproval<P> {
	pub fn new(tool_name: impl Into<String>, provider: P) -> Self {
		Self {
			tool_name: tool_name.into(),
			provider,
		}
	}
}

impl<P: ApprovalProvider> SecurityControl for ToolApproval<P> {
	fn id(&self) -> &str {
		"tool-approval"
	}

	fn check(
		&self,
		request: &ActionRequest,
		arguments: Option<&serde_json::Value>,
	) -> Result<(), ControlDenial> {
		if request.action.action_type != ActionType::ToolInvoke || request.action.name != self.tool_name
		{
			return Ok(());
		}
		let Some(arguments) = arguments else {
			return Err(ControlDenial {
				control_id: self.id().into(),
				reason: format!("tool '{}' requires approval arguments", self.tool_name),
			});
		};
		if self.provider.approved(request, arguments) {
			Ok(())
		} else {
			Err(ControlDenial {
				control_id: self.id().into(),
				reason: format!("tool '{}' was not approved", self.tool_name),
			})
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
	Denied(Decision),
	DeniedByControl {
		decision: Decision,
		denial: ControlDenial,
	},
	Capability(CapabilityError),
	CapabilityUnavailable,
}

/// Ordered, composable security enforcement for one normalized Gateway request.
///
/// Identity controls run first, authorization is mandatory and independent, then action
/// controls run. Capability issuance is opt-in, allowing read-only services to use the
/// same pipeline without introducing a credential broker.
pub struct SecurityPipeline<A, S> {
	identity_controls: Vec<Box<dyn SecurityControl>>,
	authorizer: A,
	action_controls: Vec<Box<dyn SecurityControl>>,
	capabilities: Option<CapabilityBroker>,
	audit: S,
}

impl<A, S> SecurityPipeline<A, S>
where
	A: Authorizer,
	S: AuditSink,
{
	pub fn new(authorizer: A, audit: S) -> Self {
		Self {
			identity_controls: Vec::new(),
			authorizer,
			action_controls: Vec::new(),
			capabilities: None,
			audit,
		}
	}

	pub fn add_identity_control(mut self, control: impl SecurityControl + 'static) -> Self {
		self.identity_controls.push(Box::new(control));
		self
	}

	pub fn add_action_control(mut self, control: impl SecurityControl + 'static) -> Self {
		self.action_controls.push(Box::new(control));
		self
	}

	pub fn with_capability_broker(mut self) -> Self {
		self.capabilities = Some(CapabilityBroker::default());
		self
	}

	pub fn authorize(
		&self,
		request: &ActionRequest,
		arguments: Option<&serde_json::Value>,
	) -> Result<Decision, GatewayError> {
		for control in &self.identity_controls {
			if let Err(denial) = control.check(request, arguments) {
				return Err(self.reject_control(request, denial));
			}
		}

		let decision = self.authorizer.decide(request);
		if decision.effect == DecisionEffect::Deny {
			self.record(request, &decision);
			return Err(GatewayError::Denied(decision));
		}

		for control in &self.action_controls {
			if let Err(denial) = control.check(request, arguments) {
				return Err(self.reject_control(request, denial));
			}
		}

		self.record(request, &decision);
		Ok(decision)
	}

	pub fn authorize_tool(
		&self,
		request: &ActionRequest,
		arguments: &serde_json::Value,
		ttl: Duration,
	) -> Result<Capability, GatewayError> {
		self.authorize(request, Some(arguments))?;
		let Some(capabilities) = &self.capabilities else {
			return Err(GatewayError::CapabilityUnavailable);
		};
		Ok(capabilities.issue(request, arguments, ttl))
	}

	pub fn consume_tool_capability(
		&self,
		token: &str,
		request: &ActionRequest,
		arguments: &serde_json::Value,
	) -> Result<Capability, GatewayError> {
		let Some(capabilities) = &self.capabilities else {
			return Err(GatewayError::CapabilityUnavailable);
		};
		capabilities
			.consume(token, request, arguments)
			.map_err(GatewayError::Capability)
	}

	fn reject_control(&self, request: &ActionRequest, denial: ControlDenial) -> GatewayError {
		let decision = Decision {
			request_id: request.request_id.clone(),
			effect: DecisionEffect::Deny,
			policy_id: Some(format!("control:{}", denial.control_id)),
		};
		self.record(request, &decision);
		GatewayError::DeniedByControl { decision, denial }
	}

	fn record(&self, request: &ActionRequest, decision: &Decision) {
		self.audit.record(event_from_decision(
			Uuid::new_v4().to_string(),
			request,
			decision,
		));
	}
}

#[cfg(test)]
mod tests {
	use audit_core::InMemoryAuditSink;
	use chrono::Duration;
	use security_contracts::{
		Action, ActionRequest, ActionType, DecisionEffect, Resource, ResourceType, Subject,
	};

	use super::*;

	fn request() -> ActionRequest {
		ActionRequest {
			request_id: "pipeline-request".into(),
			subject: Subject {
				user_id: Some("alice".into()),
				agent_id: Some("maintenance-agent".into()),
				tenant_id: Some("tenant-a".into()),
				delegation_id: None,
			},
			action: Action {
				action_type: ActionType::ToolInvoke,
				name: "db.delete".into(),
			},
			resource: Resource {
				id: "prod-db".into(),
				resource_type: ResourceType::Tool,
			},
		}
	}

	fn allow_policy() -> Policy {
		Policy {
			id: "allow-maintenance-delete".into(),
			priority: 0,
			tenant_id: Some("tenant-a".into()),
			user_id: Some("alice".into()),
			agent_id: Some("maintenance-agent".into()),
			action_type: Some(ActionType::ToolInvoke),
			action_name: Some("db.delete".into()),
			resource_id: Some("prod-db".into()),
			resource_type: Some(ResourceType::Tool),
			effect: DecisionEffect::Allow,
			enabled: true,
		}
	}

	#[test]
	fn pipeline_composes_identity_validation_authorization_and_capabilities() {
		let audit = InMemoryAuditSink::default();
		let pipeline = SecurityPipeline::new(PolicyAuthorizer::new(vec![allow_policy()]), &audit)
			.add_identity_control(RequiredIdentity::user_agent_tenant())
			.add_action_control(RequiredToolArguments::new("db.delete", ["recordId"]))
			.add_action_control(ToolApproval::new("db.delete", StaticApproval(true)))
			.with_capability_broker();
		let request = request();
		let arguments = serde_json::json!({ "recordId": "42" });

		let capability = pipeline
			.authorize_tool(&request, &arguments, Duration::seconds(30))
			.unwrap();
		assert!(
			pipeline
				.consume_tool_capability(&capability.token, &request, &arguments)
				.is_ok()
		);
		assert_eq!(audit.events()[0].decision, DecisionEffect::Allow);
	}

	#[test]
	fn pipeline_audits_an_action_control_denial() {
		let audit = InMemoryAuditSink::default();
		let pipeline = SecurityPipeline::new(PolicyAuthorizer::new(vec![allow_policy()]), &audit)
			.add_identity_control(RequiredIdentity::user_agent_tenant())
			.add_action_control(RequiredToolArguments::new("db.delete", ["recordId"]));
		let request = request();

		let error = pipeline
			.authorize(&request, Some(&serde_json::json!({})))
			.unwrap_err();
		assert!(matches!(
			error,
			GatewayError::DeniedByControl { ref denial, .. }
				if denial.control_id == "required-tool-arguments"
		));
		let event = audit.events().pop().unwrap();
		assert_eq!(event.decision, DecisionEffect::Deny);
		assert_eq!(
			event.policy_id.as_deref(),
			Some("control:required-tool-arguments")
		);
	}

	#[test]
	fn yaml_facing_config_builds_the_expected_security_bricks() {
		let config: SecurityPipelineConfig = serde_json::from_value(serde_json::json!({
			"requiredIdentity": { "user": true, "agent": true, "tenant": true },
			"policies": [{
				"id": "allow-maintenance-delete",
				"tenantId": "tenant-a",
				"userId": "alice",
				"agentId": "maintenance-agent",
				"actionType": "toolInvoke",
				"actionName": "db.delete",
				"resourceId": "prod-db",
				"resourceType": "tool",
				"effect": "allow",
				"enabled": true
			}],
			"requiredToolArguments": [{
				"toolName": "db.delete",
				"requiredFields": ["recordId"]
			}]
		}))
		.unwrap();
		let audit = InMemoryAuditSink::default();
		let pipeline = config.build(&audit);

		assert!(
			pipeline
				.authorize(&request(), Some(&serde_json::json!({ "recordId": "42" })))
				.is_ok()
		);
	}
}
