use super::redaction::{
    construction_error, sanitize_lineage, sanitize_reason, Spec031ConstructionViolation,
};
use super::Spec031ConstructionError;
use super::{
    Spec031Availability, Spec031Capability, Spec031Lineage, Spec031ProjectionKind, Spec031Reason,
    Spec031SchemaVersion, Spec031Severity, Spec031Source,
};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct Spec031Envelope {
    schema_version: Spec031SchemaVersion,
    kind: Spec031ProjectionKind,
    state: Spec031Availability,
    severity: Spec031Severity,
    reason: Spec031Reason,
    lineage: Spec031Lineage,
    source: Spec031Source,
    capability: Spec031Capability,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    children: Vec<Spec031Envelope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec031EnvelopeInput {
    pub schema_version: Spec031SchemaVersion,
    pub kind: Spec031ProjectionKind,
    pub state: Spec031Availability,
    pub severity: Spec031Severity,
    pub reason: Spec031Reason,
    pub lineage: Spec031Lineage,
    pub source: Spec031Source,
    pub capability: Spec031Capability,
    pub children: Vec<Spec031Envelope>,
}

impl Spec031Envelope {
    pub fn try_new(input: Spec031EnvelopeInput) -> Result<Self, Spec031ConstructionError> {
        if !capability_matches_kind(input.kind, &input.capability) {
            return Err(construction_error(
                "capability.kind",
                Spec031ConstructionViolation::CapabilityFamilyMismatch,
            ));
        }
        Ok(Self {
            schema_version: input.schema_version,
            kind: input.kind,
            state: input.state,
            severity: input.severity,
            reason: sanitize_reason(input.reason)?,
            lineage: sanitize_lineage(input.lineage)?,
            source: input.source,
            capability: input.capability,
            children: input.children,
        })
    }

    pub const fn schema_version(&self) -> Spec031SchemaVersion {
        self.schema_version
    }

    pub const fn kind(&self) -> Spec031ProjectionKind {
        self.kind
    }

    pub const fn state(&self) -> Spec031Availability {
        self.state
    }

    pub const fn severity(&self) -> Spec031Severity {
        self.severity
    }

    pub const fn reason(&self) -> &Spec031Reason {
        &self.reason
    }

    pub const fn lineage(&self) -> &Spec031Lineage {
        &self.lineage
    }

    pub const fn source(&self) -> &Spec031Source {
        &self.source
    }

    pub const fn capability(&self) -> &Spec031Capability {
        &self.capability
    }

    pub fn children(&self) -> &[Spec031Envelope] {
        &self.children
    }

    pub fn parse_json(input: &str) -> Result<Self, Spec031ParseError> {
        serde_json::from_str::<Spec031EnvelopeWire>(input)
            .map_err(Spec031ParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    pub fn from_json_value(value: serde_json::Value) -> Result<Self, Spec031ParseError> {
        serde_json::from_value::<Spec031EnvelopeWire>(value)
            .map_err(Spec031ParseError::from_serde)
            .and_then(Self::try_from_wire)
    }

    fn try_from_wire(wire: Spec031EnvelopeWire) -> Result<Self, Spec031ParseError> {
        let children = wire
            .children
            .into_iter()
            .map(Self::try_from_wire)
            .collect::<Result<Vec<_>, _>>()?;
        Self::try_new(Spec031EnvelopeInput {
            schema_version: wire.schema_version,
            kind: wire.kind,
            state: wire.state,
            severity: wire.severity,
            reason: wire.reason,
            lineage: wire.lineage,
            source: wire.source,
            capability: wire.capability,
            children,
        })
        .map_err(Spec031ParseError::from_construction)
    }
}

fn capability_matches_kind(kind: Spec031ProjectionKind, capability: &Spec031Capability) -> bool {
    matches!(
        (kind, capability),
        (
            Spec031ProjectionKind::Session,
            Spec031Capability::Session(_)
        ) | (Spec031ProjectionKind::Turn, Spec031Capability::Turn(_))
            | (
                Spec031ProjectionKind::Subagent,
                Spec031Capability::Subagent(_)
            )
            | (
                Spec031ProjectionKind::Approval,
                Spec031Capability::Approval(_)
            )
            | (Spec031ProjectionKind::Tool, Spec031Capability::Tool(_))
            | (
                Spec031ProjectionKind::Context,
                Spec031Capability::Context(_)
            )
            | (Spec031ProjectionKind::Plugin, Spec031Capability::Plugin(_))
            | (Spec031ProjectionKind::App, Spec031Capability::App(_))
            | (Spec031ProjectionKind::Media, Spec031Capability::Media(_))
            | (
                Spec031ProjectionKind::Diagnostics,
                Spec031Capability::Diagnostics(_)
            )
            | (
                Spec031ProjectionKind::ReleaseEvidence,
                Spec031Capability::ReleaseEvidence(_)
            )
            | (
                Spec031ProjectionKind::Readiness,
                Spec031Capability::Readiness(_)
            )
            | (
                Spec031ProjectionKind::Progress,
                Spec031Capability::Progress(_)
            )
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ParseErrorKind {
    InvalidJson,
    InvalidSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec031ParseError {
    kind: Spec031ParseErrorKind,
}

impl Spec031ParseError {
    pub const fn kind(&self) -> Spec031ParseErrorKind {
        self.kind
    }

    fn from_serde(error: serde_json::Error) -> Self {
        let kind = if error.is_syntax() || error.is_eof() {
            Spec031ParseErrorKind::InvalidJson
        } else {
            Spec031ParseErrorKind::InvalidSchema
        };
        Self { kind }
    }

    fn from_construction(_error: Spec031ConstructionError) -> Self {
        Self {
            kind: Spec031ParseErrorKind::InvalidSchema,
        }
    }
}

impl fmt::Display for Spec031ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            Spec031ParseErrorKind::InvalidJson => write!(formatter, "invalid Spec031 JSON"),
            Spec031ParseErrorKind::InvalidSchema => write!(formatter, "invalid Spec031 schema"),
        }
    }
}

impl Error for Spec031ParseError {}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct Spec031EnvelopeWire {
    schema_version: Spec031SchemaVersion,
    kind: Spec031ProjectionKind,
    state: Spec031Availability,
    severity: Spec031Severity,
    reason: Spec031Reason,
    lineage: Spec031Lineage,
    source: Spec031Source,
    capability: Spec031Capability,
    #[serde(default)]
    children: Vec<Spec031EnvelopeWire>,
}
