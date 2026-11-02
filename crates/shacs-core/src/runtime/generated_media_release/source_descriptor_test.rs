use super::*;
use std::error::Error;

fn tree(files: &[(&str, &[u8])]) -> Result<tempfile::TempDir, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (name, bytes) in files {
        std::fs::write(root.path().join(name), bytes)?;
    }
    Ok(root)
}

#[test]
fn dependency_trees_have_larger_limits_than_source_trees() {
    // Then
    assert_eq!(
        TreeKind::Cache.limits(),
        TreeLimits {
            bytes: 2 * 1024 * 1024 * 1024,
            entries: 131_072
        }
    );
    for kind in [
        TreeKind::Worktree,
        TreeKind::LiveWorktree,
        TreeKind::Git,
        TreeKind::ControlledGit,
    ] {
        assert_eq!(
            kind.limits(),
            TreeLimits {
                bytes: 512 * 1024 * 1024,
                entries: 32_768
            }
        );
    }
}

#[test]
fn a_tree_at_its_limits_is_read_and_one_byte_or_entry_more_is_invalid_evidence(
) -> Result<(), Box<dyn Error>> {
    // Given
    let root = tree(&[("a", b"12345"), ("b", b"67890")])?;
    let handle = File::open(root.path())?;
    let exact = TreeLimits {
        bytes: 10,
        entries: 2,
    };
    let over = [
        TreeLimits {
            bytes: 9,
            entries: 2,
        },
        TreeLimits {
            bytes: 10,
            entries: 1,
        },
    ];
    let copies = tempfile::tempdir()?;

    // When / Then
    assert!(digest_tree_within(&handle, TreeKind::Cache, exact).is_ok());
    assert!(copy_tree_within(
        &handle,
        &copies.path().join("exact"),
        TreeKind::Cache,
        exact
    )
    .is_ok());
    for (index, limits) in over.into_iter().enumerate() {
        assert!(matches!(
            digest_tree_within(&handle, TreeKind::Cache, limits),
            Err(Spec034ReleaseArtifactError::InvalidEvidence)
        ));
        assert!(matches!(
            copy_tree_within(
                &handle,
                &copies.path().join(format!("over-{index}")),
                TreeKind::Cache,
                limits
            ),
            Err(Spec034ReleaseArtifactError::InvalidEvidence)
        ));
    }
    Ok(())
}

#[test]
fn a_file_that_changes_while_read_stays_a_digest_mismatch() -> Result<(), Box<dyn Error>> {
    // Given
    let root = tree(&[("a", b"1"), ("b", b"2")])?;
    let first = std::fs::metadata(root.path().join("a"))?;
    let other = std::fs::metadata(root.path().join("b"))?;
    let mut budget = TreeBudget::new(TreeLimits {
        bytes: 1,
        entries: 2,
    });

    // When / Then
    budget.bytes = 1;
    assert!(check_read(&budget, &first, &first).is_ok());
    assert!(matches!(
        check_read(&budget, &first, &other),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    budget.bytes = 2;
    assert!(matches!(
        check_read(&budget, &first, &first),
        Err(Spec034ReleaseArtifactError::InvalidEvidence)
    ));
    assert!(matches!(
        check_read(&budget, &first, &other),
        Err(Spec034ReleaseArtifactError::DigestMismatch)
    ));
    Ok(())
}
