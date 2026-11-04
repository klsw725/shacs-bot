use super::*;

fn removal(command: &str) -> Option<CriticalRemoval> {
    let home = Path::new("/home/u");
    let workspace = Path::new("/home/u/work/project");
    let data = Path::new("/home/u/.shacs-bot");
    let analysis = shell_command::analyze(command, workspace, home);
    critical_removal(
        &analysis,
        &CriticalFolders::new(home, workspace, data),
        home,
    )
}

fn folder(path: &str) -> Option<CriticalRemoval> {
    Some(CriticalRemoval::Folder(PathBuf::from(path)))
}

#[test]
fn critical_folders_and_globs_directly_under_them_are_found() {
    for (command, expected) in [
        ("rm -r --force /", folder("/")),
        ("sudo rm -rf /usr", folder("/usr")),
        ("rm -rf ~", folder("/home/u")),
        ("rmdir /home/u/work", folder("/home/u/work")),
        ("rm -rf .", folder("/home/u/work/project")),
        ("rm -rf /home/u/.shacs-bot", folder("/home/u/.shacs-bot")),
        ("rm -f /*", folder("/")),
        ("rm -rf ~/*", folder("/home/u")),
        ("/bin/rm -rf *", folder("/home/u/work/project")),
        ("rm -rf \"$DIR\"/*", folder("/")),
        ("rm -rf \"$TMPDIR/usr\"", folder("/usr")),
        ("rm -rf /Volumes/USB", folder("/Volumes/USB")),
        ("rm /mnt/c/Users/*", folder("/mnt/c/Users")),
    ] {
        assert_eq!(removal(command), expected, "{command}");
    }
}

#[test]
fn unverifiable_removals_are_reported_with_their_reason() {
    for command in [
        "rm -rf logs/*/*",
        "rm -rf logs/*/",
        "cd \"$X\" && rm -r build",
        "cd logs && rm -rf a/*",
        "bash -c 'rm -rf ~'",
        "for f in a; do rm -rf $f; done",
    ] {
        assert!(
            matches!(
                removal(command),
                Some(CriticalRemoval::Unverifiable(_)) | Some(CriticalRemoval::Folder(_))
            ),
            "{command}"
        );
    }
    assert!(matches!(
        removal("for f in a; do rm -rf $f; done"),
        Some(CriticalRemoval::Unverifiable(_))
    ));
}

#[test]
fn ordinary_removals_and_bound_positional_parameters_are_not_critical() {
    for command in [
        "rm -rf build",
        "rm -f \"$OUT/report.txt\"",
        "sh -c 'rm -rf \"$1\"/*' _ build",
        "rm -rf /home/u/work/project/target",
        "find . | xargs rm -rf",
        "echo rm -rf /",
    ] {
        assert_eq!(removal(command), None, "{command}");
    }
}
