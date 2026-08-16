use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035MediaValidationErrorKind {
    MissingOwnerFact,
    DuplicateOwnerFact,
    UnsafeOwnerFact,
    InconsistentState,
    MisleadingSuccess,
    OwnerLineageMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035MediaValidationError {
    kind: Spec035MediaValidationErrorKind,
}

impl Spec035MediaValidationError {
    pub(super) const fn new(kind: Spec035MediaValidationErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> Spec035MediaValidationErrorKind {
        self.kind
    }
}

impl Display for Spec035MediaValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid Spec035 media projection: {:?}",
            self.kind
        )
    }
}

impl std::error::Error for Spec035MediaValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035MediaParseErrorKind {
    InvalidJson,
    InvalidSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035MediaParseError {
    kind: Spec035MediaParseErrorKind,
}

impl Spec035MediaParseError {
    pub const fn kind(&self) -> Spec035MediaParseErrorKind {
        self.kind
    }

    pub(super) const fn invalid_schema() -> Self {
        Self {
            kind: Spec035MediaParseErrorKind::InvalidSchema,
        }
    }

    pub(super) fn from_serde(error: serde_json::Error) -> Self {
        if error.is_syntax() || error.is_eof() {
            Self {
                kind: Spec035MediaParseErrorKind::InvalidJson,
            }
        } else {
            Self::invalid_schema()
        }
    }

    pub(super) const fn from_validation(_error: Spec035MediaValidationError) -> Self {
        Self::invalid_schema()
    }
}

impl Display for Spec035MediaParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Spec035MediaParseErrorKind::InvalidJson => {
                formatter.write_str("invalid Spec035 media JSON")
            }
            Spec035MediaParseErrorKind::InvalidSchema => {
                formatter.write_str("invalid Spec035 media schema")
            }
        }
    }
}

impl std::error::Error for Spec035MediaParseError {}
