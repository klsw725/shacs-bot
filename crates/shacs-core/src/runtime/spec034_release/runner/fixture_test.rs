use super::*;

#[test]
fn mismatch_uses_isolated_tree() -> Result<(), Box<dyn std::error::Error>> {
    let canonical = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    let canonical_path = canonical.join(catalog::FIXTURES[0]);
    let canonical_bytes = std::fs::read(&canonical_path)?;
    let root = copy_canonical_fixtures(&canonical)?;
    let expected = digests(root.path())?;

    std::fs::write(root.path().join(catalog::FIXTURES[0]), b"isolated tamper")?;

    assert!(validate(root.path(), &expected).is_err());
    assert_eq!(std::fs::read(canonical_path)?, canonical_bytes);
    Ok(())
}

#[cfg(unix)]
#[test]
fn rejects_symlink_leaf_and_ancestor() -> Result<(), Box<dyn std::error::Error>> {
    let outside = tempfile::tempdir()?;
    let outside_file = outside.path().join("fixture.bin");
    std::fs::write(&outside_file, b"outside")?;
    let leaf_root = fixture_tree()?;
    let leaf = leaf_root.path().join(catalog::FIXTURES[0]);
    std::fs::remove_file(&leaf)?;
    std::os::unix::fs::symlink(&outside_file, &leaf)?;
    assert!(digests(leaf_root.path()).is_err());

    let ancestor_root = tempfile::tempdir()?;
    let first = Path::new(catalog::FIXTURES[0])
        .components()
        .next()
        .ok_or("fixture locator empty")?;
    std::os::unix::fs::symlink(outside.path(), ancestor_root.path().join(first.as_os_str()))?;
    assert!(digests(ancestor_root.path()).is_err());
    Ok(())
}

#[test]
fn accepts_regular_files() -> Result<(), Box<dyn std::error::Error>> {
    let root = fixture_tree()?;
    assert_eq!(digests(root.path())?.len(), catalog::FIXTURES.len());
    Ok(())
}

#[cfg(unix)]
#[test]
fn digest_uses_open_fixture_when_leaf_name_is_replaced() -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let root = fixture_tree()?;
    let locator = catalog::FIXTURES[0];
    let path = root.path().join(locator);
    let displaced = root.path().join("displaced-fixture");
    let expected = format!("sha256:{:x}", Sha256::digest(locator.as_bytes()));

    let rows = digests_with(root.path(), |opened| {
        if opened == locator {
            std::fs::rename(&path, &displaced).expect("displace fixture");
            std::fs::write(&path, b"replacement").expect("write replacement");
        }
    })?;

    assert_eq!(rows[0].digest, expected);
    Ok(())
}

#[cfg(unix)]
#[test]
fn digest_stays_bound_when_fixture_root_is_replaced() -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let parent = tempfile::tempdir()?;
    let root = parent.path().join("repo");
    std::fs::create_dir(&root)?;
    for locator in catalog::FIXTURES {
        let path = root.join(locator);
        std::fs::create_dir_all(path.parent().ok_or("fixture parent")?)?;
        std::fs::write(path, locator.as_bytes())?;
    }
    let displaced = parent.path().join("displaced");

    let rows = digests_with_hooks(
        &root,
        || {
            std::fs::rename(&root, &displaced).expect("displace fixture root");
            std::fs::create_dir(&root).expect("replace fixture root");
            for locator in catalog::FIXTURES {
                let path = root.join(locator);
                std::fs::create_dir_all(path.parent().expect("fixture parent"))
                    .expect("create replacement parent");
                std::fs::write(path, b"replacement").expect("write replacement");
            }
        },
        |_| {},
    )?;

    assert_eq!(
        rows[0].digest,
        format!("sha256:{:x}", Sha256::digest(catalog::FIXTURES[0].as_bytes()))
    );
    Ok(())
}

fn fixture_tree() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    for locator in catalog::FIXTURES {
        let destination = root.path().join(locator);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, locator.as_bytes())?;
    }
    Ok(root)
}

fn copy_canonical_fixtures(root: &Path) -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let destination = tempfile::tempdir()?;
    for locator in catalog::FIXTURES {
        let path = destination.path().join(locator);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(root.join(locator), path)?;
    }
    Ok(destination)
}
