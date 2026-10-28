//! Spec031 release runner fixtures shared by `tests/spec031_release_runner_exploits.rs`
//! (loaded with `#[path]`) and the `it` binary. Keep tests out of this file: both
//! binaries compile it, so a test here would run twice.

use shacs_projection::{
    canonical_current_command_specs, run_spec031_release_runner, write_spec031_release_artifacts,
    Spec031ArtifactMediaType, Spec031CoverageEvidenceKind, Spec031CoverageRequirementKind,
    Spec031CoverageStatus, Spec031ExternalAuditRow, Spec031ExternalAuditStatus,
    Spec031ExternalOwnerId, Spec031ReleaseCommandRecord, Spec031ReleaseCommandStatus,
    Spec031ReleaseCoverageEntry, Spec031ReleaseRunArtifacts, Spec031ReleaseRunId,
    Spec031ReleaseRunnerConfig, Spec031ReleaseRunnerMode, Spec031ReleaseTestCounts,
    Spec031TypedEvidenceClass, SPEC031_RELEASE_RUNNER_SCHEMA,
};
use std::fs;
use std::hash::{Hash, Hasher};
use std::time::Duration;

pub(crate) fn valid_artifacts_on_disk(
    label: &str,
) -> Result<(std::path::PathBuf, Spec031ReleaseRunArtifacts), Box<dyn std::error::Error>> {
    let root = temp_path(label);
    let artifacts = run_spec031_release_runner(&Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("spec031-run-20")?,
        evidence_root: root.clone(),
        repo_root: workspace_root(),
        mode: Spec031ReleaseRunnerMode::SuccessFixture,
        command_timeout: Duration::from_secs(30),
    })?;
    Ok((root, artifacts))
}

pub(crate) fn valid_non_fixture_artifacts_on_disk(
    label: &str,
) -> Result<
    (
        std::path::PathBuf,
        std::path::PathBuf,
        Spec031ReleaseRunArtifacts,
    ),
    Box<dyn std::error::Error>,
> {
    let repo_root = temp_path(&format!("{label}-repo"));
    let (root, mut artifacts) = valid_artifacts_on_disk(label)?;
    fs::create_dir_all(repo_root.join("crates"))?;
    fs::write(repo_root.join("crates/Cargo.toml"), "[workspace]\n")?;
    let repo_root = repo_root.canonicalize()?;
    artifacts.fixture_registry = vec!["fixtures/current-worktree.json".to_owned()];
    artifacts.cleanup_registry = vec!["cleanup/success-receipt.json".to_owned()];
    artifacts.command_registry = canonical_current_commands(&root, &repo_root)?;
    fs::write(
        root.join("fixtures/current-worktree.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": artifacts.run_id.as_str(),
            "resource_id": "current-worktree"
        }))?,
    )?;
    fs::write(
        root.join("cleanup/success-receipt.json"),
        serde_json::to_vec(&serde_json::json!({
            "schema": SPEC031_RELEASE_RUNNER_SCHEMA,
            "run_id": artifacts.run_id.as_str(),
            "status": "verified",
            "success": true,
            "resource_id": "current-worktree",
            "check_artifact": "commands/spec031-test-release-runner.stdout"
        }))?,
    )?;
    write_exact_owner_fact_files(&repo_root)?;
    write_complete_owner_sources(&repo_root)?;
    for source in [
        "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/SPEC.md",
        "docs/specs/031-configuration-runtime-layout-and-execution-snapshots/prds/005-sequential-integration-and-spec031-closure.md",
    ] {
        let target = repo_root.join(source);
        fs::create_dir_all(target.parent().ok_or("spec031 source has no parent")?)?;
        fs::copy(workspace_root().join(source), target)?;
    }
    for audit in &mut artifacts.external_audits {
        audit.implementation_artifacts = exact_fact_artifacts(audit.owner)
            .iter()
            .map(|artifact| (*artifact).to_owned())
            .collect();
        rewrite_audit_file(&root, audit)?;
    }
    refresh_coverage_hashes(&root, &mut artifacts)?;
    write_artifacts(&root, &artifacts)?;
    Ok((repo_root, root, artifacts))
}

pub(crate) const RETIRED_SPEC034_ARCHIVE: &str =
    "docs/archive/spec034-video-analyzer/runtime_agent_video_injection_test.rs";
const RETIRED_SPEC034_FACT: &str =
    "crates/shacs-core/tests/runtime_agent.rs#runtime_context_routes_stored_video_with_injected_analyzer";

