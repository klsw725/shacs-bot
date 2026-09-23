use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_execution_io::decode;
use serde::{de::IgnoredAny, Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigAcceptance {
    schema: AcceptanceSchema,
    incident: String,
    disposition: AcceptanceDisposition,
    preincident_backup_available: bool,
    user_confirmed_intent: IgnoredAny,
    authorized_changes: Vec<AuthorizedChange>,
    verification: Verification,
    limits: Vec<String>,
}

#[derive(Deserialize)]
enum AcceptanceSchema {
    #[serde(rename = "spec035.config_user_baseline_acceptance.v1")]
    V1,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AcceptanceDisposition {
    UserAcceptedRepairedBaseline,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorizedChange {
    key: String,
    value: IgnoredAny,
    backup: IgnoredAny,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Verification {
    each_edit_changed_only_authorized_value: bool,
    config_permissions: String,
    provider_matches_user_intent: bool,
    discord_enabled: bool,
    discord_credential_field_nonempty: bool,
    credential_values_disclosed: bool,
}

#[derive(Debug, Serialize)]
pub struct AcceptedIncident {
    pub disposition: AcceptanceDisposition,
    pub preincident_restoration: Unverified,
    pub credential_validity: Unverified,
    pub limits: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unverified {
    Unverified,
}

pub(super) fn accepted_incident(bytes: &[u8]) -> Result<AcceptedIncident, Error> {
    let receipt: ConfigAcceptance = decode(bytes)?;
    match receipt.schema {
        AcceptanceSchema::V1 => {}
    }
    let _ = receipt.user_confirmed_intent;
    let keys: Vec<_> = receipt
        .authorized_changes
        .iter()
        .map(|change| {
            let _ = (&change.value, &change.backup);
            change.key.as_str()
        })
        .collect();
    let verification = receipt.verification;
    if receipt.incident != "default-config-rewrite"
        || receipt.preincident_backup_available
        || keys != ["agents.defaults.workspace", "agents.defaults.model"]
        || !verification.each_edit_changed_only_authorized_value
        || verification.credential_values_disclosed
        || verification.config_permissions != "0600"
        || !verification.provider_matches_user_intent
        || !verification.discord_enabled
        || !verification.discord_credential_field_nonempty
        || receipt.limits.is_empty()
    {
        return Err(Error::InvalidCoverageEvidence);
    }
    Ok(AcceptedIncident {
        disposition: receipt.disposition,
        preincident_restoration: Unverified::Unverified,
        credential_validity: Unverified::Unverified,
        limits: receipt.limits,
    })
}
