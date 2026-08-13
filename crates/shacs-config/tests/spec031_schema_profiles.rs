use serde_json::json;
use shacs_config::{
    classify_config_schema, migrate_config_value, AuthSourceConfig, Config, ConfigSchemaState,
    ContextProfileConfig, CredentialFamily, ProfileSelection, ProfileSelectionSource,
    ProfilesConfig, ProviderConfig, ProviderCredentialSourceConfig, ProviderProfileConfig,
    TrustedRuntimeConfig, CURRENT_CONFIG_SCHEMA_VERSION,
};
use std::collections::BTreeMap;

#[test]
fn spec031_schema_classification_is_deterministic_for_legacy_current_and_future() {
    let legacy = json!({"providers": {}});
    let current = json!({"schemaVersion": CURRENT_CONFIG_SCHEMA_VERSION});
    let future = json!({"schemaVersion": CURRENT_CONFIG_SCHEMA_VERSION + 1});

    assert_eq!(
        classify_config_schema(&legacy),
        Ok(ConfigSchemaState::Legacy)
    );
    assert_eq!(
        classify_config_schema(&current),
        Ok(ConfigSchemaState::Current)
    );
    assert_eq!(
        classify_config_schema(&future),
        Ok(ConfigSchemaState::FutureUnsupported {
            found: CURRENT_CONFIG_SCHEMA_VERSION + 1,
        })
    );
}

#[test]
fn spec031_direct_migration_does_not_downgrade_future_schema() {
    let mut future = json!({"schemaVersion": CURRENT_CONFIG_SCHEMA_VERSION + 1});

    let migrations = migrate_config_value(&mut future);

    assert_eq!(future["schemaVersion"], CURRENT_CONFIG_SCHEMA_VERSION + 1);
    assert!(migrations.is_empty());
}

#[test]
fn spec031_legacy_migration_sets_current_schema_without_replacing_compatibility_data() {
    let mut value = json!({
        "agents": {"defaults": {"workspace": "~/kept", "sessionTtlMinutes": 7}},
        "providers": {"openrouter": {
            "apiKey": "${OPENROUTER_API_KEY}",
            "apiKeyRef": {
                "kind": "secret_ref",
                "schema_version": 1,
                "ref_id": "sec_kept",
                "source_kind": "env",
                "locator": {"kind": "env_var", "name": "OPENROUTER_API_KEY"},
                "owner": "spec035-config-profile",
                "scope": "provider-auth",
                "locator_digest": "sha256:locator",
                "staleness_token": "sha256:spec035-open",
                "safe_summary": {"label": "env:OPENROUTER_API_KEY", "required": true}
            }
        }},
        "profiles": {"selection": {"provider": "daily"}}
    });

    let migrations = migrate_config_value(&mut value);

    assert_eq!(value["schemaVersion"], CURRENT_CONFIG_SCHEMA_VERSION);
    assert_eq!(value["agents"]["defaults"]["workspace"], "~/kept");
    assert_eq!(
        value["providers"]["openrouter"]["apiKey"],
        "${OPENROUTER_API_KEY}"
    );
    assert_eq!(
        value["providers"]["openrouter"]["apiKeyRef"]["ref_id"],
        "sec_kept"
    );
    assert_eq!(value["profiles"]["selection"]["provider"], "daily");
    assert!(migrations
        .iter()
        .any(|migration| migration.key == "schemaVersion"));
}

