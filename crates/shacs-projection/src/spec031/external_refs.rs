use super::redaction::{
    construction_error, Spec031ConstructionError, Spec031ConstructionViolation,
};
use serde::{Deserialize, Deserializer, Serialize};

macro_rules! external_ref_newtype {
    ($name:ident, $field:literal, $parser:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn try_new(value: &str) -> Result<Self, Spec031ConstructionError> {
                $parser(value).map_err(|()| {
                    construction_error($field, Spec031ConstructionViolation::UnsafeOpaqueRef)
                })?;
                Ok(Self(value.to_owned()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::try_new(&value).map_err(serde::de::Error::custom)
            }
        }
    };
}

external_ref_newtype!(
    Spec031ExternalOwnerRef,
    "external_owner.opaque_ref",
    parse_owner_ref
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ExternalRefKind {
    Spec032App,
    Spec034Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spec031ExternalReceiptRefKind {
    Spec032,
    Spec034,
}

impl Spec031ExternalOwnerRef {
    pub fn kind(&self) -> Spec031ExternalRefKind {
        let (scheme, segments) =
            parse_external_ref(self.as_str()).expect("typed owner ref grammar");
        match (scheme, segments.as_slice()) {
            ("spec032", ["app", "lifecycle" | "readiness", _]) => {
                Spec031ExternalRefKind::Spec032App
            }
            ("spec034", ["media", "artifact" | "analyzer", _]) => {
                Spec031ExternalRefKind::Spec034Media
            }
            _ => unreachable!("typed owner ref grammar"),
        }
    }
}

impl Spec031ExternalOwnerReceiptRef {
    pub fn kind(&self) -> Spec031ExternalReceiptRefKind {
        let (scheme, segments) =
            parse_external_ref(self.as_str()).expect("typed receipt ref grammar");
        match (scheme, segments.as_slice()) {
            ("spec032", ["receipt", _]) => Spec031ExternalReceiptRefKind::Spec032,
            ("spec034", ["receipt", _]) => Spec031ExternalReceiptRefKind::Spec034,
            _ => unreachable!("typed receipt ref grammar"),
        }
    }
}
external_ref_newtype!(
    Spec031ExternalOwnerReceiptRef,
    "external_owner.receipt_ref",
    parse_receipt_ref
);

fn parse_owner_ref(value: &str) -> Result<(), ()> {
    let (scheme, segments) = parse_external_ref(value)?;
    match (scheme, segments.as_slice()) {
        ("spec032", ["app", "lifecycle" | "readiness", identifier]) => {
            validate_identifier(identifier)
        }
        ("spec034", ["media", "artifact" | "analyzer", identifier]) => {
            validate_identifier(identifier)
        }
        ("spec032" | "spec034", _) => Err(()),
        (_, _) => Err(()),
    }
}

fn parse_receipt_ref(value: &str) -> Result<(), ()> {
    let (scheme, segments) = parse_external_ref(value)?;
    match (scheme, segments.as_slice()) {
        ("spec032" | "spec034", ["receipt", identifier]) => validate_identifier(identifier),
        ("spec032" | "spec034", _) => Err(()),
        (_, _) => Err(()),
    }
}

fn parse_external_ref(value: &str) -> Result<(&str, Vec<&str>), ()> {
    if value.is_empty()
        || value.len() > 160
        || value.contains(['?', '#', '@', '=', '%', '\\'])
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(());
    }
    let (scheme, path) = value.split_once("://").ok_or(())?;
    if !matches!(scheme, "spec032" | "spec034") || path.contains(':') {
        return Err(());
    }
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return Err(());
    }
    Ok((scheme, segments))
}

fn validate_identifier(value: &str) -> Result<(), ()> {
    if !(1..=64).contains(&value.len())
        || matches!(value, "." | "..")
        || is_reserved_identifier(value)
        || !value.chars().all(is_identifier_char)
    {
        Err(())
    } else {
        Ok(())
    }
}

fn is_identifier_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
}

fn is_reserved_identifier(value: &str) -> bool {
    let normalized: String = value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(
        normalized.as_str(),
        "body"
            | "mediabytes"
            | "payload"
            | "prompt"
            | "promptbytes"
            | "rawbody"
            | "rawbytes"
            | "rawpayload"
            | "rawprompt"
            | "secret"
            | "secrettoken"
            | "token"
    )
}
