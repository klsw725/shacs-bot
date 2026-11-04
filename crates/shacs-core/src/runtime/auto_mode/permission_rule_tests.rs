use super::*;

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn rules_parse_with_tool_names_and_specifiers() {
    // Given
    let allow = strings(&[
        "exec(npm test)",
        "exec(*)",
        "read(./docs/**)",
        "web_fetch(domain:*.example.com)",
        "mcp_github",
        "mcp_github_get_*",
        "spawn",
    ]);
    let deny = strings(&["*", "mcp_*", "edit(//etc/**)", "read(!sample.env)"]);

    // When
    let rules = UserPermissionRules::parse(
        RawRuleLists {
            allow: &allow,
            ask: &[],
            deny: &deny,
        },
        &["github".to_owned()],
    );

    // Then
    assert!(rules.diagnostics.is_empty(), "{:?}", rules.diagnostics);
    assert!(matches!(rules.allow[0].specifier, RuleSpecifier::Exec(_)));
    assert_eq!(rules.allow[1].specifier, RuleSpecifier::Any);
    assert_eq!(
        rules.allow[4].tool,
        ToolSelector::McpServer("github".to_owned())
    );
    assert!(matches!(
        rules.allow[5].tool,
        ToolSelector::McpServerTools { .. }
    ));
    assert!(matches!(rules.deny[0].tool, ToolSelector::ToolGlob(_)));
    assert!(matches!(&rules.deny[3].specifier, RuleSpecifier::Path(path) if path.negated));
}

#[test]
fn unreadable_rules_are_left_out_with_a_reason_and_warnings_keep_the_rule() {
    // Given
    let allow = strings(&[
        "",
        "exec(npm test",
        "message(hello)",
        "web_fetch(example.com)",
        "read(!x)",
        "read(data[1)",
        "*",
        "exec(git * main)",
        "custom_tool",
    ]);

    // When
    let rules = UserPermissionRules::parse(
        RawRuleLists {
            allow: &allow,
            ask: &[],
            deny: &[],
        },
        &[],
    );

    // Then
    let problems = rules
        .diagnostics
        .iter()
        .map(|diagnostic| (diagnostic.index, diagnostic.problem))
        .collect::<Vec<_>>();
    assert_eq!(
        problems,
        [
            (0, RuleProblem::Empty),
            (1, RuleProblem::UnbalancedParentheses),
            (2, RuleProblem::UnsupportedSpecifier),
            (3, RuleProblem::UnsupportedSpecifier),
            (4, RuleProblem::NegatedAllow),
            (5, RuleProblem::UnusableAllowPath),
            (6, RuleProblem::UnanchoredAllowGlob),
            (7, RuleProblem::WildcardBeforeSubcommand),
            (8, RuleProblem::UnknownTool),
        ]
    );
    assert_eq!(
        rules.rule_texts(RuleEffect::Allow),
        ["exec(git * main)", "custom_tool"]
    );
}

#[test]
fn lists_and_rules_have_size_limits() {
    // Given
    let many = vec!["exec(ls)".to_owned(); MAX_RULES_PER_LIST + 1];
    let long = vec![format!("exec({})", "a".repeat(MAX_RULE_CHARS))];

    // When
    let rules = UserPermissionRules::parse(
        RawRuleLists {
            allow: &many,
            ask: &long,
            deny: &[],
        },
        &[],
    );

    // Then
    assert_eq!(rules.allow.len(), MAX_RULES_PER_LIST);
    assert!(rules.ask.is_empty());
    let problems = rules
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.problem)
        .collect::<Vec<_>>();
    assert_eq!(problems, [RuleProblem::TooMany, RuleProblem::TooLong]);
}
