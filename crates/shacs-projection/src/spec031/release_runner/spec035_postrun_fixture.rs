use super::model::*;
use super::spec035_execution_fixture::{file_ref, write_json, Fixture};
use crate::release_evidence::EvidenceWriter;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn copy_runner_sources(repo: &Path, actual: &Path) {
    let mut paths: Vec<String> = super::coverage_provenance::requirement_provenance()
        .into_iter()
        .map(|row| {
            row.source_locator
                .rsplit_once(':')
                .expect("locator")
                .0
                .to_owned()
        })
        .collect();
    paths.extend(
        super::coverage_provenance::REQUIRED_ARTIFACT_PROVENANCE
            .iter()
            .map(|row| {
                row.source_locator
                    .rsplit_once(':')
                    .expect("locator")
                    .0
                    .to_owned()
            }),
    );
    for owner in super::external_audit_facts::external_owner_facts() {
        paths.push(owner.source_locator.to_owned());
        if owner.slug != "spec035" {
            paths.extend(
                owner
                    .fact_artifacts
                    .iter()
                    .map(|path| path.split('#').next().expect("path").to_owned()),
            );
        }
    }
    for path in paths {
        let target = repo.join(&path);
        std::fs::create_dir_all(target.parent().expect("parent")).expect("source directory");
        std::fs::copy(actual.join(path), target).expect("additional source copies");
    }
}

pub(super) fn runner_fixture() -> (Fixture, Spec031ReleaseRunArtifacts) {
    let fixture = super::spec035_preflight_test::pending_fixture();
    let root = fixture
        .repo
        .path()
        .canonicalize()
        .expect("canonical")
        .join(".omo/runner");
    let writer = EvidenceWriter::open_new_run(&root).expect("new output");
    for directory in ["commands", "fixtures", "cleanup", "postrun"] {
        writer.create_dir_all(directory).expect("directory");
    }
    let preflight = super::spec035_execution::preflight(fixture.repo.path()).expect("preflight");
    super::writer::write_json(
        &writer,
        super::spec035_execution::BINDING,
        &preflight.binding,
    )
    .expect("binding");
    let config = Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("constructed-execution").expect("id"),
        evidence_root: root.clone(),
        repo_root: fixture.repo.path().canonicalize().expect("repo"),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: std::time::Duration::ZERO,
    };
    let commands = super::current_commands::required_worktree_commands(&config).into_iter().map(|spec| {
        let stdout_path = format!("commands/{}.stdout", spec.id);
        let stderr_path = format!("commands/{}.stderr", spec.id);
        let tests = if spec.argv.get(1).is_some_and(|arg| arg == "test") { Some(Spec031ReleaseTestCounts { tests_run: 1, tests_failed: 0 }) } else { None };
        let name = spec.argv.iter().position(|arg| arg == "--").and_then(|offset| offset.checked_sub(1)).and_then(|offset| spec.argv.get(offset)).map(String::as_str).unwrap_or("constructed_test");
        let stdout = if tests.is_some() { format!("test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n") } else { "constructed test fixture, not actual release evidence\n".to_owned() };
        let stderr = spec.argv.windows(2).filter(|pair| pair[0] == "--test").map(|pair| format!("Running tests/{}.rs\n", pair[1])).collect::<String>();
        super::writer::write_text(&writer, &stdout_path, &stdout).expect("synthetic stdout");
        super::writer::write_text(&writer, &stderr_path, &stderr).expect("synthetic stderr");
        Spec031ReleaseCommandRecord { id: spec.id, gate: spec.gate, package: spec.package, filter: spec.filter,
            argv: spec.argv, cwd: spec.cwd.display().to_string(), status: Spec031ReleaseCommandStatus::Passed,
            exit_code: Some(0), duration_ms: 1, stdout_path, stderr_path, tests, process_receipt: None }
    }).collect();
    let mut artifacts = Spec031ReleaseRunArtifacts {
        schema: SPEC031_RELEASE_RUNNER_SCHEMA.to_owned(),
        run_id: config.run_id.clone(),
        evidence_root: root.display().to_string(),
        fixture_registry: vec!["fixtures/current-worktree.json".to_owned()],
        command_registry: commands,
        cleanup_registry: vec![],
        manifest_files: super::REQUIRED_ARTIFACTS
            .iter()
            .map(|path| (*path).to_owned())
            .chain([super::spec035_execution::BINDING.to_owned()])
            .collect(),
        coverage_matrix: vec![],
        external_audits: vec![],
        failure_triage: vec![],
        reproducibility_observations: vec![],
    };
    super::writer::write_json(&writer, "fixtures/current-worktree.json", &json!({"schema":SPEC031_RELEASE_RUNNER_SCHEMA,"run_id":config.run_id.as_str(),"resource_id":"current-worktree"})).expect("fixture");
    super::audit::add_external_audits(&config, &writer, &mut artifacts, false, None)
        .expect("audits");
    super::runner_outputs::push_cleanup(
        &config,
        &writer,
        &mut artifacts,
        super::runner_outputs::CleanupReceiptSpec {
            file_name: "current-worktree-receipt.json",
            status: "verified",
            resource_id: "current-worktree",
            check_artifact: "commands/spec031-test-surface-smoke.stdout",
        },
    )
    .expect("cleanup");
    super::runner_outputs::write_evidence_index(&config, &writer, &mut artifacts).expect("index");
    artifacts.coverage_matrix = super::coverage_matrix::coverage_entries(
        &root,
        &config.repo_root,
        super::coverage::Spec031CoverageStatus::Blocked,
        &artifacts.command_registry,
        &artifacts.external_audits,
    )
    .expect("coverage");
    super::writer::write_spec031_release_artifacts_with(&writer, &artifacts).expect("originals");
    (fixture, artifacts)
}

