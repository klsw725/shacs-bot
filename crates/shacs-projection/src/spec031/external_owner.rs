use super::external_fact::ExternalOwnerFact;
use super::Spec031Availability;
use serde::{Deserialize, Serialize};

pub const MISSING_EXTERNAL_OWNER_EVIDENCE: Spec031ExternalOwnerReasonCode =
    Spec031ExternalOwnerReasonCode::MissingExternalOwnerEvidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOwnerSpec {
    Spec032,
    Spec034,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExternalCapability {
    App,
    Media,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalOwnerStatus {
    Ready,
    Degraded,
    Blocked,
    Unavailable,
    Included,
    Skipped,
    Unsupported,
    ExtractionFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Spec031ExternalOwnerReasonCode {
    OwnerRecorded,
    MissingExternalOwnerEvidence,
    StaleExternalOwnerEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec031ExternalProjectionItem {
    pub owner: ExternalOwnerSpec,
    pub capability: Spec031ExternalCapability,
    pub availability: Spec031Availability,
    pub reason_code: Spec031ExternalOwnerReasonCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_status: Option<ExternalOwnerStatus>,
    pub opaque_ref: Option<String>,
    pub receipt_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec031ClosureBlocker {
    pub owner: ExternalOwnerSpec,
    pub capability: Spec031ExternalCapability,
    pub reason_code: Spec031ExternalOwnerReasonCode,
    pub blocker_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec031ExternalOwnerProjection {
    pub schema: String,
    pub items: Vec<Spec031ExternalProjectionItem>,
    pub closure_blockers: Vec<Spec031ClosureBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec031ReadAuditArtifact {
    pub file_name: String,
    pub status: String,
    pub owner: ExternalOwnerSpec,
    pub reason_code: Spec031ExternalOwnerReasonCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spec031ExternalOwnerArtifactSet {
    pub read_audits: Vec<Spec031ReadAuditArtifact>,
    pub closure_blockers: Vec<Spec031ReadAuditArtifact>,
}

pub fn build_spec031_external_owner_projection(
    app_facts: impl IntoIterator<Item = ExternalOwnerFact>,
    media_facts: impl IntoIterator<Item = ExternalOwnerFact>,
) -> Spec031ExternalOwnerProjection {
    let mut items = Vec::new();
    items.extend(app_facts.into_iter().map(project_fact));
    items.extend(media_facts.into_iter().map(project_fact));
    push_missing_if_absent(
        &mut items,
        ExternalOwnerSpec::Spec032,
        Spec031ExternalCapability::App,
    );
    push_missing_if_absent(
        &mut items,
        ExternalOwnerSpec::Spec034,
        Spec031ExternalCapability::Media,
    );
    let closure_blockers = items.iter().filter_map(blocker_for_item).collect();

    Spec031ExternalOwnerProjection {
        schema: "spec031.external_owner.v1".to_owned(),
        items,
        closure_blockers,
    }
}

fn project_fact(fact: ExternalOwnerFact) -> Spec031ExternalProjectionItem {
    let (availability, reason_code, owner_status) = if fact.stale() {
        (
            Spec031Availability::Unavailable,
            Spec031ExternalOwnerReasonCode::StaleExternalOwnerEvidence,
            Some(fact.status()),
        )
    } else {
        (
            availability_for_status(fact.status()),
            fact.reason_code(),
            Some(fact.status()),
        )
    };
    Spec031ExternalProjectionItem {
        owner: fact.owner(),
        capability: fact.capability(),
        availability,
        reason_code,
        owner_status,
        opaque_ref: Some(fact.opaque_ref().as_str().to_owned()),
        receipt_ref: fact
            .receipt_ref()
            .map(|receipt| receipt.as_str().to_owned()),
    }
}

fn push_missing_if_absent(
    items: &mut Vec<Spec031ExternalProjectionItem>,
    owner: ExternalOwnerSpec,
    capability: Spec031ExternalCapability,
) {
    if items
        .iter()
        .any(|item| item.owner == owner && item.capability == capability)
    {
        return;
    }
    items.push(Spec031ExternalProjectionItem {
        owner,
        capability,
        availability: Spec031Availability::Unavailable,
        reason_code: MISSING_EXTERNAL_OWNER_EVIDENCE,
        owner_status: None,
        opaque_ref: None,
        receipt_ref: None,
    });
}

fn availability_for_status(status: ExternalOwnerStatus) -> Spec031Availability {
    match status {
        ExternalOwnerStatus::Ready | ExternalOwnerStatus::Included => Spec031Availability::Ready,
        ExternalOwnerStatus::Degraded | ExternalOwnerStatus::Skipped => {
            Spec031Availability::Degraded
        }
        ExternalOwnerStatus::Blocked
        | ExternalOwnerStatus::Unsupported
        | ExternalOwnerStatus::ExtractionFailed => Spec031Availability::Blocked,
        ExternalOwnerStatus::Unavailable => Spec031Availability::Unavailable,
    }
}

fn blocker_for_item(item: &Spec031ExternalProjectionItem) -> Option<Spec031ClosureBlocker> {
    let reason_code = match item.reason_code {
        Spec031ExternalOwnerReasonCode::MissingExternalOwnerEvidence
        | Spec031ExternalOwnerReasonCode::StaleExternalOwnerEvidence => item.reason_code,
        Spec031ExternalOwnerReasonCode::OwnerRecorded => return None,
    };
    Some(Spec031ClosureBlocker {
        owner: item.owner,
        capability: item.capability,
        reason_code,
        blocker_ref: blocker_file_name(item.owner, item.capability),
    })
}

pub(super) fn read_audit_file_name(owner: ExternalOwnerSpec) -> String {
    match owner {
        ExternalOwnerSpec::Spec032 => "spec032-read-audit.json".to_owned(),
        ExternalOwnerSpec::Spec034 => "spec034-read-audit.json".to_owned(),
    }
}

pub(super) fn blocker_file_name(
    owner: ExternalOwnerSpec,
    capability: Spec031ExternalCapability,
) -> String {
    match (owner, capability) {
        (ExternalOwnerSpec::Spec032, Spec031ExternalCapability::App) => {
            "spec032-app-closure-blocker.json".to_owned()
        }
        (ExternalOwnerSpec::Spec034, Spec031ExternalCapability::Media) => {
            "spec034-media-closure-blocker.json".to_owned()
        }
        (ExternalOwnerSpec::Spec032, Spec031ExternalCapability::Media) => {
            "spec032-media-closure-blocker.json".to_owned()
        }
        (ExternalOwnerSpec::Spec034, Spec031ExternalCapability::App) => {
            "spec034-app-closure-blocker.json".to_owned()
        }
    }
}
