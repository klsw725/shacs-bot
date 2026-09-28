use super::{
    Spec035TransportCapability, Spec035TransportCapabilitySupport, Spec035TransportClientHello,
    Spec035TransportGeneration, Spec035TransportOffer, Spec035TransportOfferInput,
    Spec035TransportSchemaVersion, Spec035TransportServerHello, Spec035TransportUnsupportedReason,
    Spec035TransportValidationError,
};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportMutationStatus {
    Blocked,
    Unsupported,
}

impl Spec035TransportMutationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "blocked",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec035TransportMutationReason {
    SchemaMismatch,
    CapabilityUnavailable,
}

impl Spec035TransportMutationReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SchemaMismatch => "schema_mismatch",
            Self::CapabilityUnavailable => "capability_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec035TransportMutationRejection {
    status: Spec035TransportMutationStatus,
    reason: Spec035TransportMutationReason,
}

impl Spec035TransportMutationRejection {
    pub const fn schema_mismatch() -> Self {
        Self {
            status: Spec035TransportMutationStatus::Blocked,
            reason: Spec035TransportMutationReason::SchemaMismatch,
        }
    }

    pub const fn capability_unavailable() -> Self {
        Self {
            status: Spec035TransportMutationStatus::Unsupported,
            reason: Spec035TransportMutationReason::CapabilityUnavailable,
        }
    }

    pub const fn status(self) -> Spec035TransportMutationStatus {
        self.status
    }

    pub const fn reason(self) -> Spec035TransportMutationReason {
        self.reason
    }
}

impl Display for Spec035TransportMutationRejection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: {}",
            self.status.as_str(),
            self.reason.as_str()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TransportNegotiationError {
    SchemaMismatch(Spec035TransportValidationError),
    Unsupported(Spec035TransportUnsupportedReason),
}

impl Spec035TransportNegotiationError {
    pub const fn is_schema_mismatch(&self) -> bool {
        matches!(self, Self::SchemaMismatch(_))
    }

    pub const fn unsupported_reason(&self) -> Option<Spec035TransportUnsupportedReason> {
        match self {
            Self::SchemaMismatch(_) => None,
            Self::Unsupported(reason) => Some(*reason),
        }
    }
}

impl Display for Spec035TransportNegotiationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SchemaMismatch(source) => {
                write!(
                    formatter,
                    "Spec035 transport schema negotiation failed: {source}"
                )
            }
            Self::Unsupported(Spec035TransportUnsupportedReason::CapabilityUnavailable) => {
                formatter.write_str("unsupported: capability_unavailable")
            }
        }
    }
}

impl std::error::Error for Spec035TransportNegotiationError {}

impl Spec035TransportOffer {
    pub fn negotiate_mutation(
        &self,
        client: &Spec035TransportClientHello,
        capability: Spec035TransportCapability,
    ) -> Result<Spec035TransportServerHello, Spec035TransportNegotiationError> {
        let server = self
            .select(client)
            .map_err(Spec035TransportNegotiationError::SchemaMismatch)?;
        match server.capability_support(capability) {
            Spec035TransportCapabilitySupport::Supported => Ok(server),
            Spec035TransportCapabilitySupport::Unsupported { reason } => {
                Err(Spec035TransportNegotiationError::Unsupported(reason))
            }
        }
    }
}

pub fn negotiate_spec035_task_mutation(
    client: &Spec035TransportClientHello,
    capability: Spec035TransportCapability,
    generation: &str,
) -> Result<Spec035TransportServerHello, Spec035TransportMutationRejection> {
    let schema = Spec035TransportSchemaVersion::try_new(1)
        .map_err(|_| Spec035TransportMutationRejection::schema_mismatch())?;
    let generation = Spec035TransportGeneration::try_new(generation)
        .map_err(|_| Spec035TransportMutationRejection::schema_mismatch())?;
    let offer = Spec035TransportOffer::try_new(Spec035TransportOfferInput {
        schema_versions: vec![schema],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskResume,
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskRecover,
        ],
        generation,
    })
    .map_err(|_| Spec035TransportMutationRejection::schema_mismatch())?;
    offer
        .negotiate_mutation(client, capability)
        .map_err(|error| match error {
            Spec035TransportNegotiationError::SchemaMismatch(_) => {
                Spec035TransportMutationRejection::schema_mismatch()
            }
            Spec035TransportNegotiationError::Unsupported(
                Spec035TransportUnsupportedReason::CapabilityUnavailable,
            ) => Spec035TransportMutationRejection::capability_unavailable(),
        })
}

impl Spec035TransportServerHello {
    pub fn capability_support(
        &self,
        capability: Spec035TransportCapability,
    ) -> Spec035TransportCapabilitySupport {
        self.capability_decisions()
            .iter()
            .find(|decision| decision.capability() == capability)
            .map_or(
                Spec035TransportCapabilitySupport::Unsupported {
                    reason: Spec035TransportUnsupportedReason::CapabilityUnavailable,
                },
                |decision| decision.support(),
            )
    }
}