#[test]
fn spec031_named_profiles_resolve_each_selected_domain_with_configured_provenance() {
    let config = Config {
        profiles: ProfilesConfig {
            providers: BTreeMap::from([(
                "daily".to_owned(),
                ProviderProfileConfig {
                    provider: "openrouter".to_owned(),
                    model: Some("anthropic/claude-sonnet".to_owned()),
                    credential_source: None,
                },
            )]),
            trusted_runtimes: BTreeMap::from([(
                "local".to_owned(),
                TrustedRuntimeConfig::default(),
            )]),
            contexts: BTreeMap::from([(
                "focused".to_owned(),
                ContextProfileConfig {
                    files: vec!["CONTEXT.md".to_owned()],
                },
            )]),
            selection: ProfileSelection {
                provider: Some("daily".to_owned()),
                trusted_runtime: Some("local".to_owned()),
                context: Some("focused".to_owned()),
            },
        },
        ..Config::default()
    };

    let resolved = config.resolve_profiles().expect("named profiles resolve");

    assert_eq!(resolved.provider.expect("provider").name, "daily");
    assert_eq!(
        resolved.trusted_runtime.expect("trusted runtime").name,
        "local"
    );
    assert_eq!(resolved.context.expect("context").name, "focused");
    assert_eq!(resolved.source, ProfileSelectionSource::Configured);
}

#[test]
fn spec031_missing_named_profile_is_reported_with_domain_and_name() {
    let config: Config = serde_json::from_value(json!({
        "profiles": {"selection": {"context": "missing"}}
    }))
    .expect("config parses");

    let error = config
        .resolve_profiles()
        .expect_err("missing profile is rejected");

    assert_eq!(error.kind, shacs_config::ProfileKind::Context);
    assert_eq!(error.name, "missing");
}

#[test]
fn spec031_empty_selection_reports_default_provenance() {
    let config = Config::default();
    let resolved = config.resolve_profiles().expect("default profiles resolve");

    assert_eq!(resolved.source, ProfileSelectionSource::Defaults);
    assert!(resolved.provider.is_none());
    assert!(resolved.trusted_runtime.is_none());
    assert!(resolved.context.is_none());
}

#[test]
fn spec031_auth_source_declarations_roundtrip_and_feed_spec030_fields() {
    let source = ProviderCredentialSourceConfig {
        schema_version: 1,
        sources: vec![
            AuthSourceConfig::Environment {
                name: "CUSTOM_KEY".to_owned(),
            },
            AuthSourceConfig::LocalAuthEntry {
                entry: "openrouter".to_owned(),
            },
            AuthSourceConfig::Command {
                command: "credential-helper".to_owned(),
            },
            AuthSourceConfig::Literal,
        ],
        environment: None,
        local_auth: true,
        command: None,
    };

    let serialized = serde_json::to_value(&source).expect("source serializes");
    let roundtrip: ProviderCredentialSourceConfig =
        serde_json::from_value(serialized).expect("source deserializes");

    assert_eq!(roundtrip, source);
    assert_eq!(roundtrip.environment_name(), Some("CUSTOM_KEY"));
    assert_eq!(roundtrip.local_auth_entry(), Some("openrouter"));
    assert_eq!(roundtrip.command_line(), Some("credential-helper"));
    assert!(roundtrip.literal_enabled());
}

#[test]
fn spec031_typed_auth_sources_feed_existing_spec030_declaration_without_reowning_precedence() {
    let provider = ProviderConfig {
        api_key: Some("literal-value".to_owned()),
        credential_source: Some(ProviderCredentialSourceConfig {
            schema_version: 1,
            sources: vec![
                AuthSourceConfig::Environment {
                    name: "PROFILE_KEY".to_owned(),
                },
                AuthSourceConfig::LocalAuthEntry {
                    entry: "profile-entry".to_owned(),
                },
                AuthSourceConfig::Command {
                    command: "profile-helper".to_owned(),
                },
                AuthSourceConfig::Literal,
            ],
            environment: None,
            local_auth: true,
            command: None,
        }),
        ..ProviderConfig::default()
    };

    let declaration = provider.credential_declaration(CredentialFamily::ApiKey, None);

    assert_eq!(declaration.environment.as_deref(), Some("PROFILE_KEY"));
    assert!(declaration.local_auth);
    assert_eq!(declaration.command.as_deref(), Some("profile-helper"));
    assert_eq!(provider.api_key.as_deref(), Some("literal-value"));
}
