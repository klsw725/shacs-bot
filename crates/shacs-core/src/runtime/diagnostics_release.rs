use serde::{Deserialize, Serialize};
use shacs_utils::evaluator::{
    spec018_evidence_ref_has_owner_and_redaction, spec018_release_gate_outcome, EvidenceRef,
    RedactionStatus, Spec018DiagnosticsEvidenceManifest, Spec018DiagnosticsRedactionSummary,
    Spec018LedgerInspectQuery, Spec018LedgerInspectResult, Spec018ReleaseBlocker,
    Spec018ReleaseBlockerCategory, Spec018ReleaseBlockerSeverity, Spec018ReleaseCoverageEntry,
    Spec018ReleaseGateOutcome, Spec018SkippedEvidence,
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSearchReleaseEvidenceBucket {
    Config,
    Assembler,
    Bridge,
    RunnerWiring,
    McpDefaultDeny,
    SubagentScope,
    ReplaySafety,
    Diagnostics,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSearchReleaseEvidence {
    pub bucket: ToolSearchReleaseEvidenceBucket,
    pub test_names: Vec<String>,
    pub manual_qa_refs: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<EvidenceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSearchReleaseEvidenceChecklist {
    pub required_buckets: Vec<ToolSearchReleaseEvidenceBucket>,
    pub covered_buckets: Vec<ToolSearchReleaseEvidenceBucket>,
    pub missing_buckets: Vec<ToolSearchReleaseEvidenceBucket>,
    pub passed: bool,
}

impl ToolSearchReleaseEvidenceBucket {
    pub fn required_prd005_buckets() -> Vec<Self> {
        vec![
            Self::Config,
            Self::Assembler,
            Self::Bridge,
            Self::RunnerWiring,
            Self::McpDefaultDeny,
            Self::SubagentScope,
            Self::ReplaySafety,
            Self::Diagnostics,
        ]
    }
}

pub fn tool_search_prd005_release_evidence_checklist(
    evidence: &[ToolSearchReleaseEvidence],
) -> ToolSearchReleaseEvidenceChecklist {
    let required_buckets = ToolSearchReleaseEvidenceBucket::required_prd005_buckets();
    let covered = evidence
        .iter()
        .filter(|entry| {
            (!entry.test_names.is_empty() || !entry.manual_qa_refs.is_empty())
                && entry
                    .evidence_refs
                    .iter()
                    .any(spec018_evidence_ref_has_owner_and_redaction)
        })
        .map(|entry| entry.bucket)
        .collect::<BTreeSet<_>>();
    let covered_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| covered.contains(bucket))
        .collect::<Vec<_>>();
    let missing_buckets = required_buckets
        .iter()
        .copied()
        .filter(|bucket| !covered.contains(bucket))
        .collect::<Vec<_>>();
    let passed = missing_buckets.is_empty();

    ToolSearchReleaseEvidenceChecklist {
        required_buckets,
        covered_buckets,
        missing_buckets,
        passed,
    }
}

pub struct RuntimeSpec018DiagnosticsManifestInput<'a> {
    pub manifest_id: &'a str,
    pub generated_at_ms: u64,
    pub redaction_profile: &'a str,
    pub evaluator_refs: &'a [EvidenceRef],
    pub ledger_refs: &'a [EvidenceRef],
    pub automation_refs: &'a [EvidenceRef],
    pub memory_refs: &'a [EvidenceRef],
    pub improvement_refs: &'a [EvidenceRef],
    pub replay_refs: &'a [EvidenceRef],
    pub projection_refs: &'a [EvidenceRef],
    pub skipped_evidence: &'a [Spec018SkippedEvidence],
    pub diagnostics_artifact_refs: &'a [EvidenceRef],
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeSpec018LedgerInspectInput<'a> {
    pub query: &'a Spec018LedgerInspectQuery,
    pub source_refs: &'a [EvidenceRef],
    pub consumption_record_refs: &'a [EvidenceRef],
    pub runtime_decision_refs: &'a [EvidenceRef],
    pub projection_item_refs: &'a [EvidenceRef],
    pub diagnostics_artifact_refs: &'a [EvidenceRef],
    pub skipped_evidence: &'a [Spec018SkippedEvidence],
}

#[derive(Debug, Clone, Copy)]
pub struct RuntimeSpec018ReleaseGateInput<'a> {
    pub coverage_entries: &'a [Spec018ReleaseCoverageEntry],
    pub blockers: &'a [Spec018ReleaseBlocker],
    pub diagnostics_manifest_ref: Option<&'a EvidenceRef>,
    pub ledger_inspect_ref: Option<&'a EvidenceRef>,
}

pub fn build_spec018_diagnostics_manifest(
    input: RuntimeSpec018DiagnosticsManifestInput<'_>,
) -> Spec018DiagnosticsEvidenceManifest {
    let redaction_summary = spec018_redaction_summary(
        input.redaction_profile,
        &[
            input.evaluator_refs,
            input.ledger_refs,
            input.automation_refs,
            input.memory_refs,
            input.improvement_refs,
            input.replay_refs,
            input.projection_refs,
            input.diagnostics_artifact_refs,
        ],
        input.skipped_evidence,
    );

    Spec018DiagnosticsEvidenceManifest {
        manifest_id: input.manifest_id.to_owned(),
        generated_at_ms: input.generated_at_ms,
        evaluator_refs: input.evaluator_refs.to_vec(),
        ledger_refs: input.ledger_refs.to_vec(),
        automation_refs: input.automation_refs.to_vec(),
        memory_refs: input.memory_refs.to_vec(),
        improvement_refs: input.improvement_refs.to_vec(),
        replay_refs: input.replay_refs.to_vec(),
        projection_refs: input.projection_refs.to_vec(),
        skipped_evidence: input.skipped_evidence.to_vec(),
        diagnostics_artifact_refs: input.diagnostics_artifact_refs.to_vec(),
        redaction_summary,
    }
}

