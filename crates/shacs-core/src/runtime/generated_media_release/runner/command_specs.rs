pub(super) struct CommandSpec {
    pub kind: &'static str,
    pub package: &'static str,
    pub target: &'static str,
    pub tests_run: u64,
}

pub(super) const COMMAND_SPECS: [CommandSpec; 2] = [
    CommandSpec {
        kind: "schema-contract",
        package: "shacs-projection",
        target: "spec034_evidence_schema",
        tests_run: 7,
    },
    CommandSpec {
        kind: "sequential-integration",
        package: "shacs-core",
        target: "spec034_sequential_integration",
        tests_run: 2,
    },
];

impl CommandSpec {
    pub fn argv(&self) -> Vec<String> {
        [
            "cargo",
            "test",
            "--manifest-path",
            "crates/Cargo.toml",
            "--locked",
            "-p",
            self.package,
            "--test",
            self.target,
        ]
        .map(str::to_owned)
        .to_vec()
    }
}
