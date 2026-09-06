use super::*;

#[test]
fn artifact_file_sync_failure_is_reported() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("artifact.json");

    let result = durable_write_with(&path, b"{}", |_| {
        Err(std::io::Error::other("injected file sync failure"))
    });

    assert!(matches!(result, Err(Spec034ReleaseArtifactError::Io(_))));
    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_rejects_symlinked_configured_root() -> Result<(), Box<dyn std::error::Error>> {
    let parent = tempfile::tempdir()?;
    let real = parent.path().join("real");
    let linked = parent.path().join("linked");
    std::fs::create_dir(&real)?;
    std::fs::write(real.join("manifest.json"), b"{}")?;
    std::os::unix::fs::symlink(&real, &linked)?;

    let result = ArtifactSnapshot::capture(&linked);

    assert!(result.is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_stays_bound_when_root_path_is_replaced() -> Result<(), Box<dyn std::error::Error>> {
    let parent = tempfile::tempdir()?;
    let root = parent.path().join("evidence");
    let displaced = parent.path().join("displaced");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let original = br#"{"state":"original"}"#;
    std::fs::write(root.join("results.json"), original)?;

    let snapshot = ArtifactSnapshot::capture_for_test(
        &root,
        || {
            std::fs::rename(&root, &displaced).expect("displace evidence root");
            std::fs::create_dir(&root).expect("replace evidence root");
            std::fs::write(root.join("results.json"), br#"{"state":"replacement"}"#)
                .expect("write replacement");
            std::fs::write(root.join("extra.json"), b"{}").expect("write extra artifact");
        },
        || {},
        |_| {},
    )?;

    let value: serde_json::Value = snapshot.json("results.json")?;
    assert_eq!(value["state"], "original");
    assert_eq!(snapshot.digest("results.json")?, digest_bytes(original));
    assert_eq!(snapshot.files().count(), 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn inventory_stays_on_one_descriptor_during_s_b_s_root_race(
) -> Result<(), Box<dyn std::error::Error>> {
    let parent = tempfile::tempdir()?;
    let root = parent.path().join("evidence");
    let displaced = parent.path().join("displaced");
    let replacement = parent.path().join("replacement");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    std::fs::write(root.join("a.json"), b"{}")?;
    std::fs::write(root.join("b.json"), b"{}")?;

    let snapshot = ArtifactSnapshot::capture_for_test(
        &root,
        || {},
        || {
            std::fs::rename(&root, &displaced).expect("displace S");
            std::fs::create_dir(&replacement).expect("create B");
            std::fs::write(replacement.join("extra.json"), b"{}").expect("write B");
            std::fs::rename(&replacement, &root).expect("install B");
            std::fs::remove_dir_all(&root).expect("remove B");
            std::fs::rename(&displaced, &root).expect("restore S");
        },
        |_| {},
    )?;

    assert_eq!(snapshot.files().map(|(name, _)| name).collect::<Vec<_>>(), ["a.json", "b.json"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn snapshot_reads_open_leaf_when_name_is_replaced() -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let canonical_root = root.path().canonicalize()?;
    let path = canonical_root.join("results.json");
    let displaced = root.path().join("displaced.json");
    let original = br#"{"state":"original"}"#;
    std::fs::write(&path, original)?;

    let snapshot = ArtifactSnapshot::capture_for_test(
        &canonical_root,
        || {},
        || {},
        |locator| {
            if locator == "results.json" {
                std::fs::rename(&path, &displaced).expect("displace leaf");
                std::fs::write(&path, br#"{"state":"replacement"}"#)
                    .expect("write replacement");
            }
        },
    )?;

    let value: serde_json::Value = snapshot.json("results.json")?;
    assert_eq!(value["state"], "original");
    assert_eq!(snapshot.digest("results.json")?, digest_bytes(original));
    Ok(())
}
