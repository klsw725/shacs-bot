use shacs_cli::{parse_cli_args, CliCommand};

#[test]
fn built_cli_parses_each_local_improvement_action() -> Result<(), Box<dyn std::error::Error>> {
    // Given / When / Then
    for action in ["inspect", "apply", "verify", "candidate", "rollback"] {
        let command = parse_cli_args([
            "improve",
            action,
            "--root",
            "/tmp/improvement-root",
            "--proposal",
            "proposal:1",
        ])?;
        assert!(matches!(command, CliCommand::Improve(_)));
    }
    Ok(())
}
