use super::*;

fn io<T>(result: std::io::Result<T>) -> Result<T, Spec034ReleaseArtifactError> {
    result.map_err(Spec034ReleaseArtifactError::Io)
}

#[test]
fn handle_a_and_path_b_content_never_mix_during_a_b_a_race(
) -> Result<(), Spec034ReleaseArtifactError> {
    let root = io(tempfile::tempdir())?;
    let root = io(root.path().canonicalize())?;
    let displaced = root.join("handle-a");
    let replacement = root.join("path-b");
    let destination = EvidenceDestination::prepare(&root.join("evidence"))?;
    let staging = destination.staging()?;
    io(std::fs::write(staging.path().join("manifest.json"), b"content-a"))?;
    io(std::fs::create_dir(&replacement))?;
    io(std::fs::write(replacement.join("manifest.json"), b"content-b"))?;
    io(std::fs::write(replacement.join("extra.json"), b"content-b"))?;
    let staging_path = staging.path().to_path_buf();

    let result = staging.finalize_marker_for_test("run", || {
        assert!(std::fs::rename(&staging_path, &displaced).is_ok());
        assert!(std::fs::rename(&replacement, &staging_path).is_ok());
        assert!(std::fs::rename(&staging_path, &replacement).is_ok());
        assert!(std::fs::rename(&displaced, &staging_path).is_ok());
    });

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    assert_eq!(io(std::fs::read(replacement.join("manifest.json")))?, b"content-b");
    Ok(())
}

#[test]
fn inventory_add_and_read_file_mutation_between_snapshots_are_rejected(
) -> Result<(), Spec034ReleaseArtifactError> {
    for add_inventory in [true, false] {
        let root = io(tempfile::tempdir())?;
        let root = io(root.path().canonicalize())?;
        let destination = EvidenceDestination::prepare(&root.join("evidence"))?;
        let staging = destination.staging()?;
        io(std::fs::write(staging.path().join("manifest.json"), b"first"))?;
        let staging_path = staging.path().to_path_buf();

        let result = staging.finalize_marker_for_test("run", || {
            let path = if add_inventory {
                staging_path.join("added.json")
            } else {
                staging_path.join("manifest.json")
            };
            assert!(std::fs::write(path, b"second").is_ok());
        });

        assert!(matches!(result, Err(Spec034ReleaseArtifactError::DigestMismatch)));
    }
    Ok(())
}
