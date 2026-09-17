use super::{
    Spec035TransportCapability, Spec035TransportCapabilitySupport, Spec035TransportClientId,
    Spec035TransportGeneration, Spec035TransportParseError, Spec035TransportResumeCursor,
    Spec035TransportSchemaVersion, Spec035TransportUnsupportedReason,
    Spec035TransportValidationError, Spec035TransportValidationErrorKind,
    SPEC035_TRANSPORT_CAPABILITIES_MAX, SPEC035_TRANSPORT_SCHEMA_VERSIONS_MAX,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035TransportClientHelloInput {
    pub client_id: Spec035TransportClientId,
    pub schema_versions: Vec<Spec035TransportSchemaVersion>,
    pub mutation_capabilities: Vec<Spec035TransportCapability>,
    pub resume: Option<Spec035TransportResumeCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportClientHello {
    client_id: Spec035TransportClientId,
    schema_versions: Vec<Spec035TransportSchemaVersion>,
    mutation_capabilities: Vec<Spec035TransportCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resume: Option<Spec035TransportResumeCursor>,
}

impl Spec035TransportClientHello {
    pub fn try_new(
        mut input: Spec035TransportClientHelloInput,
    ) -> Result<Self, Spec035TransportValidationError> {
        normalize_versions(&mut input.schema_versions)?;
        normalize_capabilities(&mut input.mutation_capabilities)?;
        Ok(Self {
            client_id: input.client_id,
            schema_versions: input.schema_versions,
            mutation_capabilities: input.mutation_capabilities,
            resume: input.resume,
        })
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec035TransportParseError> {
        serde_json::from_str::<Spec035TransportClientHelloWire>(input)
            .map_err(Spec035TransportParseError::from_serde)
            .and_then(|wire| {
                Self::try_new(Spec035TransportClientHelloInput {
                    client_id: wire.client_id,
                    schema_versions: wire.schema_versions,
                    mutation_capabilities: wire.mutation_capabilities,
                    resume: wire.resume,
                })
                .map_err(Spec035TransportParseError::from_validation)
            })
    }

    pub const fn client_id(&self) -> &Spec035TransportClientId {
        &self.client_id
    }

    pub fn schema_versions(&self) -> &[Spec035TransportSchemaVersion] {
        &self.schema_versions
    }

    pub fn mutation_capabilities(&self) -> &[Spec035TransportCapability] {
        &self.mutation_capabilities
    }

    pub const fn resume(&self) -> Option<&Spec035TransportResumeCursor> {
        self.resume.as_ref()
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035TransportClientHelloWire {
    client_id: Spec035TransportClientId,
    schema_versions: Vec<Spec035TransportSchemaVersion>,
    mutation_capabilities: Vec<Spec035TransportCapability>,
    #[serde(default)]
    resume: Option<Spec035TransportResumeCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035TransportOfferInput {
    pub schema_versions: Vec<Spec035TransportSchemaVersion>,
    pub mutation_capabilities: Vec<Spec035TransportCapability>,
    pub generation: Spec035TransportGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec035TransportOffer {
    schema_versions: Vec<Spec035TransportSchemaVersion>,
    mutation_capabilities: Vec<Spec035TransportCapability>,
    generation: Spec035TransportGeneration,
}

impl Spec035TransportOffer {
    pub fn try_new(
        mut input: Spec035TransportOfferInput,
    ) -> Result<Self, Spec035TransportValidationError> {
        normalize_versions(&mut input.schema_versions)?;
        normalize_capabilities(&mut input.mutation_capabilities)?;
        Ok(Self {
            schema_versions: input.schema_versions,
            mutation_capabilities: input.mutation_capabilities,
            generation: input.generation,
        })
    }

    pub fn select(
        &self,
        client: &Spec035TransportClientHello,
    ) -> Result<Spec035TransportServerHello, Spec035TransportValidationError> {
        let schema_version = client
            .schema_versions
            .iter()
            .rev()
            .find(|version| self.schema_versions.binary_search(version).is_ok())
            .copied()
            .ok_or_else(|| {
                Spec035TransportValidationError::new(
                    Spec035TransportValidationErrorKind::UnsupportedSchema,
                )
            })?;
        let capability_decisions = client
            .mutation_capabilities
            .iter()
            .copied()
            .map(|capability| Spec035TransportCapabilityDecision {
                capability,
                support: if self
                    .mutation_capabilities
                    .binary_search(&capability)
                    .is_ok()
                {
                    Spec035TransportCapabilitySupport::Supported
                } else {
                    Spec035TransportCapabilitySupport::Unsupported {
                        reason: Spec035TransportUnsupportedReason::CapabilityUnavailable,
                    }
                },
            })
            .collect();
        Ok(Spec035TransportServerHello {
            schema_version,
            generation: self.generation.clone(),
            capability_decisions,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportCapabilityDecision {
    capability: Spec035TransportCapability,
    support: Spec035TransportCapabilitySupport,
}

impl Spec035TransportCapabilityDecision {
    pub const fn capability(&self) -> Spec035TransportCapability {
        self.capability
    }

    pub const fn support(&self) -> Spec035TransportCapabilitySupport {
        self.support
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec035TransportServerHello {
    schema_version: Spec035TransportSchemaVersion,
    generation: Spec035TransportGeneration,
    capability_decisions: Vec<Spec035TransportCapabilityDecision>,
}

impl Spec035TransportServerHello {
    pub fn parse_json(input: &str) -> Result<Self, Spec035TransportParseError> {
        let mut wire = serde_json::from_str::<Spec035TransportServerHelloWire>(input)
            .map_err(Spec035TransportParseError::from_serde)?;
        if wire.capability_decisions.len() > SPEC035_TRANSPORT_CAPABILITIES_MAX {
            return Err(Spec035TransportParseError::from_validation(
                Spec035TransportValidationError::new(
                    Spec035TransportValidationErrorKind::TooManyCapabilities,
                ),
            ));
        }
        wire.capability_decisions
            .sort_unstable_by_key(Spec035TransportCapabilityDecision::capability);
        if wire
            .capability_decisions
            .windows(2)
            .any(|pair| pair[0].capability() == pair[1].capability())
        {
            return Err(Spec035TransportParseError::from_validation(
                Spec035TransportValidationError::new(
                    Spec035TransportValidationErrorKind::DuplicateCapabilityDecision,
                ),
            ));
        }
        Ok(Self {
            schema_version: wire.schema_version,
            generation: wire.generation,
            capability_decisions: wire.capability_decisions,
        })
    }

    pub const fn schema_version(&self) -> Spec035TransportSchemaVersion {
        self.schema_version
    }

    pub const fn generation(&self) -> &Spec035TransportGeneration {
        &self.generation
    }

    pub fn capability_decisions(&self) -> &[Spec035TransportCapabilityDecision] {
        &self.capability_decisions
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec035TransportServerHelloWire {
    schema_version: Spec035TransportSchemaVersion,
    generation: Spec035TransportGeneration,
    capability_decisions: Vec<Spec035TransportCapabilityDecision>,
}

fn normalize_versions(
    versions: &mut Vec<Spec035TransportSchemaVersion>,
) -> Result<(), Spec035TransportValidationError> {
    if versions.is_empty() {
        return Err(Spec035TransportValidationError::new(
            Spec035TransportValidationErrorKind::MissingSchemaVersion,
        ));
    }
    if versions.len() > SPEC035_TRANSPORT_SCHEMA_VERSIONS_MAX {
        return Err(Spec035TransportValidationError::new(
            Spec035TransportValidationErrorKind::TooManySchemaVersions,
        ));
    }
    if versions.iter().any(|version| version.as_u32() == 0) {
        return Err(Spec035TransportValidationError::new(
            Spec035TransportValidationErrorKind::InvalidSchemaVersion,
        ));
    }
    versions.sort_unstable();
    versions.dedup();
    Ok(())
}

fn normalize_capabilities(
    capabilities: &mut Vec<Spec035TransportCapability>,
) -> Result<(), Spec035TransportValidationError> {
    if capabilities.len() > SPEC035_TRANSPORT_CAPABILITIES_MAX {
        return Err(Spec035TransportValidationError::new(
            Spec035TransportValidationErrorKind::TooManyCapabilities,
        ));
    }
    capabilities.sort_unstable();
    capabilities.dedup();
    Ok(())
}
