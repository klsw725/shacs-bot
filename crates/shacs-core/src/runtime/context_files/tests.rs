use super::*;
use std::fs;
use std::io;
use std::path::PathBuf;

#[test]
fn orders_nested_files_from_root_to_current() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    let nested = workspace.path().join("a/b");
    fs::create_dir_all(&nested)?;
    fs::write(workspace.path().join("AGENTS.md"), "root")?;
    fs::write(workspace.path().join("a/CLAUDE.md"), "middle")?;
    fs::write(nested.join(".shacs.md"), "leaf")?;
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            current_dir: Some(nested),
            ..ContextFileDiscoveryOptions::default()
        },
    );
    let names = discovery
        .entries
        .iter()
        .map(|entry| entry.filename.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["AGENTS.md", "CLAUDE.md", ".shacs.md"]);
    assert!(discovery
        .entries
        .iter()
        .all(|entry| entry.status == ContextFileReadStatus::Included));
    Ok(())
}

#[test]
fn keeps_duplicate_filenames_in_order() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    let nested = workspace.path().join("a");
    fs::create_dir_all(&nested)?;
    fs::write(workspace.path().join("AGENTS.md"), "root")?;
    fs::write(nested.join("AGENTS.md"), "nested")?;
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            current_dir: Some(nested),
            ..ContextFileDiscoveryOptions::default()
        },
    );
    assert_eq!(discovery.entries.len(), 2);
    assert_eq!(discovery.entries[0].source_directory_depth, 0);
    assert_eq!(discovery.entries[1].source_directory_depth, 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn denies_symlink_outside_workspace() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let outside_file = outside.path().join("AGENTS.md");
    fs::write(&outside_file, "outside")?;
    std::os::unix::fs::symlink(&outside_file, workspace.path().join("AGENTS.md"))?;
    let discovery =
        discover_context_files(workspace.path(), ContextFileDiscoveryOptions::default());
    assert_eq!(discovery.entries.len(), 1);
    assert_eq!(
        discovery.entries[0].status,
        ContextFileReadStatus::DeniedBoundary
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn denies_protected_symlink_target_inside_workspace() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    let env_file = workspace.path().join(".env");
    fs::write(&env_file, "SECRET_TOKEN=raw")?;
    std::os::unix::fs::symlink(&env_file, workspace.path().join("AGENTS.md"))?;
    let discovery =
        discover_context_files(workspace.path(), ContextFileDiscoveryOptions::default());
    assert_eq!(
        discovery.entries[0].status,
        ContextFileReadStatus::DeniedBoundary
    );
    assert!(discovery.entries[0].content.is_none());
    assert!(discovery.entries[0]
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("protected"));
    Ok(())
}

#[test]
fn truncates_oversized_files() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    fs::write(workspace.path().join("AGENTS.md"), "0123456789")?;
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            max_bytes: 4,
            ..ContextFileDiscoveryOptions::default()
        },
    );
    assert_eq!(
        discovery.entries[0].status,
        ContextFileReadStatus::Truncated
    );
    assert_eq!(discovery.entries[0].content.as_deref(), Some("0123"));
    assert_eq!(
        discovery.entries[0]
            .digest
            .as_ref()
            .map(|digest| digest.byte_count),
        Some(4)
    );
    Ok(())
}

#[test]
fn orders_configured_extras_after_defaults_and_reports_missing() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    fs::write(workspace.path().join("AGENTS.md"), "root")?;
    fs::write(workspace.path().join("z.md"), "z")?;
    fs::write(workspace.path().join("a.md"), "a")?;
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            extra_context_files: vec!["z.md".into(), "missing.md".into(), "a.md".into()],
            ..ContextFileDiscoveryOptions::default()
        },
    );
    let names = discovery
        .entries
        .iter()
        .map(|entry| entry.filename.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["AGENTS.md", "a.md", "missing.md", "z.md"]);
    assert_eq!(
        discovery.entries[2].status,
        ContextFileReadStatus::SkippedMissing
    );
    Ok(())
}

#[test]
fn deduplicates_configured_default_by_canonical_path_and_digest() -> io::Result<()> {
    let workspace = tempfile::tempdir()?;
    fs::write(workspace.path().join(".shacs.md"), "same context")?;
    fs::write(workspace.path().join("copy.md"), "same context")?;
    let discovery = discover_context_files(
        workspace.path(),
        ContextFileDiscoveryOptions {
            extra_context_files: vec![PathBuf::from(".shacs.md"), PathBuf::from("copy.md")],
            ..ContextFileDiscoveryOptions::default()
        },
    );
    assert_eq!(discovery.entries.len(), 3);
    assert_eq!(discovery.entries[0].status, ContextFileReadStatus::Included);
    assert!(discovery.entries[1..]
        .iter()
        .all(|entry| entry.status == ContextFileReadStatus::SkippedDuplicate));
    assert!(discovery.entries[1..]
        .iter()
        .all(|entry| entry.content.is_none()));
    Ok(())
}
