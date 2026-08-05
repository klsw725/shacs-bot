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
}

pub(super) const REQUIRED_ARTIFACT_PROVENANCE: [ArtifactProvenance; 5] = [
    ArtifactProvenance {
        name: "manifest",
        artifact: "manifest.json",
        source_locator: "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/prds/007-release-runner-and-spec031-closure.md:183",
        evidence_class: Spec031TypedEvidenceClass::ManifestJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "coverage-matrix",
        artifact: "coverage-matrix.json",
        source_locator: "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/prds/007-release-runner-and-spec031-closure.md:183",
        evidence_class: Spec031TypedEvidenceClass::CoverageMatrixJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "results",
        artifact: "results.json",
        source_locator: "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/prds/007-release-runner-and-spec031-closure.md:183",
        evidence_class: Spec031TypedEvidenceClass::CommandResultsJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "failure-triage",
        artifact: "failure-triage.json",
        source_locator: "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/prds/007-release-runner-and-spec031-closure.md:183",
        evidence_class: Spec031TypedEvidenceClass::FailureTriageJson,
        media_type: Spec031ArtifactMediaType::Json,
    },
    ArtifactProvenance {
        name: "summary",
        artifact: "summary.md",
        source_locator: "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/prds/007-release-runner-and-spec031-closure.md:184",
        evidence_class: Spec031TypedEvidenceClass::SummaryMarkdown,
        media_type: Spec031ArtifactMediaType::Markdown,
    },
];

pub(super) fn requirement_provenance() -> Vec<RequirementProvenance> {
    let mut rows = Vec::new();
    push_numbered(&mut rows, "must", 10, 74, "results.json");
    push_numbered(&mut rows, "acceptance", 9, 99, "results.json");
    push_numbered(&mut rows, "closure", 8, 165, "results.json");
    for index in 1..=8 {
        rows.push(RequirementProvenance {
            id: match index {
                1 => "spec031:prd:01".to_owned(),
                2 => "spec031:prd:02".to_owned(),
                3 => "spec031:prd:03".to_owned(),
                4 => "spec031:prd:04".to_owned(),
                5 => "spec031:prd:05".to_owned(),
                6 => "spec031:prd:06".to_owned(),
                7 => "spec031:prd:07".to_owned(),
                8 => "spec031:prd:08".to_owned(),
                _ => unreachable!(),
            },
            source_locator: match index {
                1 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:144".to_owned(),
                2 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:145".to_owned(),
                3 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:146".to_owned(),
                4 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:147".to_owned(),
                5 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:148".to_owned(),
                6 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:149".to_owned(),
                7 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:150".to_owned(),
                8 => "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md:151".to_owned(),
                _ => unreachable!(),
            },
            artifact: "results.json",
        });
    }
    rows
}

fn push_numbered(
    rows: &mut Vec<RequirementProvenance>,
    prefix: &'static str,
    count: usize,
    first_line: usize,
    artifact: &'static str,
) {
    let base = "docs/specs/031-ui-projection-diagnostics-and-release-evidence-parity/SPEC.md";
    for index in 1..=count {
        rows.push(RequirementProvenance {
            id: format!("spec031:{prefix}:{index:02}"),
            source_locator: format!("{base}:{}", first_line + index - 1),
            artifact,
        });
    }
}
