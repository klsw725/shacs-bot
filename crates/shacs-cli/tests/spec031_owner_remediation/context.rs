use super::TestResult;
use shacs_cli::{format_context_refs_resolve_report, ContextRefsResolveCliReport};
use shacs_core::runtime::{
    build_context_diagnostics_summary, project_spec031_context_evidence, ContextDiagnosticsInput,
    ContextFileProjection, ContextFileReadStatus, ContextFileSource, ContextPermissionEvidence,
    ContextPermissionStatus, ContextRedactionStatus, ContextReferenceKind, ContextResolutionState,
    ContextTruncationStatus, ResolvedContextArtifact, Spec031ContextEvidenceInput,
    Spec031ContextOwnerRef,
};
use shacs_projection::Spec031Freshness;

#[test]
fn real_context_formatter_preserves_all_seven_mixed_canonical_rows() -> TestResult {
    let inline: Vec<_> = [
        (
            ContextReferenceKind::Unsupported,
            ContextResolutionState::Skipped,
        ),
        (ContextReferenceKind::Url, ContextResolutionState::Denied),
        (ContextReferenceKind::File, ContextResolutionState::Failed),
    ]
    .into_iter()
    .map(|(kind, state)| ResolvedContextArtifact {
        kind,
        state,
        source: "/HOST_PATH_SENTINEL/source".to_owned(),
        display_name: "raw".to_owned(),
        content: Some("RAW_CONTENT_SENTINEL".to_owned()),
        digest: None,
        byte_count: Some(0),
        token_estimate: Some(0),
        redaction_status: ContextRedactionStatus::NotApplied,
        truncation_status: ContextTruncationStatus::NotApplied,
        permission_evidence: ContextPermissionEvidence {
            status: ContextPermissionStatus::NotChecked,
            evidence: None,
        },
    })
    .collect();
    let files: Vec<_> = [
        ContextFileReadStatus::Included,
        ContextFileReadStatus::SkippedMissing,
        ContextFileReadStatus::DeniedBoundary,
        ContextFileReadStatus::ParseError,
    ]
    .into_iter()
    .enumerate()
    .map(|(order, status)| ContextFileProjection {
        order,
        status,
        path: "/HOST_PATH_SENTINEL/AGENTS.md".into(),
        filename: "AGENTS.md".to_owned(),
        source: ContextFileSource::DefaultCandidate,
        source_directory_depth: 0,
        reason: None,
        digest: None,
        content: None,
    })
    .collect();
    let evidence = project_spec031_context_evidence(Spec031ContextEvidenceInput {
        batch_ref: Some(Spec031ContextOwnerRef::try_new("subject:context:batch")?),
        owner_freshness: Spec031Freshness::Current,
        inline_artifacts: &inline,
        context_files: &files,
        provider_handoff: None,
    })?;
    let refs: Vec<_> = evidence
        .rows
        .iter()
        .map(|row| row.opaque_ref.as_str().to_owned())
        .collect();
    println!("owner={}", serde_json::to_string(&evidence)?);
    let summary = build_context_diagnostics_summary(ContextDiagnosticsInput {
        reference_parse: None,
        context_files: &files,
        resolved_artifacts: &inline,
        safety_report: None,
        provider_handoff: None,
    });

    let output = format_context_refs_resolve_report(ContextRefsResolveCliReport {
        workspace: "/HOST_PATH_SENTINEL/workspace".into(),
        summary,
        evidence,
    });

    println!("formatter={output}");
    let rows: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("Spec031 context:"))
        .collect();
    assert_eq!(rows.len(), 7);
    for ((row, reference), reason) in rows.iter().zip(refs).zip([
        "unsupported",
        "blocked",
        "extraction_failed",
        "included",
        "missing",
        "blocked",
        "extraction_failed",
    ]) {
        assert!(row.contains(&reference), "{row}");
        assert!(row.contains(&format!("reason={reason}")), "{row}");
        assert!(row.contains("parent=subject:context:batch"), "{row}");
        assert!(row.contains("freshness=current"), "{row}");
    }
    assert!(!output.contains("HOST_PATH_SENTINEL"));
    assert!(!output.contains("RAW_CONTENT_SENTINEL"));
    Ok(())
}
