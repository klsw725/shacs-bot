use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::{OnboardWizardProviderRef, OnboardWizardResumeState};
use crate::CliError;

pub(crate) fn parse_provider_id(value: &str) -> Result<(), CliError> {
    if !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        })
    {
        return Ok(());
    }
    Err(CliError::InvalidArguments(
        "provider id must use lowercase ASCII letters, digits, hyphen, or underscore".to_owned(),
    ))
}

pub(crate) fn parse_env_ref(value: &str) -> Result<(), CliError> {
    let parts = value.split('_').collect::<Vec<_>>();
    let structurally_valid = (1..=64).contains(&value.len())
        && value.as_bytes()[0].is_ascii_uppercase()
        && value.as_bytes()[value.len() - 1] != b'_'
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && parts.len() >= 2
        && parts.iter().all(|part| !part.is_empty())
        && !value.contains("__");
    if structurally_valid && !looks_like_token_words(&parts) {
        return Ok(());
    }
    Err(CliError::InvalidArguments(
        "secret ref must be a bounded uppercase environment variable name, not a raw value"
            .to_owned(),
    ))
}

pub(crate) fn add_provider_ref(
    config: &Value,
    state: &mut OnboardWizardResumeState,
    provider: String,
    env_var: String,
) -> Result<(), CliError> {
    if provider_has_key_material(
        config
            .get("providers")
            .and_then(|providers| providers.get(&provider)),
    ) {
        return Err(existing_key_error(&provider));
    }
    state
        .provider_secret_refs
        .retain(|item| item.provider != provider);
    state.provider_secret_refs.push(OnboardWizardProviderRef {
        provider,
        source_kind: "env".to_owned(),
        locator: env_var,
    });
    state
        .provider_secret_refs
        .sort_by(|left, right| left.provider.cmp(&right.provider));
    Ok(())
}

pub(crate) fn apply_refs(
    config: &mut Value,
    state: &OnboardWizardResumeState,
) -> Result<(), CliError> {
    let root = config.as_object_mut().ok_or_else(|| {
        CliError::InvalidArguments("config JSON root must be an object".to_owned())
    })?;
    let provider_map = root
        .entry("providers".to_owned())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            CliError::InvalidArguments("config `providers` must be a JSON object".to_owned())
        })?;
    for item in &state.provider_secret_refs {
        let provider = provider_map
            .entry(item.provider.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if provider_has_key_material(Some(provider)) {
            return Err(existing_key_error(&item.provider));
        }
        let provider_object = provider.as_object_mut().ok_or_else(|| {
            CliError::InvalidArguments(format!(
                "config provider `{}` must be a JSON object",
                item.provider
            ))
        })?;
        provider_object.insert(
            "apiKeyRef".to_owned(),
            secret_ref_value(&item.provider, &item.locator),
        );
    }
    Ok(())
}

fn provider_has_key_material(provider: Option<&Value>) -> bool {
    ["apiKeyRef", "api_key_ref", "apiKey", "api_key"]
        .iter()
        .any(|key| provider.and_then(|value| value.get(*key)).is_some())
}

fn secret_ref_value(provider: &str, env_var: &str) -> Value {
    json!({
        "kind": "secret_ref", "schema_version": 1,
        "ref_id": format!("sec_onboard_{}_{}", provider.replace('-', "_"), env_var.to_ascii_lowercase()),
        "source_kind": "env", "locator": {"kind": "env_var", "name": env_var},
        "owner": "spec035-config-profile", "scope": "provider-auth",
        "created_by": "onboard-wizard", "created_at_ms": crate::now_millis(),
        "locator_digest": format!("sha256:{:x}", Sha256::digest(format!("env:{env_var}").as_bytes())),
        "staleness_token": "sha256:spec035-open", "safe_summary": {"label": format!("env:{env_var}"), "required": true}
    })
}

fn existing_key_error(provider: &str) -> CliError {
    CliError::InvalidArguments(format!("provider `{provider}` already has api key material or a secret ref; wizard will not overwrite it"))
}

fn looks_like_token_words(parts: &[&str]) -> bool {
    let joined = parts.join("_");
    joined.starts_with("SK_")
        || joined.contains("_SECRET")
        || joined.contains("TOKEN")
        || joined.contains("BEARER")
        || joined.contains("JWT")
}
