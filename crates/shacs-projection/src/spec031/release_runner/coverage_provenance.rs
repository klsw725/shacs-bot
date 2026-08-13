use super::coverage::{Spec031ArtifactMediaType, Spec031TypedEvidenceClass};

pub(super) struct ArtifactProvenance {
    pub(super) name: &'static str,
    pub(super) artifact: &'static str,
    pub(super) source_locator: &'static str,
    pub(super) evidence_class: Spec031TypedEvidenceClass,
    pub(super) media_type: Spec031ArtifactMediaType,
}

pub(super) struct RequirementProvenance {
    pub(super) id: String,
    pub(super) source_locator: String,
    pub(super) artifact: &'static str,
    pub(super) command_id: &'static str,
}

pub(super) const REQUIRED_ARTIFACT_PROVENANCE: [ArtifactProvenance; 5] = [
    ArtifactProvenance {
        name: "manifest",
        artifact: "manifest.json",
        source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:59",
        evidence_class: Spec031TypedEvidenceClass::ManifestJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "coverage-matrix",
        artifact: "coverage-matrix.json",
        source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:59",
        evidence_class: Spec031TypedEvidenceClass::CoverageMatrixJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "results",
        artifact: "results.json",
        source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:62",
        evidence_class: Spec031TypedEvidenceClass::CommandResultsJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "failure-triage",
        artifact: "failure-triage.json",
        source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:62",
        evidence_class: Spec031TypedEvidenceClass::FailureTriageJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "summary",
        artifact: "summary.md",
        source_locator: "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md:62",
        evidence_class: Spec031TypedEvidenceClass::SummaryMarkdown,
        media_type: Spec031ArtifactMediaType::Markdown,
    },
];

pub(super) fn requirement_provenance() -> Vec<RequirementProvenance> {
    let mut rows = Vec::new();
    push_numbered(&mut rows, "must", 13, 63, requirement_command);
    push_numbered(&mut rows, "acceptance", 14, 102, requirement_command);
    push_numbered(&mut rows, "closure", 12, 227, closure_command);
    const PRD_ROWS: [(&str, &str, &str); 6] = [
        (
            "spec031:prd:01",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:198",
            "spec031-test-lifecycle",
        ),
        (
            "spec031:prd:02",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:199",
            "spec031-test-lifecycle",
        ),
        (
            "spec031:prd:03",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:200",
            "spec031-test-projection-parity",
        ),
        (
            "spec031:prd:04",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:201",
            "spec031-test-projection-parity",
        ),
        (
            "spec031:prd:05",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:202",
            "spec031-test-surface-smoke",
        ),
        (
            "spec031:prd:06",
            "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:203",
            "spec031-test-surface-smoke",
        ),
    ];
    for (id, source_locator, command_id) in PRD_ROWS {
        rows.push(RequirementProvenance {
            id: id.to_owned(),
            source_locator: source_locator.to_owned(),
            artifact: command_artifact(command_id),
            command_id,
        });
    }
    rows
}

fn push_numbered(
    rows: &mut Vec<RequirementProvenance>,
    prefix: &'static str,
    count: usize,
    first_line: usize,
    command_for: fn(usize) -> &'static str,
) {
    let base = "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md";
    for index in 1..=count {
        let command_id = command_for(index);
        rows.push(RequirementProvenance {
            id: format!("spec031:{prefix}:{index:02}"),
            source_locator: format!("{base}:{}", first_line + index - 1),
            artifact: command_artifact(command_id),
            command_id,
        });
    }
}

fn requirement_command(index: usize) -> &'static str {
    match index {
        1..=6 => "spec031-test-lifecycle",
        7..=11 => "spec031-test-projection-parity",
        12..=14 => "spec031-test-surface-smoke",
        _ => "spec031-test-release-runner",
    }
}

fn closure_command(index: usize) -> &'static str {
    match index {
        1..=4 => "spec031-test-lifecycle",
        5..=9 => "spec031-test-projection-parity",
        10..=12 => "spec031-test-surface-smoke",
        _ => "spec031-test-release-runner",
    }
}

fn command_artifact(command_id: &str) -> &'static str {
    match command_id {
        "spec031-test-lifecycle" => "commands/spec031-test-lifecycle.stdout",
        "spec031-test-projection-parity" => "commands/spec031-test-projection-parity.stdout",
        "spec031-test-surface-smoke" => "commands/spec031-test-surface-smoke.stdout",
        _ => "commands/spec031-test-release-runner.stdout",
    }
}
