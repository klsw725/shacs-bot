use super::model::Spec031ReleaseArtifactError;
use super::writer::write_text;
use crate::spec031::evidence_writer::EvidenceWriter;

pub(super) fn prepare_success_fixture_project(
    writer: &EvidenceWriter,
) -> Result<(), Spec031ReleaseArtifactError> {
    writer
        .create_dir_all("fixtures/success-fixture/src")
        .map_err(|_| Spec031ReleaseArtifactError::Io)?;
    write_text(
        writer,
        "fixtures/success-fixture/Cargo.toml",
        "[package]\nname = \"spec031-success-fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\npath = \"src/lib.rs\"\n",
    )?;
    write_text(
        writer,
        "fixtures/success-fixture/src/lib.rs",
        "pub fn spec031_release_runner_success_fixture() -> bool { true }\n\n#[cfg(test)]\nmod tests {\n    use super::spec031_release_runner_success_fixture;\n\n    #[test]\n    fn spec031_release_runner_success_fixture_passes() {\n        assert!(spec031_release_runner_success_fixture());\n    }\n}\n",
    )
}
