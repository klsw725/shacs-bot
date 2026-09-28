use super::coverage::Spec031CoverageStatus;
use super::coverage_matrix::coverage_entries;
use super::model::Spec031ReleaseArtifactError;
use super::spec035_evidence::validate_spec035_closure_evidence;
use std::collections::HashSet;
use std::fs;

#[test]
fn spec035_catalog_covers_authoritative_closure_and_parent_rows_without_relabeling_spec031() {
    let root = tempfile::tempdir().expect("owned evidence root");
    fs::create_dir(root.path().join("triage")).expect("triage directory");
    fs::create_dir(root.path().join("external")).expect("audit directory");
    fs::write(
        root.path().join("triage/blocked-external-evidence.json"),
        "{}",
    )
    .expect("blocked receipt");
    for owner in ["029", "030", "032", "033", "034", "035"] {
        fs::write(
            root.path()
                .join(format!("external/spec{owner}-read-audit.md")),
            "blocked",
        )
        .expect("audit receipt");
    }

    let rows = coverage_entries(
        root.path(),
        root.path(),
        Spec031CoverageStatus::Blocked,
        &[],
        &[],
    )
    .expect("coverage generated");

    let spec035: Vec<_> = rows
        .iter()
        .filter(|row| row.requirement_id.starts_with("spec035:"))
        .collect();
    assert_eq!(spec035.len(), 80);
    let ids: HashSet<_> = spec035
        .iter()
        .map(|row| row.requirement_id.as_str())
        .collect();
    for (prd, count) in [4, 4, 4, 4, 5, 4, 4, 8, 4, 4].into_iter().enumerate() {
        let section = if prd == 7 { "FC" } else { "CE" };
        for item in 1..=count {
            assert!(ids.contains(format!("spec035:PRD{prd:03}-{section}-{item}").as_str()));
        }
    }
    for (section, count) in [("must", 13), ("acceptance", 12), ("closure", 10)] {
        for item in 1..=count {
            assert!(ids.contains(format!("spec035:{section}:{item:02}").as_str()));
        }
    }
    assert!(spec035
        .iter()
        .all(|row| row.status == Spec031CoverageStatus::Blocked));
    assert!(spec035
        .iter()
        .all(|row| row.owner.starts_with("spec035:prd:")));
    let original = rows
        .iter()
        .find(|row| row.requirement_id == "spec031:must:01")
        .expect("shipped row");
    assert_eq!(original.owner, "spec031");
    assert_eq!(
        original.source_locator,
        "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md:63"
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.requirement_id.starts_with("spec031:"))
            .count(),
        66
    );
}

#[test]
fn spec035_v2_canonical_classification_is_recognized_but_never_execution_approval() {
    let repo = super::spec035_classification_test::classification_fixture();

    let result = validate_spec035_closure_evidence(repo.path());

    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
}

#[test]
fn spec035_catalog_source_locations_match_numbered_authority_sections() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repository");
    let rows = super::spec035_catalog::catalog_at(repo).expect("current authority");
    for row in &rows {
        let (file, line) = row.source_locator.rsplit_once(':').expect("source locator");
        let line: usize = line.parse().expect("line number");
        let text = fs::read_to_string(repo.join(file)).expect("authority reads");
        let number = row
            .id
            .rsplit([':', '-'])
            .next()
            .expect("number")
            .parse::<usize>()
            .expect("numbered row");
        assert!(
            text.lines()
                .nth(line - 1)
                .expect("source line")
                .starts_with(&format!("{number}. ")),
            "{}",
            row.id
        );
        let header = text
            .lines()
            .take(line)
            .filter(|line| line.starts_with("## "))
            .last()
            .expect("section");
        let section = if row.id.contains(":must:") {
            "## Must Have"
        } else if row.id.contains(":acceptance:") {
            "## Acceptance Criteria"
        } else if row.id.contains("-FC-") {
            "## Final Closure Condition"
        } else {
            "## Closure Evidence"
        };
        assert_eq!(header, section, "{}", row.id);
    }
    for (prd, count) in [4, 4, 4, 4, 5, 4, 4, 8, 4, 4].into_iter().enumerate() {
        let authority = &super::spec035_catalog::PRDS[prd];
        let text = fs::read_to_string(repo.join(format!(
            "{}/prds/{}",
            super::spec035_catalog::SPEC_ROOT,
            authority.file
        )))
        .expect("PRD reads");
        let count_in_section = text
            .lines()
            .skip_while(|line| *line != format!("## {}", authority.section))
            .skip(1)
            .take_while(|line| !line.starts_with("## "))
            .filter(|line| {
                line.split_once(". ")
                    .is_some_and(|(number, _)| number.parse::<usize>().is_ok())
            })
            .count();
        assert_eq!(count_in_section, count, "PRD{prd:03}");
    }
}

#[test]
#[ignore = "requires retained canonical classification and an explicit new evidence root"]
fn spec035_catalog_retained_current_worktree_audit() {
    use super::model::{
        Spec031ReleaseRunArtifacts, Spec031ReleaseRunId, Spec031ReleaseRunnerConfig,
        Spec031ReleaseRunnerMode,
    };
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("repository");
    assert_eq!(
        validate_spec035_closure_evidence(repo),
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("SHACS_SPEC035_RUNNER_QA_ROOT").expect("explicit retained evidence root"),
    );
    assert!(output.is_absolute());
    let result = super::runner::run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("spec035-f1-catalog-current").expect("run id"),
        evidence_root: output.clone(),
        repo_root: repo.to_path_buf(),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: std::time::Duration::from_secs(1),
    });
    assert_eq!(
        result,
        Err(Spec031ReleaseArtifactError::BlockedExternalEvidence)
    );
    let artifacts: Spec031ReleaseRunArtifacts = serde_json::from_slice(
        &fs::read(output.join("manifest.json")).expect("real manifest retained"),
    )
    .expect("typed runner manifest");
    assert_eq!(artifacts.coverage_matrix.len(), 146);
    assert!(artifacts.command_registry.is_empty());
    assert_eq!(
        artifacts
            .coverage_matrix
            .iter()
            .filter(|row| row.requirement_id.starts_with("spec035:")
                && row.status == Spec031CoverageStatus::Blocked)
            .count(),
        80
    );
    for file in [
        "coverage-matrix.json",
        "results.json",
        "failure-triage.json",
        "summary.md",
        "evidence-index.json",
    ] {
        assert!(output.join(file).is_file(), "{file}");
    }
    println!("retained current-worktree library QA: {}", output.display());
}
