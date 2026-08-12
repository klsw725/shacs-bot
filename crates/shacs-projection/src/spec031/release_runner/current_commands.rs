use super::external_owner_commands::exact_owner_commands;
use super::model::{Spec031ReleaseCommandSpec, Spec031ReleaseGateKind, Spec031ReleaseRunnerConfig};

pub fn required_worktree_commands(
    config: &Spec031ReleaseRunnerConfig,
) -> Vec<Spec031ReleaseCommandSpec> {
    let mut commands = vec![
        Spec031ReleaseCommandSpec {
            id: "spec031-fmt".to_owned(),
            gate: Spec031ReleaseGateKind::SurfaceSmoke,
            package: None,
            filter: Some("fmt --check".to_owned()),
            argv: vec![
                "cargo".to_owned(),
                "fmt".to_owned(),
                "--manifest-path".to_owned(),
                "crates/Cargo.toml".to_owned(),
                "--all".to_owned(),
                "--".to_owned(),
                "--check".to_owned(),
            ],
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        Spec031ReleaseCommandSpec {
            id: "spec031-clippy-workspace".to_owned(),
            gate: Spec031ReleaseGateKind::FullCargoGate,
            package: None,
            filter: Some("workspace all-targets warnings-deny".to_owned()),
            argv: vec![
                "cargo".to_owned(),
                "clippy".to_owned(),
                "--manifest-path".to_owned(),
                "crates/Cargo.toml".to_owned(),
                "--locked".to_owned(),
                "--workspace".to_owned(),
                "--all-targets".to_owned(),
                "--".to_owned(),
                "-D".to_owned(),
                "warnings".to_owned(),
            ],
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        Spec031ReleaseCommandSpec {
            id: "spec031-test-workspace".to_owned(),
            gate: Spec031ReleaseGateKind::FullCargoGate,
            package: None,
            filter: Some("workspace tests".to_owned()),
            argv: vec![
                "cargo".to_owned(),
                "test".to_owned(),
                "--manifest-path".to_owned(),
                "crates/Cargo.toml".to_owned(),
                "--locked".to_owned(),
                "--workspace".to_owned(),
            ],
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        focused_test(
            config,
            "spec031-test-release-runner",
            "shacs-projection",
            "spec031 release runner artifact tests",
            &[
                "--test",
                "spec031_release_runner",
                "--test",
                "spec031_release_runner_exploits",
            ],
            Spec031ReleaseGateKind::FocusedCargoTest,
        ),
        focused_test(
            config,
            "spec031-test-lifecycle",
            "shacs-config",
            "Spec031 config migration and layout",
            &[
                "--test",
                "spec031_migration_transaction",
                "--test",
                "spec031_runtime_layout",
                "--test",
                "spec031_schema_profiles",
            ],
            Spec031ReleaseGateKind::FocusedCargoTest,
        ),
        focused_test(
            config,
            "spec031-test-projection-parity",
            "shacs-core",
            "Spec031 immutable snapshot and explicit context",
            &[
                "--test",
                "spec031_execution_snapshot",
                "--test",
                "spec031_context_projection",
            ],
            Spec031ReleaseGateKind::FocusedCargoTest,
        ),
        focused_test(
            config,
            "spec031-test-surface-smoke",
            "shacs-core",
            "Spec031 activation and sequential integration",
            &[
                "--test",
                "spec031_activation_store",
                "--test",
                "spec031_activation_execution",
                "--test",
                "spec031_sequential_integration",
            ],
            Spec031ReleaseGateKind::SurfaceSmoke,
        ),
        focused_test(
            config,
            "spec031-test-failure-injection",
            "shacs-cli",
            "Spec031 management CLI and runtime admission",
            &[
                "--test",
                "spec031_management_cli",
                "--test",
                "spec031_runtime_layout_admission",
            ],
            Spec031ReleaseGateKind::FailureInjection,
        ),
        Spec031ReleaseCommandSpec {
            id: "spec031-build-cli".to_owned(),
            gate: Spec031ReleaseGateKind::SurfaceSmoke,
            package: Some("shacs-cli".to_owned()),
            filter: Some("build shacs-cli".to_owned()),
            argv: vec![
                "cargo".to_owned(),
                "build".to_owned(),
                "--manifest-path".to_owned(),
                "crates/Cargo.toml".to_owned(),
                "--locked".to_owned(),
                "-p".to_owned(),
                "shacs-cli".to_owned(),
            ],
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
        Spec031ReleaseCommandSpec {
            id: "spec031-build-tui".to_owned(),
            gate: Spec031ReleaseGateKind::SurfaceSmoke,
            package: Some("shacs-tui".to_owned()),
            filter: Some("build shacs-tui".to_owned()),
            argv: vec![
                "cargo".to_owned(),
                "build".to_owned(),
                "--manifest-path".to_owned(),
                "crates/Cargo.toml".to_owned(),
                "--locked".to_owned(),
                "-p".to_owned(),
                "shacs-tui".to_owned(),
            ],
            cwd: config.repo_root.clone(),
            timeout: config.command_timeout,
        },
    ];
    commands.extend(exact_owner_commands(config));
    commands
}

pub(super) fn focused_test(
    config: &Spec031ReleaseRunnerConfig,
    id: &str,
    package: &str,
    filter: &str,
    extra_args: &[&str],
    gate: Spec031ReleaseGateKind,
) -> Spec031ReleaseCommandSpec {
    let mut argv = vec![
        "cargo".to_owned(),
        "test".to_owned(),
        "--manifest-path".to_owned(),
        "crates/Cargo.toml".to_owned(),
        "--locked".to_owned(),
        "-p".to_owned(),
        package.to_owned(),
    ];
    argv.extend(extra_args.iter().map(|arg| (*arg).to_owned()));
    Spec031ReleaseCommandSpec {
        id: id.to_owned(),
        gate,
        package: Some(package.to_owned()),
        filter: Some(filter.to_owned()),
        argv,
        cwd: config.repo_root.clone(),
        timeout: config.command_timeout,
    }
}