pub fn build_spec018_ledger_inspect_result(
    input: RuntimeSpec018LedgerInspectInput<'_>,
) -> Spec018LedgerInspectResult {
    let skipped_evidence = if input.query.include_skipped {
        input.skipped_evidence.to_vec()
    } else {
        Vec::new()
    };
    let diagnostics_artifact_refs = if input.query.include_diagnostics_refs {
        input.diagnostics_artifact_refs.to_vec()
    } else {
        Vec::new()
    };

    Spec018LedgerInspectResult {
        query: input.query.clone(),
        source_refs: input.source_refs.to_vec(),
        consumption_record_refs: input.consumption_record_refs.to_vec(),
        runtime_decision_refs: input.runtime_decision_refs.to_vec(),
        projection_item_refs: input.projection_item_refs.to_vec(),
        diagnostics_artifact_refs,
        skipped_evidence,
    }
}

pub fn evaluate_spec018_release_gate(
    input: RuntimeSpec018ReleaseGateInput<'_>,
) -> Spec018ReleaseGateOutcome {
    let mut blockers = input.blockers.to_vec();
    if input.diagnostics_manifest_ref.is_none() || input.ledger_inspect_ref.is_none() {
        blockers.push(Spec018ReleaseBlocker {
            blocker_id: "missing-diagnostics-integration-evidence".to_owned(),
            category: Spec018ReleaseBlockerCategory::MissingLedgerConsumptionEvidence,
            source_ref: input
                .diagnostics_manifest_ref
                .or(input.ledger_inspect_ref)
                .filter(|evidence_ref| spec018_evidence_ref_has_owner_and_redaction(evidence_ref))
                .cloned()
                .unwrap_or_else(|| release_gate_synthetic_ref("missing-diagnostics-integration")),
            severity: Spec018ReleaseBlockerSeverity::Blocking,
            redacted_summary: "release gate lacks diagnostics manifest or ledger inspect evidence"
                .to_owned(),
            resolution_hint:
                "attach diagnostics manifest and ledger inspect refs before release closure"
                    .to_owned(),
        });
    }

    for (label, evidence_ref) in [
        ("diagnostics-manifest", input.diagnostics_manifest_ref),
        ("ledger-inspect", input.ledger_inspect_ref),
    ] {
        if let Some(evidence_ref) = evidence_ref {
            if !spec018_evidence_ref_has_owner_and_redaction(evidence_ref) {
                blockers.push(Spec018ReleaseBlocker {
                    blocker_id: format!("invalid-{label}-evidence"),
                    category: Spec018ReleaseBlockerCategory::MissingLedgerConsumptionEvidence,
                    source_ref: release_gate_synthetic_ref(&format!("invalid-{label}-evidence")),
                    severity: Spec018ReleaseBlockerSeverity::Blocking,
                    redacted_summary: format!(
                        "release gate {label} evidence lacks owner or redaction validity"
                    ),
                    resolution_hint:
                        "attach an owner-scoped, redacted diagnostics or ledger evidence ref"
                            .to_owned(),
                });
            }
        }
    }

    spec018_release_gate_outcome(input.coverage_entries.to_vec(), blockers)
}

fn release_gate_synthetic_ref(id: &str) -> EvidenceRef {
    EvidenceRef {
        kind: shacs_utils::evaluator::EvidenceKind::DiagnosticRecord,
        id: id.to_owned(),
        digest: id.to_owned(),
        summary: "release gate synthetic diagnostics evidence".to_owned(),
        redaction_status: RedactionStatus::AlreadySafe,
        owner_spec: Some("018".to_owned()),
        locator: Some(format!("diagnostics://{id}")),
        retention_hint: Some("release_gate".to_owned()),
    }
}

fn spec018_redaction_summary(
    redaction_profile: &str,
    evidence_groups: &[&[EvidenceRef]],
    skipped_evidence: &[Spec018SkippedEvidence],
) -> Spec018DiagnosticsRedactionSummary {
    let mut redacted_ref_count = 0;
    let mut already_safe_ref_count = 0;
    let mut failed_ref_count = 0;

    for evidence_ref in evidence_groups
        .iter()
        .flat_map(|group| group.iter())
        .chain(skipped_evidence.iter().map(|skipped| &skipped.source_ref))
    {
        match evidence_ref.redaction_status {
            RedactionStatus::Redacted => redacted_ref_count += 1,
            RedactionStatus::AlreadySafe => already_safe_ref_count += 1,
            RedactionStatus::RedactionFailed => failed_ref_count += 1,
        }
    }

    Spec018DiagnosticsRedactionSummary {
        redaction_profile: redaction_profile.to_owned(),
        redacted_ref_count,
        already_safe_ref_count,
        failed_ref_count,
        skipped_ref_count: skipped_evidence.len(),
    }
}
