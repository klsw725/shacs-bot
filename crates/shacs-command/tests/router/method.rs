use shacs_command::{
    build_help_text, parse_loop_command_route, CommandId, CommandKind, CommandRouter, LoopCommand,
    MethodCommandArgs as Args, MethodName, PluginCommandRouter, PluginCommandSpec,
};

#[test]
fn method_variants_are_non_priority_builtin_commands() {
    // Given
    let cases = [
        ("/method", Args::Status),
        ("/method status", Args::Status),
        ("/method list", Args::List),
        ("/method auto", Args::Auto),
        ("/method history", Args::History),
        ("/method accept", Args::Accept),
        ("/method revise", Args::Revise),
        ("/method use general", Args::Use(MethodName::General)),
        (
            "/method use calculation",
            Args::Use(MethodName::ReproducibleCalculation),
        ),
        (
            "/method use reproducible_calculation",
            Args::Use(MethodName::ReproducibleCalculation),
        ),
        (
            "/method use report",
            Args::Use(MethodName::SourceGroundedReport),
        ),
        (
            "/method use source_grounded_report",
            Args::Use(MethodName::SourceGroundedReport),
        ),
        (
            "/method use image",
            Args::Use(MethodName::ImageGenerationReview),
        ),
        (
            "/method use image_generation_review",
            Args::Use(MethodName::ImageGenerationReview),
        ),
        (
            "/METHOD USE IMAGE",
            Args::Use(MethodName::ImageGenerationReview),
        ),
    ];
    for (input, expected) in cases {
        // When
        let route = parse_loop_command_route(input).expect("method route");
        // Then
        assert_eq!(route.command, LoopCommand::Method(expected));
        assert_eq!(route.parsed.id, CommandId::Method);
        assert_ne!(route.parsed.kind, CommandKind::Priority);
        assert!(!CommandRouter::builtin().is_priority(input));
    }
}

#[test]
fn invalid_method_input_stays_in_the_builtin_router() {
    // Given
    for input in [
        "/method use",
        "/method use composed",
        "/method use other",
        "/method auto extra",
        "/method accept now",
        "/method revise again",
        "/method unknown",
    ] {
        // When
        let route = parse_loop_command_route(input).expect("method route");
        // Then
        assert_eq!(route.command, LoopCommand::Method(Args::Invalid));
    }
}

#[test]
fn method_name_cannot_be_shadowed_by_a_plugin() {
    // Given
    let router = PluginCommandRouter::new([PluginCommandSpec::new("example", "method")]);
    // When
    let route = router.dispatch("/method use image");
    // Then
    assert!(route.is_none());
    assert!(build_help_text().contains(
        "/method [status|list|use <general|calculation|report|image>|auto|history|accept|revise]"
    ));
}
