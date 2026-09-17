use super::coverage::Spec031CoverageRequirementKind;
use serde::{Deserialize, Serialize};

pub(super) const SPEC_ROOT: &str =
    "docs/specs/035-ui-projection-diagnostics-and-release-evidence-parity";

pub(super) struct PrdAuthority {
    pub(super) file: &'static str,
    pub(super) section: &'static str,
    pub(super) tag: &'static str,
    pub(super) first_line: usize,
    pub(super) count: usize,
}

pub(super) const PRDS: [PrdAuthority; 10] = [
    PrdAuthority {
        file: "000-shared-projection-model-and-vocabulary.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 91,
        count: 4,
    },
    PrdAuthority {
        file: "001-surface-adapter-parity-cli-api-channel.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 87,
        count: 4,
    },
    PrdAuthority {
        file: "002-approval-progress-and-recovery-parity.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 93,
        count: 4,
    },
    PrdAuthority {
        file: "003-readiness-degraded-health-and-diagnostics.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 98,
        count: 4,
    },
    PrdAuthority {
        file: "004-context-extension-app-and-media-projection.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 90,
        count: 5,
    },
    PrdAuthority {
        file: "005-interactive-tui-repl-and-onboard-flows.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 99,
        count: 4,
    },
    PrdAuthority {
        file: "006-reconnect-backpressure-and-drop-accounting.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 92,
        count: 4,
    },
    PrdAuthority {
        file: "007-release-runner-and-spec035-closure.md",
        section: "Final Closure Condition",
        tag: "FC",
        first_line: 278,
        count: 8,
    },
    PrdAuthority {
        file: "008-transport-capability-and-snapshot-first-reconnect.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 43,
        count: 4,
    },
    PrdAuthority {
        file: "009-goal-and-read-only-tasks-projection.md",
        section: "Closure Evidence",
        tag: "CE",
        first_line: 40,
        count: 4,
    },
];

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Requirement {
    pub(super) id: String,
    pub(super) kind: Spec031CoverageRequirementKind,
    pub(super) source_locator: String,
    pub(super) owner: String,
    pub(super) closure_ids: Vec<String>,
}

pub(super) fn catalog() -> Vec<Requirement> {
    let mut rows = Vec::with_capacity(80);
    for (prefix, kind, first_line, owners) in [
        (
            "must",
            Spec031CoverageRequirementKind::ParentMustHave,
            83,
            &[0, 1, 5, 5, 5, 2, 4, 3, 6, 7, 8, 8, 9][..],
        ),
        (
            "acceptance",
            Spec031CoverageRequirementKind::AcceptanceCriterion,
            113,
            &[0, 2, 6, 2, 4, 3, 5, 7, 7, 8, 8, 9][..],
        ),
        (
            "closure",
            Spec031CoverageRequirementKind::ClosureEvidence,
            191,
            &[0, 1, 5, 5, 1, 6, 7, 7, 8, 9][..],
        ),
    ] {
        for (offset, &prd) in owners.iter().enumerate() {
            let mut closure_ids = prd_closure_ids(prd);
            if prefix == "acceptance" && offset == 0 {
                closure_ids.extend(prd_closure_ids(1));
            }
            rows.push(Requirement {
                id: format!("spec035:{prefix}:{:02}", offset + 1),
                kind,
                source_locator: format!("{SPEC_ROOT}/SPEC.md:{}", first_line + offset),
                owner: format!("spec035:prd:{prd:03}"),
                closure_ids,
            });
        }
    }
    for (prd, authority) in PRDS.iter().enumerate() {
        for (offset, id) in prd_closure_ids(prd).into_iter().enumerate() {
            rows.push(Requirement {
                id: format!("spec035:{id}"),
                kind: Spec031CoverageRequirementKind::PrdTask,
                source_locator: format!(
                    "{SPEC_ROOT}/prds/{}:{}",
                    authority.file,
                    authority.first_line + offset
                ),
                owner: format!("spec035:prd:{prd:03}"),
                closure_ids: vec![id],
            });
        }
    }
    rows
}

pub(super) fn prd_closure_ids(prd: usize) -> Vec<String> {
    let authority = &PRDS[prd];
    (1..=authority.count)
        .map(|item| format!("PRD{prd:03}-{}-{item}", authority.tag))
        .collect()
}
