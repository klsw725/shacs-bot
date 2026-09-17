use super::coverage::{
    artifact_hash, Spec031ArtifactMediaType, Spec031CoverageStatus, Spec031ExternalAuditRow,
    Spec031ExternalAuditStatus, Spec031ExternalOwnerId, Spec031TypedEvidenceClass,
};
use super::spec035_evidence::validate_spec035_closure_evidence;
use super::spec035_execution_fixture::Fixture;
use std::fs;

#[test]
fn spec035_execution_rejects_one_transcript_reused_as_two_commands() {
    let mut fixture = Fixture::new();
    fixture.manifest["commands"][1]["stdout"] = fixture.manifest["commands"][0]["stdout"].clone();
    fixture.save();

    assert!(super::spec035_execution::admit(fixture.repo.path()).is_err());
}

#[test]
fn spec035_execution_accepts_complete_constructed_evidence_through_real_validator() {
    let fixture = Fixture::new();

    let result = validate_spec035_closure_evidence(fixture.repo.path());

    assert_eq!(result, Ok(()));
}

#[test]
fn spec035_execution_admits_real_coverage_without_success_fixture_facts() {
    let fixture = Fixture::new();
    let execution = super::spec035_execution::admit(fixture.repo.path())
        .expect("real validator admits constructed evidence");
    let output = fixture.repo.path().join(".omo/run");
    fs::create_dir_all(output.join("external")).expect("output");
    fs::write(
        output.join("external/spec035-read-audit.md"),
        "constructed real-input audit\n",
    )
    .expect("audit");
    fs::write(
        output.join(super::spec035_execution::BINDING),
        serde_json::to_vec(&execution.binding).expect("binding"),
    )
    .expect("binding writes");
    let audit = Spec031ExternalAuditRow {
        owner: Spec031ExternalOwnerId::Spec035,
        status: Spec031ExternalAuditStatus::Pass,
        source_locator: format!("{}/SPEC.md", super::spec035_catalog::SPEC_ROOT),
        source_status_locator: format!("{}/SPEC.md:3", super::spec035_catalog::SPEC_ROOT),
        implementation_artifacts: vec![super::spec035_execution::MANIFEST.to_owned()],
        command_result_ids: vec!["spec031-owner-spec035".to_owned()],
        artifact: "external/spec035-read-audit.md".to_owned(),
        artifact_media_type: Spec031ArtifactMediaType::Markdown,
        evidence_class: Spec031TypedEvidenceClass::ExternalAuditMarkdown,
        artifact_hash: artifact_hash(&output, "external/spec035-read-audit.md").expect("hash"),
        reason: "constructed test input, not semantic closure".to_owned(),
    };

    let rows = super::spec035_coverage::coverage_rows(&output, &[audit], fixture.repo.path())
        .expect("coverage");

    assert_eq!(rows.len(), 80);
    assert!(rows
        .iter()
        .all(|row| row.status == Spec031CoverageStatus::Pass));
}

#[test]
fn spec035_execution_rejects_weak_cargo_gates() {
    for (index, argv) in [
        (
            1,
            serde_json::json!([
                "cargo",
                "test",
                "--manifest-path",
                "crates/Cargo.toml",
                "--locked",
                "--workspace",
                "-p",
                "probe"
            ]),
        ),
        (
            2,
            serde_json::json!(["cargo", "fmt", "--manifest-path", "crates/Cargo.toml"]),
        ),
        (
            3,
            serde_json::json!(["cargo", "clippy", "--manifest-path", "crates/Cargo.toml"]),
        ),
    ] {
        let mut fixture = Fixture::new();
        fixture.manifest["commands"][index]["argv"] = argv;
        fixture.save();
        assert!(
            super::spec035_execution::admit(fixture.repo.path()).is_err(),
            "command {index}"
        );
    }
}

#[test]
fn spec035_execution_binding_rejects_even_valid_manifest_replacement() {
    let fixture = Fixture::new();
    let execution = super::spec035_execution::admit(fixture.repo.path()).expect("valid input");
    let output = fixture.repo.path().join(".omo/binding");
    fs::create_dir(&output).expect("output");
    fs::write(
        output.join(super::spec035_execution::BINDING),
        serde_json::to_vec(&execution.binding).expect("binding"),
    )
    .expect("write binding");
    fs::write(
        fixture.root.join("manifest.json"),
        serde_json::to_vec(&fixture.manifest).expect("equivalent JSON bytes"),
    )
    .expect("replace");
    assert!(super::spec035_execution::admit(fixture.repo.path()).is_ok());
    assert!(matches!(
        super::spec035_execution::validate_bound(fixture.repo.path(), &output),
        Err(super::model::Spec031ReleaseArtifactError::ArtifactMismatch)
    ));
}