pub(crate) const RETIRED_SPEC032_ARCHIVE: &str =
    "docs/archive/spec043-app-environment/app_environment_lifecycle_projection_test.rs";
const RETIRED_SPEC032_FACT: &str =
    "crates/shacs-app/tests/app_environment.rs#enable_disable_lifecycle_projection_does_not_create_process_truth";

fn current_owner_spec(
    repo_root: &std::path::Path,
    root: &std::path::Path,
    id: &str,
) -> Result<shacs_projection::Spec031ReleaseCommandSpec, Box<dyn std::error::Error>> {
    let config = Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("canonical-current-commands")?,
        evidence_root: root.to_path_buf(),
        repo_root: repo_root.to_path_buf(),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::ZERO,
    };
    Ok(canonical_current_command_specs(&config)
        .into_iter()
        .find(|spec| spec.id == id)
        .ok_or("owner command")?)
}

/// Rebuilds a recorded pre-v5 run: those runs still cite the retired Spec032
/// owner, whose deleted test is checked through the pinned archive copy.
pub(crate) fn convert_to_legacy_run(
    repo_root: &std::path::Path,
    root: &std::path::Path,
    artifacts: &mut Spec031ReleaseRunArtifacts,
    schema: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    artifacts.schema = schema.to_owned();
    for receipt in artifacts
        .fixture_registry
        .iter()
        .chain(&artifacts.cleanup_registry)
    {
        let path = root.join(receipt);
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
        value["schema"] = serde_json::json!(artifacts.schema);
        fs::write(&path, serde_json::to_vec(&value)?)?;
    }
    let retired_test = RETIRED_SPEC032_FACT
        .split_once('#')
        .map_or("", |(_, name)| name);
    let mut spec = current_owner_spec(repo_root, root, "spec031-owner-spec033")?;
    spec.id = "spec031-owner-spec032".to_owned();
    spec.package = Some("shacs-app".to_owned());
    spec.filter = Some(retired_test.to_owned());
    for argument in &mut spec.argv {
        *argument = argument
            .replace("shacs-core", "shacs-app")
            .replace("runtime_loop", "app_environment")
            .replace(
                "automation_channel_event_projects_delivery_only_when_user_visible",
                retired_test,
            );
    }
    artifacts
        .command_registry
        .push(current_record(root, &spec)?);
    let source = "docs/specs/032-app-maker-runtime-and-extension-lifecycle/SPEC.md";
    let source_path = repo_root.join(source);
    fs::create_dir_all(source_path.parent().ok_or("source has no parent")?)?;
    fs::write(
        source_path,
        format!(
            "# Test source\n\n{}\n",
            source_status_for_owner(Spec031ExternalOwnerId::Spec032)
        ),
    )?;
    let mut audit = Spec031ExternalAuditRow {
        owner: Spec031ExternalOwnerId::Spec032,
        status: Spec031ExternalAuditStatus::Pass,
        source_locator: source.to_owned(),
        source_status_locator: format!("{source}:3"),
        implementation_artifacts: vec![RETIRED_SPEC032_FACT.to_owned()],
        command_result_ids: vec!["spec031-owner-spec032".to_owned()],
        artifact: "external/spec032-read-audit.md".to_owned(),
        artifact_media_type: Spec031ArtifactMediaType::Markdown,
        evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
        artifact_hash: String::new(),
        reason: "artifact-backed exact fact audit passes".to_owned(),
    };
    rewrite_audit_file(root, &mut audit)?;
    artifacts.coverage_matrix.push(Spec031ReleaseCoverageEntry {
        requirement_id: "spec031:external:spec032".to_owned(),
        kind: Spec031CoverageRequirementKind::ExternalOwner,
        source_locator: audit.source_status_locator.clone(),
        owner: "spec031".to_owned(),
        status: Spec031CoverageStatus::Pass,
        evidence_kind: Spec031CoverageEvidenceKind::ExternalAudit,
        evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
        artifact_media_type: Spec031ArtifactMediaType::Markdown,
        artifact_hash: String::new(),
        artifact: audit.artifact.clone(),
        command_result_id: None,
        reason: "external owner verdict is derived from observed spec status and audit inputs"
            .to_owned(),
    });
    artifacts.external_audits.insert(2, audit);
    let archive = repo_root.join(RETIRED_SPEC032_ARCHIVE);
    fs::create_dir_all(archive.parent().ok_or("archive has no parent")?)?;
    fs::copy(workspace_root().join(RETIRED_SPEC032_ARCHIVE), archive)?;
    refresh_coverage_hashes(root, artifacts)?;
    write_artifacts(root, artifacts)?;
    Ok(())
}