pub(super) fn supplement(fixture: &Fixture, artifacts: &Spec031ReleaseRunArtifacts) -> Value {
    let root = Path::new(&artifacts.evidence_root);
    let binding = serde_json::to_value(
        super::spec035_execution::preflight(fixture.repo.path())
            .expect("preflight")
            .binding,
    )
    .expect("binding");
    let mut paths: Vec<_> = super::spec035_postrun::original_paths(artifacts)
        .into_iter()
        .collect();
    paths.sort();
    let originals: Vec<_> = paths.iter().map(|path| file_ref(root, path)).collect();
    for (path, content) in [
        ("exit.stdout", "pending-final-audit\n"),
        ("exit.stderr", ""),
        (
            "review.stdout",
            "constructed independent review test data\n",
        ),
        ("review.stderr", ""),
    ] {
        std::fs::write(root.join("postrun").join(path), content).expect("test transcript");
    }
    let exit = json!({"binding":binding,"runner_manifest":file_ref(root,"manifest.json"),"originals_sha256":super::spec035_evidence_io::sha256(&serde_json::to_vec(&originals).expect("originals")),"argv":["spec031-release-runner","--run-id",artifacts.run_id.as_str(),"--mode","current-worktree","--repo-root",fixture.repo.path(),"--evidence-root",artifacts.evidence_root],"exit_code":0,"reaped":true,"stdout":file_ref(root,"postrun/exit.stdout"),"stderr":file_ref(root,"postrun/exit.stderr")});
    write_json(&root.join("postrun/exit.json"), &exit);
    let mut inputs = originals.clone();
    for path in [
        "postrun/exit.json",
        "postrun/exit.stdout",
        "postrun/exit.stderr",
        "postrun/review.stdout",
        "postrun/review.stderr",
    ] {
        inputs.push(file_ref(root, path));
    }
    let audit = review_receipt(&binding, "spec035:postrun-read-audit", &inputs);
    write_json(&root.join("postrun/read-audit.json"), &audit);
    let requirements: Vec<_> = super::spec035_preflight_test::DEFERRED
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let path = format!("postrun/row-{index}.json");
            write_json(
                &root.join(&path),
                &review_receipt(
                    &binding,
                    id,
                    &[
                        file_ref(root, "postrun/read-audit.json"),
                        file_ref(root, "postrun/exit.json"),
                        file_ref(root, "manifest.json"),
                    ],
                ),
            );
            json!({"id":id,"receipt":file_ref(root,&path)})
        })
        .collect();
    let postrun = json!({"binding":binding,"runner_manifest":file_ref(root,"manifest.json"),"originals":originals,"exit":file_ref(root,"postrun/exit.json"),"read_audit":file_ref(root,"postrun/read-audit.json"),"review_command":{"id":"independent-read-audit","kind":"review","run_id":binding["run_id"],"source_sha256":binding["source_sha256"],"argv":["independent-review-test-fixture"],"exit_code":0,"stdout":file_ref(root,"postrun/review.stdout"),"stderr":file_ref(root,"postrun/review.stderr")},"requirements":requirements});
    write_json(&root.join("postrun/finalization.json"), &postrun);
    postrun
}

fn review_receipt(binding: &Value, subject: &str, artifacts: &[Value]) -> Value {
    json!({"run_id":binding["run_id"],"source_sha256":binding["source_sha256"],"subject":subject,"verdict":"PASS","checks":[{"id":"constructed-independent-read","verdict":"PASS","producer":"crates/probe.rs:1","commands":["independent-read-audit"],"artifacts":artifacts}]})
}
