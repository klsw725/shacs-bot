use super::activation_record::{
    ActivationReason, ActivationRecord, ActivationStatus, WorkspaceTrustRef,
};
use super::activation_wire::{parse_document, ActivationDocument};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationMutation {
    Disable,
    Revoke,
}

pub struct ActivationMutationRequest {
    pub activation_ref: String,
    pub workspace_trust_ref: WorkspaceTrustRef,
    pub mutation: ActivationMutation,
    pub occurred_at_unix_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationMutationReceipt {
    pub receipt_ref: String,
    pub activation_ref: String,
    pub previous_status: ActivationStatus,
    pub current_status: ActivationStatus,
    pub reason: ActivationReason,
    pub occurred_at_unix_ms: u64,
}

#[derive(Debug)]
pub enum ActivationStoreError {
    Io(io::Error),
    Malformed(String),
    UnknownSchema(u32),
    Missing,
    OwnerMismatch,
    SourceMismatch,
    InvalidTransition {
        from: ActivationStatus,
        mutation: ActivationMutation,
    },
}

impl fmt::Display for ActivationStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "activation store I/O failed: {error}"),
            Self::Malformed(error) => write!(formatter, "activation store is malformed: {error}"),
            Self::UnknownSchema(version) => {
                write!(formatter, "unknown activation schema: {version}")
            }
            Self::Missing => formatter.write_str("activation record is missing"),
            Self::OwnerMismatch => formatter.write_str("activation owner does not match"),
            Self::SourceMismatch => formatter.write_str("activation source does not match"),
            Self::InvalidTransition { from, mutation } => {
                write!(
                    formatter,
                    "activation mutation {mutation:?} is invalid from {from:?}"
                )
            }
        }
    }
}

impl std::error::Error for ActivationStoreError {}
impl From<io::Error> for ActivationStoreError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct ActivationStore {
    path: PathBuf,
}

impl ActivationStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn put(&self, record: ActivationRecord) -> Result<(), ActivationStoreError> {
        let mut document = self.load()?;
        if let Some(existing) = document
            .records
            .iter()
            .find(|item| item.activation_ref() == record.activation_ref())
        {
            if existing.workspace_trust_ref() != record.workspace_trust_ref() {
                return Err(ActivationStoreError::OwnerMismatch);
            }
            if existing.source_identity() != record.source_identity() {
                return Err(ActivationStoreError::SourceMismatch);
            }
        }
        document
            .records
            .retain(|item| item.activation_ref() != record.activation_ref());
        document.records.push(record);
        self.save(&document)
    }

    pub fn inspect(
        &self,
        activation_ref: &str,
        owner: &WorkspaceTrustRef,
    ) -> Result<ActivationRecord, ActivationStoreError> {
        let document = self.load()?;
        let record = document
            .records
            .iter()
            .find(|item| item.activation_ref() == activation_ref)
            .ok_or(ActivationStoreError::Missing)?;
        if record.workspace_trust_ref() != owner {
            return Err(ActivationStoreError::OwnerMismatch);
        }
        Ok(record.clone())
    }

    pub fn find_current(
        &self,
        resource_ref: &str,
        owner: &WorkspaceTrustRef,
        source_identity: &str,
    ) -> Result<Option<ActivationRecord>, ActivationStoreError> {
        Ok(self.load()?.records.into_iter().find(|record| {
            record.resource_ref() == resource_ref
                && record.workspace_trust_ref() == owner
                && record.source_identity() == source_identity
        }))
    }

    pub fn mutate(
        &self,
        request: ActivationMutationRequest,
    ) -> Result<ActivationMutationReceipt, ActivationStoreError> {
        let mut document = self.load()?;
        let record = document
            .records
            .iter_mut()
            .find(|item| item.activation_ref() == request.activation_ref)
            .ok_or(ActivationStoreError::Missing)?;
        if record.workspace_trust_ref() != &request.workspace_trust_ref {
            return Err(ActivationStoreError::OwnerMismatch);
        }
        let previous_status = record.status();
        let (current_status, reason) = transition(previous_status, request.mutation)?;
        record.transition(current_status, reason);
        let receipt_ref = receipt_ref(
            &request.activation_ref,
            previous_status,
            current_status,
            request.occurred_at_unix_ms,
        );
        let receipt = ActivationMutationReceipt {
            receipt_ref,
            activation_ref: request.activation_ref,
            previous_status,
            current_status,
            reason,
            occurred_at_unix_ms: request.occurred_at_unix_ms,
        };
        document.receipts.push(receipt.clone());
        self.save(&document)?;
        Ok(receipt)
    }

    pub fn receipts(
        &self,
        activation_ref: &str,
    ) -> Result<Vec<ActivationMutationReceipt>, ActivationStoreError> {
        Ok(self
            .load()?
            .receipts
            .into_iter()
            .filter(|receipt| receipt.activation_ref == activation_ref)
            .collect())
    }

    fn load(&self) -> Result<ActivationDocument, ActivationStoreError> {
        if !self.path.exists() {
            return Ok(ActivationDocument::default());
        }
        let bytes = fs::read(&self.path)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| ActivationStoreError::Malformed(error.to_string()))?;
        let (document, migrated) = parse_document(value)?;
        if migrated {
            self.save(&document)?;
        }
        Ok(document)
    }

    fn save(&self, document: &ActivationDocument) -> Result<(), ActivationStoreError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(document)
                .map_err(|error| ActivationStoreError::Malformed(error.to_string()))?,
        )?;
        fs::rename(temporary, &self.path)?;
        Ok(())
    }
}

fn receipt_ref(
    activation_ref: &str,
    previous: ActivationStatus,
    current: ActivationStatus,
    at: u64,
) -> String {
    let value = format!("{activation_ref}:{previous:?}:{current:?}:{at}");
    format!(
        "activation-receipt:sha256:{:x}",
        Sha256::digest(value.as_bytes())
    )
}

fn transition(
    status: ActivationStatus,
    mutation: ActivationMutation,
) -> Result<(ActivationStatus, ActivationReason), ActivationStoreError> {
    match (status, mutation) {
        (ActivationStatus::Active | ActivationStatus::Stale, ActivationMutation::Disable) => {
            Ok((ActivationStatus::Disabled, ActivationReason::UserDisabled))
        }
        (
            ActivationStatus::Active | ActivationStatus::Stale | ActivationStatus::Disabled,
            ActivationMutation::Revoke,
        ) => Ok((ActivationStatus::Revoked, ActivationReason::UserRevoked)),
        (
            ActivationStatus::Disabled | ActivationStatus::Revoked | ActivationStatus::Removed,
            ActivationMutation::Disable,
        )
        | (ActivationStatus::Revoked | ActivationStatus::Removed, ActivationMutation::Revoke) => {
            Err(ActivationStoreError::InvalidTransition {
                from: status,
                mutation,
            })
        }
    }
}