pub(crate) fn convert_to_v3_retired_spec034_run(
    repo_root: &std::path::Path,
    root: &std::path::Path,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Box<dyn std::error::Error>> {
    convert_to_legacy_run(repo_root, root, artifacts, "spec031.release_runner.v3")?;
    let mut spec = current_owner_spec(repo_root, root, "spec031-owner-spec034")?;
    let retired_test = RETIRED_SPEC034_FACT
        .split_once('#')
        .map_or("", |(_, name)| name);
    for argument in &mut spec.argv {
        *argument = argument
            .replace("spec034_persistence_operations", "runtime_agent")
            .replace(
                "final_edit_and_variation_candidates_publish_one_complete_record_each",
                retired_test,
            );
    }
    spec.filter = Some(retired_test.to_owned());
    let record = current_record(root, &spec)?;
    let position = artifacts
        .command_registry
        .iter()
        .position(|existing| existing.id == record.id)
        .ok_or("spec034 owner record")?;
    artifacts.command_registry[position] = record;
    let spec034 = audit_for_owner_mut(artifacts, Spec031ExternalOwnerId::Spec034);
    spec034.implementation_artifacts = vec![RETIRED_SPEC034_FACT.to_owned()];
    rewrite_audit_file(root, spec034)?;
    let archive = repo_root.join(RETIRED_SPEC034_ARCHIVE);
    fs::create_dir_all(archive.parent().ok_or("archive has no parent")?)?;
    fs::copy(workspace_root().join(RETIRED_SPEC034_ARCHIVE), archive)?;
    refresh_coverage_hashes(root, artifacts)?;
    write_artifacts(root, artifacts)?;
    Ok(())
}

pub(crate) fn canonical_current_commands(
    root: &std::path::Path,
    repo_root: &std::path::Path,
) -> Result<Vec<Spec031ReleaseCommandRecord>, Box<dyn std::error::Error>> {
    let config = Spec031ReleaseRunnerConfig {
        run_id: Spec031ReleaseRunId::try_new("canonical-current-commands")?,
        evidence_root: root.to_path_buf(),
        repo_root: repo_root.to_path_buf(),
        mode: Spec031ReleaseRunnerMode::CurrentWorktree,
        command_timeout: Duration::ZERO,
    };
    let specs = canonical_current_command_specs(&config);
    specs
        .iter()
        .map(|spec| current_record(root, spec))
        .collect()
}

fn current_record(
    root: &std::path::Path,
    spec: &shacs_projection::Spec031ReleaseCommandSpec,
) -> Result<Spec031ReleaseCommandRecord, Box<dyn std::error::Error>> {
    let tests = matches!(spec.argv.get(1).map(String::as_str), Some("test"));
    let exact = exact_name(&spec.argv);
    let stdout = exact.map_or_else(
        || {
            if tests {
                "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n".to_owned()
            } else {
                String::new()
            }
        },
        |name| format!("test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"),
    );
    let stderr = spec
        .argv
        .windows(2)
        .filter_map(|pair| (pair[0] == "--test").then_some(pair[1].as_str()))
        .map(|target| format!("Running tests/{target}.rs\n"))
        .collect::<String>();
    let stdout_path = format!("commands/{}.stdout", spec.id);
    let stderr_path = format!("commands/{}.stderr", spec.id);
    fs::write(root.join(&stdout_path), stdout)?;
    fs::write(root.join(&stderr_path), stderr)?;
    Ok(Spec031ReleaseCommandRecord {
        id: spec.id.clone(),
        gate: spec.gate,
        package: spec.package.clone(),
        filter: spec.filter.clone(),
        argv: spec.argv.clone(),
        cwd: spec.cwd.display().to_string(),
        status: Spec031ReleaseCommandStatus::Passed,
        exit_code: Some(0),
        duration_ms: 1,
        stdout_path,
        stderr_path,
        tests: tests.then_some(Spec031ReleaseTestCounts {
            tests_run: 1,
            tests_failed: 0,
        }),
        process_receipt: None,
    })
}

fn exact_name(argv: &[String]) -> Option<&str> {
    let separator = argv.iter().position(|arg| arg == "--")?;
    argv.get(separator.checked_sub(1)?).map(String::as_str)
}

fn write_complete_owner_sources(
    repo_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for (owner, _, source_locator, _) in external_rows() {
        let path = repo_root.join(source_locator);
        let parent = path.parent().ok_or("source locator has no parent")?;
        fs::create_dir_all(parent)?;
        fs::write(
            path,
            format!("# Test source\n\n{}\n", source_status_for_owner(owner)),
        )?;
    }
    for archive_file in [
        "docs/archive/spec035/README.md",
        "docs/archive/spec035/current/SHA256SUMS",
        "docs/archive/spec035/current/SPEC.md",
        "docs/archive/spec035/current/CLOSURE.md",
        "docs/archive/spec035/current/prds/000-shared-projection-model-and-vocabulary.md",
        "docs/archive/spec035/current/prds/001-surface-adapter-parity-cli-api-channel.md",
        "docs/archive/spec035/current/prds/002-approval-progress-and-recovery-parity.md",
        "docs/archive/spec035/current/prds/003-readiness-degraded-health-and-diagnostics.md",
        "docs/archive/spec035/current/prds/004-context-extension-app-and-media-projection.md",
        "docs/archive/spec035/current/prds/005-interactive-tui-repl-and-onboard-flows.md",
        "docs/archive/spec035/current/prds/006-reconnect-backpressure-and-drop-accounting.md",
        "docs/archive/spec035/current/prds/007-release-runner-and-spec035-closure.md",
        "docs/archive/spec035/current/prds/008-transport-capability-and-snapshot-first-reconnect.md",
        "docs/archive/spec035/current/prds/009-goal-and-read-only-tasks-projection.md",
    ] {
        let target = repo_root.join(archive_file);
        fs::create_dir_all(target.parent().ok_or("archive file has no parent")?)?;
        fs::copy(workspace_root().join(archive_file), target)?;
    }
    Ok(())
}

fn write_exact_owner_fact_files(
    repo_root: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for (_, _, _, _) in external_rows() {
        for artifact in exact_fact_artifacts_by_index().into_iter().flatten() {
            let (path, token) = artifact.split_once('#').unwrap_or((artifact, "owner fact"));
            let full_path = repo_root.join(path);
            let parent = full_path.parent().ok_or("fact artifact has no parent")?;
            fs::create_dir_all(parent)?;
            fs::write(full_path, format!("{token}\n"))?;
        }
    }
    Ok(())
}

pub(crate) fn rewrite_audit_file(
    root: &std::path::Path,
    audit: &mut Spec031ExternalAuditRow,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = format!(
        "# Audit\n\nowner: {}\nstatus: pass\nverdict: pass\nsource_locator: {}\nsource_status_locator: {}\nsource_status: {}\nimplementation_artifacts: {}\ncommand_result_ids: {}\nreason: {}\n",
        owner_slug(audit.owner),
        audit.source_locator,
        audit.source_status_locator,
        source_status_for_owner(audit.owner),
        audit.implementation_artifacts.join(", "),
        audit.command_result_ids.join(", "),
        audit.reason,
    );
    fs::write(root.join(&audit.artifact), text)?;
    audit.artifact_hash = hash_path(&root.join(&audit.artifact))?;
    Ok(())
}

pub(crate) fn refresh_coverage_hashes(
    root: &std::path::Path,
    artifacts: &mut Spec031ReleaseRunArtifacts,
) -> Result<(), Box<dyn std::error::Error>> {
    for row in &mut artifacts.coverage_matrix {
        row.artifact_hash = match row.artifact.as_str() {
            "manifest.json"
            | "coverage-matrix.json"
            | "results.json"
            | "failure-triage.json"
            | "summary.md" => format!("self:{}", row.artifact),
            _ => hash_path(&root.join(&row.artifact))?,
        };
    }
    Ok(())
}

pub(crate) fn audit_for_owner_mut(
    artifacts: &mut Spec031ReleaseRunArtifacts,
    owner: Spec031ExternalOwnerId,
) -> &mut Spec031ExternalAuditRow {
    artifacts
        .external_audits
        .iter_mut()
        .find(|audit| audit.owner == owner)
        .expect("owner audit exists")
}

fn exact_fact_artifacts(owner: Spec031ExternalOwnerId) -> &'static [&'static str] {
    match owner {
        Spec031ExternalOwnerId::Spec029 => exact_fact_artifacts_by_index()[0],
        Spec031ExternalOwnerId::Spec030 => exact_fact_artifacts_by_index()[1],
        Spec031ExternalOwnerId::Spec033 => exact_fact_artifacts_by_index()[2],
        Spec031ExternalOwnerId::Spec034 => exact_fact_artifacts_by_index()[3],
        Spec031ExternalOwnerId::Spec032 | Spec031ExternalOwnerId::Spec035 => &[],
    }
}

