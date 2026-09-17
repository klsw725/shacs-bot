use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TasksValidationErrorKind {
    UnsafeLocator,
    DetailOwnerMismatch,
    MisleadingSuccess,
    InvalidAction,
    TooManyRows,
    DuplicateOwnerLocator,
    CoverageMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035TasksValidationError {
    kind: Spec035TasksValidationErrorKind,
}

impl Spec035TasksValidationError {
    pub(super) const fn new(kind: Spec035TasksValidationErrorKind) -> Self {
        Self { kind }
    }

    pub const fn kind(&self) -> Spec035TasksValidationErrorKind {
        self.kind
    }
}

impl Display for Spec035TasksValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "invalid Spec035 tasks projection: {:?}",
            self.kind
        )
    }
}

impl std::error::Error for Spec035TasksValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec035TasksParseErrorKind {
    InvalidJson,
    InvalidSchema,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec035TasksParseError {
    kind: Spec035TasksParseErrorKind,
}

impl Spec035TasksParseError {
    pub const fn kind(&self) -> Spec035TasksParseErrorKind {
        self.kind
    }

    pub(super) const fn invalid_schema() -> Self {
        Self {
            kind: Spec035TasksParseErrorKind::InvalidSchema,
        }
    }

    pub(super) fn from_serde(error: serde_json::Error) -> Self {
        if error.is_syntax() || error.is_eof() {
            Self {
                kind: Spec035TasksParseErrorKind::InvalidJson,
            }
        } else {
            Self::invalid_schema()
        }
    }

    pub(super) const fn from_validation(_error: Spec035TasksValidationError) -> Self {
        Self::invalid_schema()
    }
}

impl Display for Spec035TasksParseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Spec035TasksParseErrorKind::InvalidJson => {
                formatter.write_str("invalid Spec035 tasks JSON")
            }
            Spec035TasksParseErrorKind::InvalidSchema => {
                formatter.write_str("invalid Spec035 tasks schema")
            }
        }
    }
}

impl std::error::Error for Spec035TasksParseError {}
