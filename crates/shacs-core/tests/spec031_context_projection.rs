use shacs_core::runtime::{
    project_spec031_context_evidence, ContextBudgetDecision, ContextBudgetEvidence,
    ContextFileDigest, ContextFileProjection, ContextFileReadStatus, ContextFileSource,
    ContextPermissionEvidence, ContextPermissionStatus, ContextProviderHandoff,
    ContextRedactionStatus, ContextReferenceKind, ContextResolutionState, ContextTruncationStatus,
    ResolvedContextArtifact, Spec031ContextEvidenceInput, Spec031ContextEvidenceRowKind,
    Spec031ContextOwnerRef,
};
use shacs_projection::{Spec031Availability, Spec031Freshness, Spec031InclusionReason};
use std::path::PathBuf;

fn context_file(
    order: usize,
    path: &str,
    status: ContextFileReadStatus,
    digest: Option<ContextFileDigest>,
) -> ContextFileProjection {
    ContextFileProjection {
        order,
        path: PathBuf::from(path),
        filename: "AGENTS.md".to_owned(),
        source: ContextFileSource::DefaultCandidate,
        source_directory_depth: 0,
        status,
        reason: Some("safe owner summary".to_owned()),
        digest,
        content: None,
    }
}

fn inline_artifact(
    kind: ContextReferenceKind,
    state: ContextResolutionState,
    source: &str,
    digest: Option<&str>,
) -> ResolvedContextArtifact {
    ResolvedContextArtifact {
        kind,
        source: source.to_owned(),
        display_name: source.to_owned(),
        content: Some("safe resolver summary".to_owned()),
        digest: digest.map(str::to_owned),
        byte_count: Some(0),
        token_estimate: Some(0),
        redaction_status: ContextRedactionStatus::NotApplied,
        truncation_status: ContextTruncationStatus::NotApplied,
        permission_evidence: ContextPermissionEvidence {
            status: ContextPermissionStatus::Allowed,
            evidence: Some("safe resolver evidence".to_owned()),
        },
        state,
    }
}

fn handoff(
    decision: ContextBudgetDecision,
    estimated_tokens: Option<usize>,
) -> ContextProviderHandoff {
    ContextProviderHandoff {
        blocks: Vec::new(),
        evidence: vec![ContextBudgetEvidence {
            source_label: "inline:safe.md".to_owned(),
            priority: shacs_core::runtime::ContextArtifactPriority::ExplicitInline,
            decision,
            reason: Some("safe budget summary".to_owned()),
            digest: Some("sha256:budget".to_owned()),
            estimated_tokens,
            included_tokens: 0,
        }],
        used_context_tokens: 0,
        budget_tokens: 0,
        estimator: shacs_core::runtime::select_token_estimator("unknown", "unknown"),
        required: Vec::new(),
        required_overflow_tokens: 0,
    }
}

#[test]
fn spec031_context_projection_covers_canonical_reasons_without_raw_material() {
    let absolute_path = "/tmp/spec031-secret/AGENTS.md";
    let credential_url = "https://user:pass@example.invalid/secret";
    let files = vec![
        context_file(
            0,
            absolute_path,
            ContextFileReadStatus::Included,
            Some(ContextFileDigest {
                sha256: "sha256:included".to_owned(),
                byte_count: 7,
                token_estimate: 2,
            }),
        ),
        context_file(1, "missing.md", ContextFileReadStatus::SkippedMissing, None),
        context_file(2, "blocked.md", ContextFileReadStatus::DeniedBoundary, None),
        context_file(3, "failed.md", ContextFileReadStatus::ParseError, None),
    ];
    let inline = vec![
        inline_artifact(
            ContextReferenceKind::Unsupported,
            ContextResolutionState::Skipped,
            "scheme:value",
            None,
        ),
        inline_artifact(
            ContextReferenceKind::Url,
            ContextResolutionState::Denied,
            credential_url,
            None,
        ),
        inline_artifact(
            ContextReferenceKind::File,
            ContextResolutionState::Failed,
            "safe.md",
            Some("sha256:inlinefailed"),
        ),
    ];

    let projection = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: Spec031ContextOwnerRef::try_new("subject:context:batch").ok(),
        owner_freshness: Spec031Freshness::Current,
        inline_artifacts: &inline,
        context_files: &files,
        provider_handoff: None,
    })
    .expect("context projection should build");

    let reasons = projection
        .rows
        .iter()
        .map(|row| row.reason)
        .collect::<Vec<_>>();
    assert_eq!(
        reasons,
        vec![
            Spec031InclusionReason::Unsupported,
            Spec031InclusionReason::Blocked,
            Spec031InclusionReason::ExtractionFailed,
            Spec031InclusionReason::Included,
            Spec031InclusionReason::Missing,
            Spec031InclusionReason::Blocked,
            Spec031InclusionReason::ExtractionFailed,
        ]
    );
    assert!(projection.rows.iter().any(|row| {
        row.kind == Spec031ContextEvidenceRowKind::InlineReference
            && row.reason == Spec031InclusionReason::Unsupported
            && !row.included
    }));
    assert!(projection
        .rows
        .iter()
        .any(|row| row.reason == Spec031InclusionReason::Blocked));
    assert_eq!(projection.envelopes.len(), projection.rows.len());
    assert!(projection
        .envelopes
        .iter()
        .any(|envelope| envelope.state() == Spec031Availability::Blocked));

    let serialized = serde_json::to_string(&projection).expect("projection serializes");
    assert!(!serialized.contains(absolute_path));
    assert!(!serialized.contains("user:pass"));
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("safe resolver summary"));
}

#[test]
fn spec031_context_projection_preserves_mixed_order_and_budget_zero_vs_missing() {
    let inline = vec![inline_artifact(
        ContextReferenceKind::File,
        ContextResolutionState::Resolved,
        "safe.md",
        Some("sha256:inlineincluded"),
    )];
    let files = vec![context_file(
        9,
        "AGENTS.md",
        ContextFileReadStatus::Included,
        Some(ContextFileDigest {
            sha256: "sha256:fileincluded".to_owned(),
            byte_count: 0,
            token_estimate: 0,
        }),
    )];
    let zero_budget = handoff(ContextBudgetDecision::SkippedBudget, Some(0));

    let with_zero = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: Spec031ContextOwnerRef::try_new("subject:context:zero-budget").ok(),
        owner_freshness: Spec031Freshness::Stale,
        inline_artifacts: &inline,
        context_files: &files,
        provider_handoff: Some(&zero_budget),
    })
    .expect("context projection should build");
    let without_budget = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: Spec031ContextOwnerRef::try_new("subject:context:missing-budget").ok(),
        owner_freshness: Spec031Freshness::Current,
        inline_artifacts: &inline,
        context_files: &files,
        provider_handoff: None,
    })
    .expect("context projection should build");

    assert_eq!(with_zero.rows[0].order, 0);
    assert_eq!(with_zero.rows[1].order, 9);
    assert_eq!(with_zero.rows[0].budget_estimated_tokens, Some(0));
    assert_eq!(without_budget.rows[0].budget_estimated_tokens, None);
    assert_eq!(with_zero.rows[0].reason, Spec031InclusionReason::Skipped);
    assert_eq!(
        with_zero.envelopes[0].source().freshness,
        Spec031Freshness::Stale
    );
}
