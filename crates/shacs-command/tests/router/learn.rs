use shacs_command::{
    build_help_text, parse_loop_command_route, CommandId, CommandKind, LearnCommandArgs as Args,
    LoopCommand, MethodPart, PluginCommandRouter, PluginCommandSpec,
};

#[test]
fn learn_variants_route_as_non_priority_builtin_commands() {
    // Given
    let cases = [
        ("/learn", Args::Propose),
        ("/learn list", Args::List),
        ("/learn show method-1", Args::Show("method-1".to_owned())),
        ("/LEARN SHOW Method-2", Args::Show("method-2".to_owned())),
        ("/learn show", Args::Invalid),
        ("/learn show method-1 extra", Args::Invalid),
        ("/learn now", Args::Invalid),
        (
            "/learn confirm method-1",
            Args::Review("method-1".to_owned()),
        ),
        (
            "/learn confirm Method-1 V2 3F2A9C1B",
            Args::Confirm {
                method_id: "method-1".to_owned(),
                version: 2,
                code: "3f2a9c1b".to_owned(),
            },
        ),
        ("/learn confirm method-1 2 3f2a9c1b", Args::Invalid),
        (
            "/learn reject method-3 v1",
            Args::Reject {
                method_id: "method-3".to_owned(),
                version: 1,
            },
        ),
        ("/learn reject method-3", Args::Invalid),
        (
            "/learn verify method-2",
            Args::Verify("method-2".to_owned()),
        ),
        (
            "/learn MATCH Summarize the Release Notes",
            Args::Match("Summarize the Release Notes".to_owned()),
        ),
        ("/learn match", Args::Invalid),
        (
            "/learn show method-1 v2",
            Args::ShowVersion {
                method_id: "method-1".to_owned(),
                version: 2,
            },
        ),
        (
            "/learn edit Method-1 V2 step 3 Check the Date first",
            Args::Edit {
                method_id: "method-1".to_owned(),
                version: 2,
                part: MethodPart::Step(3),
                text: Some("Check the Date first".to_owned()),
            },
        ),
        (
            "/learn edit method-1 v1 applies Summaries of Notes",
            Args::Edit {
                method_id: "method-1".to_owned(),
                version: 1,
                part: MethodPart::AppliesTo,
                text: Some("Summaries of Notes".to_owned()),
            },
        ),
        (
            "/learn edit method-1 v1 remove premise 2",
            Args::Edit {
                method_id: "method-1".to_owned(),
                version: 1,
                part: MethodPart::Premise(2),
                text: None,
            },
        ),
        ("/learn edit method-1 v1 step 0 Read it", Args::Invalid),
        ("/learn edit method-1 v1 step 2", Args::Invalid),
        ("/learn edit method-1 v1 remove step 2 now", Args::Invalid),
        (
            "/learn deactivate method-1 v2",
            Args::Deactivate {
                method_id: "method-1".to_owned(),
                version: 2,
            },
        ),
        (
            "/learn restore method-1 v1",
            Args::Restore {
                method_id: "method-1".to_owned(),
                version: 1,
            },
        ),
        ("/learn restore method-1", Args::Invalid),
    ];
    for (input, expected) in cases {
        // When
        let route = parse_loop_command_route(input).expect("learn route");
        // Then
        assert_eq!(route.command, LoopCommand::Learn(expected), "{input}");
        assert_eq!(route.parsed.id, CommandId::Learn);
        assert_ne!(route.parsed.kind, CommandKind::Priority);
    }
}

#[test]
fn learn_name_cannot_be_shadowed_by_a_plugin() {
    // Given
    let router = PluginCommandRouter::new([PluginCommandSpec::new("example", "learn")]);
    // When
    let route = router.dispatch("/learn list");
    // Then
    assert!(route.is_none());
    assert!(build_help_text().contains(
        "/learn [list|show <id> [v<N>]|confirm <id>|reject <id> v<N>|verify <id>|match <request>|edit <id> v<N> <part> <text>|deactivate <id> v<N>|restore <id> v<N>]"
    ));
}
