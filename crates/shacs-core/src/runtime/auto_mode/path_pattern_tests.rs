use super::*;

fn bases() -> PathBases<'static> {
    PathBases {
        home: Path::new("/home/u"),
        config_folder: Path::new("/home/u/.shacs-bot"),
        workspace: Path::new("/ws"),
    }
}

fn check(rule: &str, path: &str, list: RuleList) -> Option<PathMatch> {
    PathPattern::parse(rule).matches(Path::new(path), bases(), list)
}

#[test]
fn anchors_follow_the_claude_code_prefixes() {
    assert!(check("//etc/hosts", "/etc/hosts", RuleList::Allow).is_some());
    assert!(check("~/.ssh/**", "/home/u/.ssh/id_rsa", RuleList::Allow).is_some());
    assert!(check(
        "/runtime/**",
        "/home/u/.shacs-bot/runtime/state",
        RuleList::Allow
    )
    .is_some());
    assert!(check("/runtime/**", "/runtime/state", RuleList::Allow).is_none());
    assert!(check("./src/*.rs", "/ws/src/main.rs", RuleList::Allow).is_some());
    assert!(check("src/*.rs", "/ws/src/a/main.rs", RuleList::Allow).is_none());
}

#[test]
fn bare_names_match_at_any_depth_and_single_folders_only_for_deny_or_ask() {
    assert_eq!(
        check(".env", "/ws/a/b/.env", RuleList::Allow),
        Some(PathMatch::Exact)
    );
    assert!(check(".env", "/other/.env", RuleList::DenyOrAsk).is_none());
    assert_eq!(
        check("secrets", "/ws/a/secrets/key", RuleList::DenyOrAsk),
        Some(PathMatch::ViaFolder)
    );
    assert!(check("secrets/**", "/ws/vendor/secrets/key", RuleList::Allow).is_none());
    assert!(check("secrets/**", "/ws/vendor/secrets/key", RuleList::DenyOrAsk).is_some());
    assert!(check(
        "src/components/**",
        "/ws/a/src/components/x",
        RuleList::DenyOrAsk
    )
    .is_none());
    assert!(check("**/src/**", "/ws/vendor/pkg/src/lib.js", RuleList::Allow).is_some());
}

#[test]
fn unusable_patterns_guard_the_literal_path() {
    // Given
    let rule = PathPattern::parse("data[1/report.txt");

    // When / Then
    assert!(!rule.usable());
    assert!(rule
        .matches(
            Path::new("/ws/data[1/report.txt"),
            bases(),
            RuleList::DenyOrAsk
        )
        .is_some());
    assert!(rule
        .matches(
            Path::new("/ws/data1/report.txt"),
            bases(),
            RuleList::DenyOrAsk
        )
        .is_none());
}

#[test]
fn negated_patterns_are_read_from_the_workspace() {
    // Given
    let rule = PathPattern::parse("!~/notes/public/**");

    // When / Then
    assert!(rule.negated);
    assert_eq!(rule.anchor, PathAnchor::Workspace);
    assert!(rule
        .matches(
            Path::new("/home/u/notes/public/a"),
            bases(),
            RuleList::DenyOrAsk
        )
        .is_none());
}

#[test]
fn anchored_deny_rules_also_match_the_real_location_of_a_linked_folder(
) -> Result<(), Box<dyn std::error::Error>> {
    // Given
    let root = tempfile::tempdir()?;
    let real = std::fs::canonicalize(root.path())?.join("real");
    std::fs::create_dir(&real)?;
    let link = real.with_file_name("link");
    std::os::unix::fs::symlink(&real, &link)?;
    let rule = PathPattern::parse(&format!("/{}/**", link.display()));

    // When
    let found = rule.matches(&real.join("secret.txt"), bases(), RuleList::DenyOrAsk);

    // Then
    assert!(found.is_some());
    assert!(rule
        .matches(&real.join("secret.txt"), bases(), RuleList::Allow)
        .is_none());
    Ok(())
}
