pub(super) fn required_command_ids() -> &'static [(&'static str, &'static str)] {
    &[
        ("fmt", "spec031-fmt"),
        ("clippy-workspace", "spec031-clippy-workspace"),
        ("test-workspace", "spec031-test-workspace"),
        ("test-release-runner", "spec031-test-release-runner"),
        ("test-lifecycle", "spec031-test-lifecycle"),
        ("test-projection-parity", "spec031-test-projection-parity"),
        ("test-surface-smoke", "spec031-test-surface-smoke"),
        ("test-failure-injection", "spec031-test-failure-injection"),
        ("build-cli", "spec031-build-cli"),
        ("build-tui", "spec031-build-tui"),
    ]
}