fn exact_fact_artifacts_by_index() -> [&'static [&'static str]; 4] {
    [
        &[
            "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md#Status: Complete (Scoped)",
            "crates/shacs-core/tests/durable_dispatch.rs#durable_dispatch_restores_due_inbound_and_requeues_stale_lease",
            "crates/shacs-core/tests/durable_child.rs#subagent_runtime_restart_discards_late_success_after_durable_cancellation",
        ],
        &["crates/shacs-core/tests/spec030_local_provider.rs#local_spec030_provider_discovers_live_resources_diagnostics_and_trace"],
        &["crates/shacs-core/tests/runtime_loop.rs#automation_channel_event_projects_delivery_only_when_user_visible"],
        &["crates/shacs-core/tests/spec034_persistence_operations.rs#final_edit_and_variation_candidates_publish_one_complete_record_each"],
    ]
}

fn source_status_for_owner(owner: Spec031ExternalOwnerId) -> &'static str {
    match owner {
        Spec031ExternalOwnerId::Spec029 => "Status: Complete (Scoped)",
        Spec031ExternalOwnerId::Spec030 => "Status: Complete (Scoped)",
        Spec031ExternalOwnerId::Spec032
        | Spec031ExternalOwnerId::Spec033
        | Spec031ExternalOwnerId::Spec034
        | Spec031ExternalOwnerId::Spec035 => "Status: Open",
    }
}

