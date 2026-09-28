use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use shacs_config::{Config, CredentialFamily, ProfileSelectionSource, ProviderConfig};
use shacs_projection::{
    project_spec035_revised_owner_facts, CredentialStatusProjection, DataSurface,
    Spec030RuntimeProjection, Spec030UnavailableReason, Spec031ConstructionError,
    Spec035DecisionProjection, Spec035OwnerSurface, Spec035RevisedOwnerFacts, TraceStatus,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "owner", rename_all = "lowercase")]
pub enum OnboardWizardExternalOwnerFact {
    Spec030 {
        projection: OnboardWizardCredentialObservation,
    },
    Spec031 {
        profile_selection: OnboardWizardProfileSelection,
        declarations: Vec<OnboardWizardCredentialDeclaration>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardWizardCredentialObservation {
    pub credential: CredentialStatusProjection,
    pub unavailable_reason: Option<Spec030UnavailableReason>,
    pub raw_content_possible: bool,
    pub disclosure_surfaces: Vec<DataSurface>,
    pub trace_status: TraceStatus,
    pub decisions: Vec<Spec035DecisionProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardWizardProfileSelection {
    Defaults,
    Available,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardWizardDeclarationOrigin {
    Provider,
    ProviderProfile,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OnboardWizardCredentialDeclaration {
    pub subject_ref: String,
    pub origin: OnboardWizardDeclarationOrigin,
    pub selected_profile: bool,
    pub environment: bool,
    pub local_auth: bool,
    pub local_auth_entry: bool,
    pub command: bool,
    pub literal: bool,
    pub secret_ref: bool,
}

pub(crate) fn project(
    config: &Config,
    projection: &Spec030RuntimeProjection,
) -> Result<Vec<OnboardWizardExternalOwnerFact>, Spec031ConstructionError> {
    let revised = project_spec035_revised_owner_facts(Spec035RevisedOwnerFacts::new(
        Spec035OwnerSurface::Cli,
        projection,
    ))?;
    let profile_selection = match config.resolve_profiles() {
        Ok(profiles) => match profiles.source {
            ProfileSelectionSource::Defaults => OnboardWizardProfileSelection::Defaults,
            ProfileSelectionSource::Configured => OnboardWizardProfileSelection::Available,
        },
        Err(_) => OnboardWizardProfileSelection::Missing,
    };
    let mut declarations = config
        .providers
        .iter()
        .map(|(name, provider)| {
            declaration(name, provider, OnboardWizardDeclarationOrigin::Provider)
        })
        .collect::<Vec<_>>();
    declarations.extend(config.profiles.providers.iter().map(|(name, profile)| {
        let mut declaration = declaration(
            name,
            &ProviderConfig {
                credential_source: profile.credential_source.clone(),
                ..ProviderConfig::default()
            },
            OnboardWizardDeclarationOrigin::ProviderProfile,
        );
        declaration.selected_profile = config.profiles.selection.provider.as_deref() == Some(name);
        declaration
    }));
    Ok(vec![
        OnboardWizardExternalOwnerFact::Spec030 {
            projection: OnboardWizardCredentialObservation {
                credential: projection.credential().clone(),
                unavailable_reason: projection.unavailable_reason(),
                raw_content_possible: projection.disclosure().raw_content_possible,
                disclosure_surfaces: projection.disclosure().surfaces.clone(),
                trace_status: projection.disclosure().trace.status,
                decisions: revised.decisions().to_vec(),
            },
        },
        OnboardWizardExternalOwnerFact::Spec031 {
            profile_selection,
            declarations,
        },
    ])
}

fn declaration(
    name: &str,
    provider: &ProviderConfig,
    origin: OnboardWizardDeclarationOrigin,
) -> OnboardWizardCredentialDeclaration {
    let source = provider.credential_declaration(CredentialFamily::ApiKey, None);
    OnboardWizardCredentialDeclaration {
        subject_ref: format!("declaration:sha256:{:x}", Sha256::digest(name.as_bytes())),
        origin,
        selected_profile: false,
        environment: source.environment.is_some(),
        local_auth: source.local_auth,
        local_auth_entry: provider
            .credential_source
            .as_ref()
            .is_some_and(|source| source.local_auth_entry().is_some()),
        command: source.command.is_some(),
        literal: provider
            .api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty())
            || provider
                .credential_source
                .as_ref()
                .is_some_and(|source| source.literal_enabled()),
        secret_ref: provider.api_key_ref.is_some(),
    }
}

impl fmt::Display for OnboardWizardExternalOwnerFact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spec030 { projection } => {
                write!(
                    formatter,
                    "owner=spec030 {}",
                    serde_json::to_string(projection).map_err(|_| fmt::Error)?
                )
            }
            Self::Spec031 {
                profile_selection,
                declarations,
            } => {
                let selection = match profile_selection {
                    OnboardWizardProfileSelection::Defaults => "defaults",
                    OnboardWizardProfileSelection::Available => "available",
                    OnboardWizardProfileSelection::Missing => "missing",
                };
                write!(formatter, "owner=spec031 capability=credential_declarations profile_selection={selection} count={}", declarations.len())?;
                for declaration in declarations {
                    let origin = match declaration.origin {
                        OnboardWizardDeclarationOrigin::Provider => "provider",
                        OnboardWizardDeclarationOrigin::ProviderProfile => "provider_profile",
                    };
                    write!(formatter, "\n  - {} origin={origin} selected_profile={} environment={} local_auth={} local_auth_entry={} command={} literal={} secret_ref={}",
                        declaration.subject_ref, declaration.selected_profile, declaration.environment,
                        declaration.local_auth, declaration.local_auth_entry, declaration.command, declaration.literal, declaration.secret_ref)?;
                }
                Ok(())
            }
        }
    }
}
