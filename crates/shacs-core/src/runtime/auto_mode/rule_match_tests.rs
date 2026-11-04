use super::*;
use crate::runtime::auto_mode::permission_rule::RawRuleLists;
use std::path::Path;

fn rules(allow: &[&str], ask: &[&str], deny: &[&str]) -> UserPermissionRules {
    let owned = |values: &[&str]| {
        values
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
    };
    let (allow, ask, deny) = (owned(allow), owned(ask), owned(deny));
    UserPermissionRules::parse(
        RawRuleLists {
            allow: &allow,
            ask: &ask,
            deny: &deny,
        },
        &["github".to_owned()],
    )
}

fn bases() -> PathBases<'static> {
    PathBases {
        home: Path::new("/home/u"),
        config_folder: Path::new("/home/u/.shacs-bot"),
        workspace: Path::new("/ws"),
    }
}

fn pair(path: &str) -> PathPair {
    PathPair {
        requested: PathBuf::from(path),
        real: PathBuf::from(path),
    }
}

fn exec_subject(parts: &[&str]) -> RuleSubject<'static> {
    RuleSubject {
        tool_name: "exec",
        file: None,
        command: Some(CommandSubject {
            analyzable: true,
            parts: parts
                .iter()
                .map(|text| CommandPart {
                    text_for_allow: (*text).to_owned(),
                    text_for_deny: (*text).to_owned(),
                    words: text.split(' ').map(str::to_owned).collect(),
                    reads: Vec::new(),
                    writes: Vec::new(),
                })
                .collect(),
            text_regions: Vec::new(),
            unanalyzed_text: Vec::new(),
        }),
        host: None,
    }
}

const AUTO: AllowOptions = AllowOptions {
    suspend_broad: true,
    classify_all_shell: false,
};

#[test]
fn exec_deny_needs_one_part_and_allow_needs_every_part() {
    // Given
    let rules = rules(
        &["exec(npm test)", "exec(git status)"],
        &[],
        &["exec(git push *)"],
    );

    // When
    let both = match_rules(
        &rules,
        &exec_subject(&["git status", "npm test"]),
        bases(),
        AUTO,
    );
    let one = match_rules(
        &rules,
        &exec_subject(&["git status", "curl x"]),
        bases(),
        AUTO,
    );
    let push = match_rules(
        &rules,
        &exec_subject(&["npm test", "git push origin"]),
        bases(),
        AUTO,
    );

    // Then
    assert_eq!(both.allow.as_deref(), Some("exec(git status)"));
    assert_eq!(one.allow, None);
    assert_eq!(push.deny.as_deref(), Some("exec(git push *)"));
}

#[test]
fn auto_mode_suspends_broad_exec_and_spawn_allows_and_class_all_shell_stops_exec_allows() {
    // Given
    let rules = rules(&["exec(python3 *)", "exec", "spawn", "exec(ls)"], &[], &[]);
    let spawn = RuleSubject {
        tool_name: "spawn",
        file: None,
        command: None,
        host: None,
    };
    let other_modes = AllowOptions {
        suspend_broad: false,
        classify_all_shell: false,
    };

    // When / Then
    let python = exec_subject(&["python3 run.py"]);
    assert_eq!(match_rules(&rules, &python, bases(), AUTO).allow, None);
    assert!(match_rules(&rules, &python, bases(), other_modes)
        .allow
        .is_some());
    assert_eq!(match_rules(&rules, &spawn, bases(), AUTO).allow, None);
    let ls = exec_subject(&["ls"]);
    assert!(match_rules(&rules, &ls, bases(), AUTO).allow.is_some());
    let all_shell = AllowOptions {
        suspend_broad: true,
        classify_all_shell: true,
    };
    assert_eq!(match_rules(&rules, &ls, bases(), all_shell).allow, None);
}

#[test]
fn path_rules_cover_file_tools_and_command_paths_with_negation() {
    // Given
    let rules = rules(
        &["edit(src/**)"],
        &[],
        &[
            "read(*.env)",
            "read(!sample.env)",
            "read(secrets/**)",
            "read(!secrets/public/**)",
        ],
    );
    let read = |path: &str| RuleSubject {
        tool_name: "read_file",
        file: Some((FileAccess::Read, vec![pair(path)])),
        command: None,
        host: None,
    };
    let linked_write = RuleSubject {
        tool_name: "write_file",
        file: Some((
            FileAccess::Write,
            vec![PathPair {
                requested: PathBuf::from("/ws/src/a.rs"),
                real: PathBuf::from("/elsewhere/a.rs"),
            }],
        )),
        command: None,
        host: None,
    };

    // When / Then
    assert!(match_rules(&rules, &read("/ws/a/prod.env"), bases(), AUTO)
        .deny
        .is_some());
    assert!(match_rules(&rules, &read("/ws/sample.env"), bases(), AUTO)
        .deny
        .is_none());
    assert!(
        match_rules(&rules, &read("/ws/secrets/public/x"), bases(), AUTO)
            .deny
            .is_some()
    );
    assert!(match_rules(&rules, &linked_write, bases(), AUTO)
        .allow
        .is_none());
    let mut cat = exec_subject(&["cat prod.env"]);
    if let Some(command) = cat.command.as_mut() {
        command.parts[0].writes.push(pair("/ws/prod.env"));
    }
    assert!(match_rules(&rules, &cat, bases(), AUTO).deny.is_some());
}

#[test]
fn unanalyzed_text_is_searched_for_exec_patterns_and_denied_names() {
    // Given
    let rules = rules(&[], &["exec(git push *)"], &["read(.env)"]);
    let subject = RuleSubject {
        tool_name: "exec",
        file: None,
        command: Some(CommandSubject {
            analyzable: false,
            parts: Vec::new(),
            text_regions: vec!["for f in .env; do cat $f; done".to_owned()],
            unanalyzed_text: vec!["echo \"$(git push)\"".to_owned()],
        }),
        host: None,
    };

    // When
    let matches = match_rules(&rules, &subject, bases(), AUTO);

    // Then
    assert_eq!(matches.ask.as_deref(), Some("exec(git push *)"));
    assert_eq!(matches.deny.as_deref(), Some("read(.env)"));
    assert_eq!(matches.allow, None);
}

#[test]
fn domains_tool_globs_and_mcp_servers_select_calls() {
    // Given
    let rules = rules(
        &["web_fetch(domain:*.example.com)", "mcp_github"],
        &["mcp_*"],
        &[],
    );
    let fetch = RuleSubject {
        tool_name: "web_fetch",
        file: None,
        command: None,
        host: Some("api.example.com".to_owned()),
    };
    let mcp = RuleSubject {
        tool_name: "mcp_github_tool_get_issue",
        file: None,
        command: None,
        host: None,
    };

    // When / Then
    assert!(match_rules(&rules, &fetch, bases(), AUTO).allow.is_some());
    let mcp_matches = match_rules(&rules, &mcp, bases(), AUTO);
    assert!(mcp_matches.allow.is_some());
    assert_eq!(mcp_matches.ask.as_deref(), Some("mcp_*"));
}
