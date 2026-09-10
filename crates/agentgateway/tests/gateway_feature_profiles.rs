#[cfg(not(feature = "gateway-a2a"))]
mod disabled_a2a {
	use agentgateway::resource_manager::ResourceFetcher;
	use agentgateway::types::agent::ListenerTarget;
	use agentgateway::types::local::NormalizedLocalConfig;

	#[tokio::test]
	async fn rejects_a2a_configuration_not_selected_at_build_time() {
		let config = agentgateway::config::parse_config("{}".to_string(), None).unwrap();
		let error = NormalizedLocalConfig::from(
			&config,
			&ResourceFetcher::files_only(),
			ListenerTarget {
				gateway_name: "name".into(),
				gateway_namespace: "ns".into(),
				listener_name: None,
				port: None,
			},
			r#"
aiSystems:
- name: restricted
  a2a:
    port: 4101
    backend:
      host: 127.0.0.1:8100
"#,
		)
		.await
		.expect_err("an LLM-only profile must reject A2A configuration");
		assert!(
			error
				.to_string()
				.contains("requires A2A Gateway, which is not compiled into this gateway profile")
		);
	}
}
