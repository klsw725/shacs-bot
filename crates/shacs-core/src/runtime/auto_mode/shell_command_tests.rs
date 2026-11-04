use super::*;

fn texts(command: &ShellCommand) -> Vec<String> {
    command
        .subcommands
        .iter()
        .map(|subcommand| subcommand.text(true))
        .collect()
}

#[test]
fn wrappers_are_removed_and_assignments_kept_apart() {
    // Given
    let command = "LANG=C timeout -s KILL 30 nice -n 5 npm test && command ls && xargs rm -f";

    // When
    let analysis = analyze(command, Path::new("/ws"), Path::new("/home/u"));

    // Then
    assert!(analysis.analyzable);
    assert_eq!(texts(&analysis), ["LANG=C npm test", "ls", "rm -f"]);
    assert_eq!(analysis.subcommands[0].text(false), "npm test");
}

#[test]
fn cd_adds_its_target_to_later_relative_paths_and_unresolved_targets_are_unknown() {
    // Given
    let command = "cd /data && cat auth.json; cd \"$X\" && rm -r build";

    // When
    let analysis = analyze(command, Path::new("/ws"), Path::new("/home/u"));

    // Then
    let cat = &analysis.subcommands[1];
    assert_eq!(
        cat.folders.known,
        [PathBuf::from("/ws"), PathBuf::from("/data")]
    );
    assert!(!cat.folders.unknown);
    assert!(analysis.subcommands[3].folders.unknown);
}

#[test]
fn shell_scripts_are_read_again_once_and_nested_shells_stay_unanalyzed() {
    // Given
    let single = "bash -lc 'rm -rf \"$1\"/*' _ build";
    let nested = "sh -c \"bash -c 'rm -rf ~'\"";

    // When
    let read_again = analyze(single, Path::new("/ws"), Path::new("/home/u"));
    let left_as_text = analyze(nested, Path::new("/ws"), Path::new("/home/u"));

    // Then
    let script = &read_again.subcommands[1];
    assert_eq!(script.text(false), "rm -rf $1/*");
    assert!(script.positional_bound);
    assert_eq!(left_as_text.unanalyzed_text, ["bash -c 'rm -rf ~'"]);
}

#[test]
fn here_docs_and_code_arguments_of_other_programs_are_searched_as_text() {
    // Given
    let command = "python3 - <<'EOF'\nopen('auth.json')\nEOF\nnode -e 'require(\"x\")'";

    // When
    let analysis = analyze(command, Path::new("/ws"), Path::new("/home/u"));

    // Then
    assert_eq!(
        analysis.text_regions,
        ["open('auth.json')\n", "require(\"x\")"]
    );
}

#[test]
fn reserved_words_and_unfollowed_commands_make_the_command_unanalyzable() {
    for command in ["for f in *; do rm $f; done", "eval \"$X\"", "source env.sh"] {
        // When
        let analysis = analyze(command, Path::new("/ws"), Path::new("/home/u"));

        // Then
        assert!(!analysis.analyzable, "{command}");
        assert_eq!(analysis.unanalyzed_text, [command]);
    }
}
