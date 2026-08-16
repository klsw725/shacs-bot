use super::video_analyzer_disclosure::{
    project_analyzer_disclosure, VideoAnalyzerDisclosureProjection,
};
use super::ExecutionSnapshot;
use serde::{Deserialize, Serialize};
use shacs_projection::{
    CredentialStatusProjection, DataDisclosureProjection, ResourceActivation,
    ResourceCandidateProjection, ResourceSource, ResourceTrust, SandboxStatusProjection,
    Spec031ExternalOwnerRef, Spec031Freshness, TrustedCodeDisclosure,
    TrustedRuntimeProfileProjection,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerOwnerFactsProjection {
    #[serde(skip)]
    _proof: VideoAnalyzerOwnerFactProof,
    pub freshness: Spec031Freshness,
    pub unavailable_reasons: Vec<VideoAnalyzerOwnerUnavailableReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<VideoAnalyzerSourceProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxStatusProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<CredentialStatusProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disclosure: Option<VideoAnalyzerDisclosureProjection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<VideoAnalyzerSnapshotProjection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoAnalyzerOwnerFactProof;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerSourceProjection {
    pub analyzer_ref: Spec031ExternalOwnerRef,
    pub source: ResourceSource,
    pub activation: ResourceActivation,
    pub trust: ResourceTrust,
    pub trusted_code_disclosure: TrustedCodeDisclosure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct VideoAnalyzerSnapshotProjection {
    pub snapshot_id: String,
    pub provenance_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoAnalyzerOwnerUnavailableReason {
    MissingAnalyzerOwnerRef,
    MissingSpec030OwnerFacts,
    MissingExecutionSnapshot,
    StaleOwnerFacts,
    OwnerFactsUnavailable,
    OwnerFreshnessUnknown,
    AnalyzerResourceMismatch,
    SnapshotResourceMissing,
    SnapshotProvenanceInvalid,
    SnapshotRefMalformed,
}

#[derive(Clone, Copy)]
pub struct VideoAnalyzerOwnerFactsInput<'a> {
    pub analyzer_ref: Option<&'a Spec031ExternalOwnerRef>,
    pub analyzer_resource: Option<&'a ResourceCandidateProjection>,
    pub profile: Option<&'a TrustedRuntimeProfileProjection>,
    pub sandbox: Option<&'a SandboxStatusProjection>,
    pub credential: Option<&'a CredentialStatusProjection>,
    pub disclosure: Option<&'a DataDisclosureProjection>,
    pub snapshot: Option<&'a ExecutionSnapshot>,
    pub freshness: Spec031Freshness,
}

impl VideoAnalyzerOwnerFactsInput<'_> {
    pub const fn unavailable(freshness: Spec031Freshness) -> Self {
        Self {
            analyzer_ref: None,
            analyzer_resource: None,
            profile: None,
            sandbox: None,
            credential: None,
            disclosure: None,
            snapshot: None,
            freshness,
        }
    }
}

pub(super) fn project_owner_facts(
    input: VideoAnalyzerOwnerFactsInput<'_>,
) -> VideoAnalyzerOwnerFactsProjection {
    let freshness_reason = match input.freshness {
        Spec031Freshness::Current => None,
        Spec031Freshness::Stale => Some(VideoAnalyzerOwnerUnavailableReason::StaleOwnerFacts),
        Spec031Freshness::Unavailable => {
            Some(VideoAnalyzerOwnerUnavailableReason::OwnerFactsUnavailable)
        }
        Spec031Freshness::Unknown => {
            Some(VideoAnalyzerOwnerUnavailableReason::OwnerFreshnessUnknown)
        }
    };
    if let Some(reason) = freshness_reason {
        return unavailable(input.freshness, vec![reason]);
    }

    let mut reasons = Vec::new();
    let analyzer_ref = input.analyzer_ref.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingAnalyzerOwnerRef);
        None
    });
    let resource = input.analyzer_resource.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts);
        None
    });
    let profile = input.profile.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts);
        None
    });
    let sandbox = input.sandbox.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts);
        None
    });
    let credential = input.credential.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts);
        None
    });
    let disclosure = input.disclosure.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts);
        None
    });
    let snapshot = input.snapshot.or_else(|| {
        reasons.push(VideoAnalyzerOwnerUnavailableReason::MissingExecutionSnapshot);
        None
    });
    reasons.sort_by_key(|reason| *reason as u8);
    reasons.dedup();
    if !reasons.is_empty() {
        return unavailable(Spec031Freshness::Unavailable, reasons);
    }

    let (Some(analyzer_ref), Some(resource), Some(profile), Some(sandbox)) =
        (analyzer_ref, resource, profile, sandbox)
    else {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::MissingSpec030OwnerFacts],
        );
    };
    let (Some(credential), Some(disclosure), Some(snapshot)) = (credential, disclosure, snapshot)
    else {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::MissingExecutionSnapshot],
        );
    };
    if resource.resource_ref != analyzer_ref.as_str() {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::AnalyzerResourceMismatch],
        );
    }
    if snapshot.validate_provenance().is_err() {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::SnapshotProvenanceInvalid],
        );
    }
    if !snapshot
        .selected_resources
        .iter()
        .any(|selected| selected.identity == analyzer_ref.as_str())
    {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::SnapshotResourceMissing],
        );
    }
    if !safe_snapshot_ref(&snapshot.snapshot_id, &snapshot.provenance_digest) {
        return unavailable(
            Spec031Freshness::Unavailable,
            vec![VideoAnalyzerOwnerUnavailableReason::SnapshotRefMalformed],
        );
    }

    VideoAnalyzerOwnerFactsProjection {
        _proof: VideoAnalyzerOwnerFactProof,
        freshness: Spec031Freshness::Current,
        unavailable_reasons: Vec::new(),
        source: Some(VideoAnalyzerSourceProjection {
            analyzer_ref: analyzer_ref.clone(),
            source: resource.source,
            activation: resource.activation,
            trust: profile.resource_trust,
            trusted_code_disclosure: resource.trusted_code_disclosure,
        }),
        sandbox: Some(sandbox.clone()),
        credential: Some(credential.clone()),
        disclosure: Some(project_analyzer_disclosure(disclosure)),
        snapshot: Some(VideoAnalyzerSnapshotProjection {
            snapshot_id: snapshot.snapshot_id.clone(),
            provenance_digest: snapshot.provenance_digest.clone(),
        }),
    }
}

fn unavailable(
    freshness: Spec031Freshness,
    unavailable_reasons: Vec<VideoAnalyzerOwnerUnavailableReason>,
) -> VideoAnalyzerOwnerFactsProjection {
    VideoAnalyzerOwnerFactsProjection {
        _proof: VideoAnalyzerOwnerFactProof,
        freshness,
        unavailable_reasons,
        source: None,
        sandbox: None,
        credential: None,
        disclosure: None,
        snapshot: None,
    }
}

fn safe_snapshot_ref(snapshot_id: &str, provenance_digest: &str) -> bool {
    !snapshot_id.is_empty()
        && snapshot_id.len() <= 160
        && snapshot_id.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '-' | '_')
        })
        && provenance_digest
            .strip_prefix("sha256:")
            .is_some_and(|digest| {
                digest.len() == 64
                    && digest
                        .chars()
                        .all(|character| character.is_ascii_hexdigit())
            })
}
