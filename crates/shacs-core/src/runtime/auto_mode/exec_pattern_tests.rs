use super::*;

fn pattern(text: &str) -> ExecPattern {
    ExecPattern::parse(text).expect("test pattern should parse")
}

#[test]
fn wildcards_follow_the_claude_code_examples() {
    for (rule, matches, misses) in [
        (
            "npm run build",
            &["npm run build"][..],
            &["npm run build --watch"][..],
        ),
        (
            "npm run *",
            &["npm run build", "npm run test --watch", "npm run"],
            &["npm install"],
        ),
        (
            "git log * main",
            &["git log --oneline main", "git log -5 main"],
            &["git log main"],
        ),
        ("* --version", &["node --version"], &["node -v"]),
        ("ls *", &["ls -la", "ls"], &["lsof"]),
        ("ls*", &["ls -la", "lsof"], &[]),
        ("ls:*", &["ls -la", "ls"], &["lsof"]),
        ("* --help *", &["npm --help x"], &["npm --help"]),
    ] {
        let pattern = pattern(rule);
        for command in matches {
            assert!(pattern.matches(command), "{rule} should match {command}");
        }
        for command in misses {
            assert!(
                !pattern.matches(command),
                "{rule} should not match {command}"
            );
        }
    }
}

#[test]
fn broad_patterns_are_interpreters_wrappers_and_package_runners() {
    for rule in [
        "*",
        "* --version",
        "python3 *",
        "python3 -c *",
        "bash -lc *",
        "py*",
        "n*",
        "sudo -u root *",
        "env *",
        "npx *",
        "npm run *",
        "make *",
    ] {
        assert!(pattern(rule).is_broad(), "{rule} should be broad");
    }
    for rule in [
        "python3 script.py *",
        "python3 -m pip *",
        "npm test *",
        "make build *",
        "ls*",
        "git * main",
        "python3",
    ] {
        assert!(!pattern(rule).is_broad(), "{rule} should be narrow");
    }
}

#[test]
fn patterns_are_found_inside_unanalyzed_text_at_word_boundaries() {
    // Given
    let push = pattern("git push *");

    // When / Then
    assert!(push.appears_in("echo \"$(git push origin)\""));
    assert!(!push.appears_in("echo git pushed"));
    assert!(pattern("rm -rf *").appears_in("for f in *; do rm -rf $f; done"));
}

#[test]
fn wildcard_allows_skip_exec_wrappers_and_find_actions_and_warn_before_subcommands() {
    // Given
    let find = pattern("find *");

    // When / Then
    assert!(find.allow_excludes(&["find", ".", "-name", "x", "-delete"]));
    assert!(!find.allow_excludes(&["find", ".", "-name", "x"]));
    assert!(!pattern("find . -delete").allow_excludes(&["find", ".", "-delete"]));
    assert!(pattern("watch *").allow_excludes(&["watch", "ls"]));
    assert!(pattern("git * main").wildcard_before_subcommand());
    assert!(!pattern("git log *").wildcard_before_subcommand());
}