fn hash_path(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = fs::read(path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Ok(format!("fnv64:{:016x}", hasher.finish()))
}

pub(crate) fn external_rows() -> [(
    Spec031ExternalOwnerId,
    &'static str,
    &'static str,
    &'static str,
); 4] {
    [
        (
            Spec031ExternalOwnerId::Spec029,
            "spec029-read-audit.md",
            "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md",
            "docs/specs/029-durable-runtime-recovery-and-data-migration/SPEC.md:3",
        ),
        (
            Spec031ExternalOwnerId::Spec030,
            "spec030-read-audit.md",
            "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md",
            "docs/specs/030-trusted-agent-runtime-and-operational-controls/SPEC.md:3",
        ),
        (
            Spec031ExternalOwnerId::Spec033,
            "spec033-read-audit.md",
            "docs/specs/033-evaluation-automation-live-integration/SPEC.md",
            "docs/specs/033-evaluation-automation-live-integration/SPEC.md:3",
        ),
        (
            Spec031ExternalOwnerId::Spec034,
            "spec034-read-audit.md",
            "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md",
            "docs/specs/034-generated-media-and-rich-file-context-expansion/SPEC.md:3",
        ),
    ]
}

pub(crate) fn owner_slug(owner: Spec031ExternalOwnerId) -> &'static str {
    match owner {
        Spec031ExternalOwnerId::Spec029 => "spec029",
        Spec031ExternalOwnerId::Spec030 => "spec030",
        Spec031ExternalOwnerId::Spec032 => "spec032",
        Spec031ExternalOwnerId::Spec033 => "spec033",
        Spec031ExternalOwnerId::Spec034 => "spec034",
        Spec031ExternalOwnerId::Spec035 => "spec035",
    }
}

pub(crate) fn write_artifacts(
    root: &std::path::Path,
    artifacts: &Spec031ReleaseRunArtifacts,
) -> Result<(), Box<dyn std::error::Error>> {
    remove_manifest_files(root)?;
    write_spec031_release_artifacts(artifacts, root)?;
    Ok(())
}

fn remove_manifest_files(root: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    for file in [
        "manifest.json",
        "coverage-matrix.json",
        "results.json",
        "failure-triage.json",
        "reproducibility-observations.json",
        "fixture-registry.json",
        "command-registry.json",
        "cleanup-registry.json",
        "summary.md",
        "evidence-index.json",
    ] {
        let path = root.join(file);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub(crate) fn temp_path(label: &str) -> std::path::PathBuf {
    temp_base().join(format!(
        "shacs-spec031-exploit-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos())
    ))
}

fn temp_base() -> std::path::PathBuf {
    std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir())
}

pub(crate) fn workspace_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("workspace root exists")
        .to_path_buf()
}
