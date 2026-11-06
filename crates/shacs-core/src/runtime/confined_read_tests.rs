use super::*;
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

struct Places {
    _root: tempfile::TempDir,
    workspace: PathBuf,
    outside: PathBuf,
}

fn places() -> Result<Places, Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let base = fs::canonicalize(root.path())?;
    let (workspace, outside) = (base.join("ws"), base.join("outside"));
    fs::create_dir_all(workspace.join("docs"))?;
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("README.md"), "outside text")?;
    Ok(Places {
        _root: root,
        workspace,
        outside,
    })
}

fn fifo(path: &Path) -> Result<(), Box<dyn Error>> {
    let made = std::process::Command::new("mkfifo").arg(path).status()?;
    if !made.success() {
        return Err(format!("mkfifo failed: {made}").into());
    }
    Ok(())
}

#[test]
fn files_are_read_only_through_folders_and_files_that_are_not_symlinks(
) -> Result<(), Box<dyn Error>> {
    // Given
    let places = places()?;
    let workspace = &places.workspace;
    fs::write(workspace.join("AGENTS.md"), "inside text")?;
    fs::write(workspace.join("docs/notes.md"), "notes")?;
    symlink(
        places.outside.join("README.md"),
        workspace.join("README.md"),
    )?;
    symlink(&places.outside, workspace.join("linked"))?;
    fs::write(workspace.join(".git"), "gitdir: ../elsewhere")?;
    fifo(&workspace.join("pipe"))?;
    fs::hard_link(
        places.outside.join("README.md"),
        workspace.join("linked.md"),
    )?;
    let folder = ConfinedFolder::open(workspace).map_err(|skip| skip.reason())?;

    // When
    let inside = folder.read_prefix(Path::new("AGENTS.md"), 6);
    let swapped_parent = {
        fs::rename(workspace.join("docs"), workspace.join("docs-old"))?;
        symlink(&places.outside, workspace.join("docs"))?;
        folder.read_prefix(Path::new("docs/README.md"), 64)
    };

    // Then
    assert_eq!(inside, Ok(b"inside".to_vec()));
    assert_eq!(
        folder.read_prefix(Path::new("README.md"), 64),
        Err(ConfinedSkip::Symlink)
    );
    assert_eq!(
        folder.read_prefix(Path::new("linked/README.md"), 64),
        Err(ConfinedSkip::Symlink)
    );
    assert_eq!(swapped_parent, Err(ConfinedSkip::Symlink));
    assert_eq!(
        folder.read_prefix(Path::new("pipe"), 64),
        Err(ConfinedSkip::NotRegularFile)
    );
    assert_eq!(
        folder.read_prefix(Path::new("linked.md"), 64),
        Err(ConfinedSkip::HardLink)
    );
    assert_eq!(
        folder.read_prefix(Path::new(".git/config"), 64),
        Err(ConfinedSkip::NotAFolder)
    );
    assert_eq!(
        folder.read_prefix(Path::new("missing.md"), 64),
        Err(ConfinedSkip::Missing)
    );
    assert_eq!(
        ConfinedFolder::open(&workspace.join("linked")).map(|_| ()),
        Err(ConfinedSkip::Symlink)
    );
    Ok(())
}

#[test]
fn an_open_folder_keeps_listing_and_reading_its_own_entries_after_a_swap(
) -> Result<(), Box<dyn Error>> {
    // Given
    let places = places()?;
    let sessions = places.workspace.join("sessions");
    fs::create_dir_all(&sessions)?;
    fs::write(sessions.join("cli_a.jsonl"), "inside session")?;
    symlink(
        places.outside.join("README.md"),
        sessions.join("linked.jsonl"),
    )?;
    fifo(&sessions.join("pipe.jsonl"))?;
    fs::write(places.outside.join("cli_b.jsonl"), "outside session")?;
    let workspace = ConfinedFolder::open(&places.workspace).map_err(|skip| skip.reason())?;
    let opened = workspace
        .folder(Path::new("sessions"))
        .map_err(|skip| skip.reason())?;

    // When
    fs::rename(&sessions, places.workspace.join("sessions-old"))?;
    symlink(&places.outside, &sessions)?;
    let names = opened
        .regular_files()
        .map_err(|skip| skip.reason())?
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();

    // Then
    assert_eq!(names, ["cli_a.jsonl"]);
    assert_eq!(
        opened.read_prefix(Path::new("cli_a.jsonl"), 64),
        Ok(b"inside session".to_vec())
    );
    assert_eq!(
        workspace.folder(Path::new("sessions")).map(|_| ()),
        Err(ConfinedSkip::Symlink)
    );
    Ok(())
}
