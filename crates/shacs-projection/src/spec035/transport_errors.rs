use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TransportValidationErrorKind {
    InvalidIdentifier,
    InvalidSchemaVersion,
    MissingSchemaVersion,
    TooManySchemaVersions,
    TooManyCapabilities,
    DuplicateCapabilityDecision,
    UnsupportedSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035TransportValidationError {
    kind: Spec035TransportValidationErrorKind,
}

impl Spec035TransportValidationError {
    pub(super) const fn new(kind: Spec035TransportValidationErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> Spec035TransportValidationErrorKind {
        self.kind
    }
}

impl Display for Spec035TransportValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid Spec035 transport contract: {:?}",
            self.kind
        )
    }
}

impl std::error::Error for Spec035TransportValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TransportParseErrorKind {
    InvalidJson,
    InvalidSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035TransportParseError {
    kind: Spec035TransportParseErrorKind,
}

impl Spec035TransportParseError {
    pub const fn kind(&self) -> Spec035TransportParseErrorKind {
        self.kind
    }

    pub(super) fn from_serde(error: serde_json::Error) -> Self {
        if error.is_syntax() || error.is_eof() {
            Self {
                kind: Spec035TransportParseErrorKind::InvalidJson,
            }
        } else {
            Self::invalid_schema()
        }
    }

    pub(super) const fn from_validation(_error: Spec035TransportValidationError) -> Self {
        Self::invalid_schema()
    }

    const fn invalid_schema() -> Self {
        Self {
            kind: Spec035TransportParseErrorKind::InvalidSchema,
        }
    }
}

impl Display for Spec035TransportParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Spec035TransportParseErrorKind::InvalidJson => {
                formatter.write_str("invalid Spec035 transport JSON")
            }
            Spec035TransportParseErrorKind::InvalidSchema => {
                formatter.write_str("invalid Spec035 transport schema")
            }
        }
    }
}

impl std::error::Error for Spec035TransportParseError {}
