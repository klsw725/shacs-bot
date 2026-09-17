use super::model::Spec031ReleaseArtifactError as Error;
use super::spec035_execution::admit;
use super::spec035_execution_fixture::{file_ref, write_json, Fixture};
use super::spec035_execution_io::read_bound;
use serde_json::Value;
use std::fs;

fn inventory_alias(fixture: &mut Fixture, alias: &str) {
    let mut inventory: Value =
        serde_json::from_slice(&fs::read(fixture.root.join("inventory.json")).expect("inventory"))
            .expect("JSON");
    inventory
        .as_array_mut()
        .expect("entries")
        .push(file_ref(&fixture.root, alias));
    write_json(&fixture.root.join("inventory.json"), &inventory);
    fixture.manifest["inventory"] = file_ref(&fixture.root, "inventory.json");
    write_json(&fixture.root.join("manifest.json"), &fixture.manifest);
}

fn reject_transcript_alias(alias: &str) {
    let mut fixture = Fixture::new();
    assert!(admit(fixture.repo.path()).is_ok());
    let reference = file_ref(&fixture.root, alias);
    assert_eq!(
        reference["sha256"],
        fixture.manifest["commands"][0]["stdout"]["sha256"]
    );
    assert_eq!(
        fixture.root.join(alias).canonicalize().expect("alias"),
        fixture
            .root
            .join("commands/focused.stdout")
            .canonicalize()
            .expect("original")
    );
    fixture.manifest["commands"][1]["stdout"] = reference;
    inventory_alias(&mut fixture, alias);

    let result = admit(fixture.repo.path());

    assert!(
        matches!(result, Err(Error::InvalidArtifactPath)),
        "alias {alias}: expected InvalidArtifactPath, actual error {:?}",
        result.err()
    );
}

fn reject_snapshot_alias(alias: &str) {
    let mut fixture = Fixture::new();
    fs::rename(
        fixture.root.join("source-before.json"),
        fixture.root.join("commands/source-before.json"),
    )
    .expect("nested snapshot");
    fixture.manifest["source"]["before"] = file_ref(&fixture.root, "commands/source-before.json");
    fixture.save();
    assert!(admit(fixture.repo.path()).is_ok());
    let reference = file_ref(&fixture.root, alias);
    assert_eq!(
        reference["sha256"],
        fixture.manifest["source"]["before"]["sha256"]
    );
    assert_eq!(
        fixture.root.join(alias).canonicalize().expect("alias"),
        fixture
            .root
            .join("commands/source-before.json")
            .canonicalize()
            .expect("original")
    );
    fixture.manifest["source"]["after"] = reference;
    inventory_alias(&mut fixture, alias);

    let result = admit(fixture.repo.path());

    assert!(
        matches!(result, Err(Error::InvalidArtifactPath)),
        "alias {alias}: expected InvalidArtifactPath, actual error {:?}",
        result.err()
    );
}

#[test]
fn transcript_reuse_rejected_when_repeated_separator_has_matching_inventory_hash() {
    reject_transcript_alias("commands//focused.stdout");
}

#[test]
fn transcript_reuse_rejected_when_interior_dot_has_matching_inventory_hash() {
    reject_transcript_alias("commands/./focused.stdout");
}

#[test]
fn snapshot_reuse_rejected_when_repeated_separator_has_matching_inventory_hash() {
    reject_snapshot_alias("commands//source-before.json");
}

#[test]
fn snapshot_reuse_rejected_when_interior_dot_has_matching_inventory_hash() {
    reject_snapshot_alias("commands/./source-before.json");
}

#[test]
fn evidence_accepted_when_distinct_transcript_files_have_identical_bytes() {
    let fixture = Fixture::new();
    let focused = fixture.root.join("commands/focused.stdout");
    let workspace = fixture.root.join("commands/workspace.stdout");
    assert_ne!(
        focused.canonicalize().expect("focused"),
        workspace.canonicalize().expect("workspace")
    );
    assert_eq!(
        fs::read(focused).expect("focused"),
        fs::read(workspace).expect("workspace")
    );

    let result = admit(fixture.repo.path());

    assert!(result.is_ok());
}

#[test]
fn inventory_rejected_when_unreferenced_alias_has_matching_hash() {
    let mut fixture = Fixture::new();
    inventory_alias(&mut fixture, "commands//focused.stdout");

    let result = admit(fixture.repo.path());

    assert!(matches!(result, Err(Error::InvalidArtifactPath)));
}

#[test]
fn bound_read_rejects_noncanonical_relative_paths_before_io() {
    let root = tempfile::tempdir().expect("owned root");
    for path in [
        "",
        "/absolute",
        "./file",
        "dir/../file",
        "dir//file",
        "dir/./file",
        "file/",
        "dir\\file",
    ] {
        let reference = super::spec035_execution_model::FileRef {
            path: path.to_owned(),
            sha256: String::new(),
        };

        let result = read_bound(root.path(), &reference);

        assert_eq!(result, Err(Error::InvalidArtifactPath), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn transcript_reuse_rejected_when_contained_directory_symlink_has_matching_hash() {
    let mut fixture = Fixture::new();
    std::os::unix::fs::symlink("commands", fixture.root.join("alias")).expect("owned symlink");
    let alias = "alias/focused.stdout";
    fixture.manifest["commands"][1]["stdout"] = file_ref(&fixture.root, alias);
    inventory_alias(&mut fixture, alias);

    let result = admit(fixture.repo.path());

    assert!(matches!(result, Err(Error::InvalidArtifactPath)));
}

#[cfg(unix)]
#[test]
fn bound_read_preserves_leaf_symlink_rejection() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("commands/focused.stdout", fixture.root.join("alias.stdout"))
        .expect("owned symlink");
    let reference =
        serde_json::from_value(file_ref(&fixture.root, "alias.stdout")).expect("reference");

    let result = read_bound(&fixture.root, &reference);

    assert_eq!(result, Err(Error::InvalidArtifactPath));
}
