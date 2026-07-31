pub type ConfigSecretRef = shacs_redaction::SecretRef;

pub fn provider_secret_refs(config: &crate::Config) -> Vec<(&str, &ConfigSecretRef)> {
    config
        .providers
        .iter()
        .filter_map(|(name, provider)| {
            provider
                .api_key_ref
                .as_ref()
                .map(|secret_ref| (name.as_str(), secret_ref))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::{load_config_with_env, provider_secret_refs, Config, LoadOptions};

    #[test]
    fn provider_secret_ref_config_preserves_ref_without_resolving_raw_value(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config: Config = serde_json::from_value(json!({
            "providers": {
                "openrouter": {
                    "apiKeyRef": {
                        "kind": "secret_ref",
                        "schema_version": 1,
                        "ref_id": "sec_prd001_env_happy",
                        "source_kind": "env",
                        "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET"},
                        "owner": "spec035-config-profile",
                        "scope": "provider-auth",
                        "created_by": "config-profile",
                        "created_at_ms": 0,
                        "locator_digest": "sha256:locator",
                        "staleness_token": "sha256:owner-state",
                        "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
                    }
                }
            }
        }))?;

        let refs = provider_secret_refs(&config);
        let serialized = serde_json::to_string(&config)?;

        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, "openrouter");
        assert_eq!(refs[0].1.ref_id.as_str(), "sec_prd001_env_happy");
        assert!(!serialized.contains("sk-prd001-raw-fixture-value"));
        Ok(())
    }

    #[test]
    fn load_config_preserves_provider_secret_refs_when_env_resolution_is_enabled(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let config_path = root.path().join("config.json");
        std::fs::write(
            &config_path,
            serde_json::to_string_pretty(&json!({
                "providers": {
                    "openrouter": {
                        "apiKeyRef": {
                            "kind": "secret_ref",
                            "schema_version": 1,
                            "ref_id": "sec_prd001_env_happy",
                            "source_kind": "env",
                            "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET"},
                            "owner": "spec035-config-profile",
                            "scope": "provider-auth",
                            "locator_digest": "sha256:locator",
                            "staleness_token": "sha256:owner-state",
                            "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
                        }
                    }
                }
            }))?,
        )?;
        let env = BTreeMap::from([(
            "SHACS_PRD001_HAPPY_SECRET".to_owned(),
            "sk-prd001-raw-fixture-value".to_owned(),
        )]);

        let bundle = load_config_with_env(
            LoadOptions {
                config_path: Some(config_path),
                workspace_override: None,
                resolve_env: true,
                write_back_migrations: false,
            },
            &env,
        )?;

        let serialized = serde_json::to_string(&bundle.config)?;
        assert_eq!(provider_secret_refs(&bundle.config).len(), 1);
        assert!(!serialized.contains("sk-prd001-raw-fixture-value"));
        Ok(())
    }

    #[test]
    fn config_deserialize_rejects_provider_secret_ref_raw_fields_and_bad_staleness() {
        let raw_value = json!({
            "providers": {
                "openrouter": {
                    "apiKeyRef": {
                        "kind": "secret_ref",
                        "schema_version": 1,
                        "ref_id": "sec_prd001_env_bad",
                        "source_kind": "env",
                        "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET", "env_value": "hunter2"},
                        "owner": "spec035-config-profile",
                        "scope": "provider-auth",
                        "locator_digest": "sha256:locator",
                        "staleness_token": "sha256:owner-state",
                        "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
                    }
                }
            }
        });
        assert!(serde_json::from_value::<Config>(raw_value).is_err());

        let missing_staleness = json!({
            "providers": {
                "openrouter": {
                    "apiKeyRef": {
                        "kind": "secret_ref",
                        "schema_version": 1,
                        "ref_id": "sec_prd001_env_bad",
                        "source_kind": "env",
                        "locator": {"kind": "env_var", "name": "SHACS_PRD001_HAPPY_SECRET"},
                        "owner": "spec035-config-profile",
                        "scope": "provider-auth",
                        "locator_digest": "sha256:locator",
                        "safe_summary": {"label": "env:SHACS_PRD001_HAPPY_SECRET", "required": true}
                    }
                }
            }
        });
        assert!(serde_json::from_value::<Config>(missing_staleness).is_err());
    }
}
