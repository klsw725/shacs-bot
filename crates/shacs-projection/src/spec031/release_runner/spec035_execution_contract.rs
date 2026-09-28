use super::spec035_execution_model::CommandKind;

pub(super) const OWNERS: [(&str, &[&str]); 6] = [
    ("029", &["durable-work-child", "recovery-delivery"]),
    (
        "030",
        &[
            "resource-disclosure",
            "runtime-controls",
            "credential-status",
        ],
    ),
    ("031", &["config-auth-declarations", "execution-snapshot"]),
    ("032", &["registry-intent", "app-lifecycle"]),
    ("033", &["automation-delivery", "goal-accounting"]),
    ("034", &["stored-video", "generated-media-analyzer"]),
];

pub(super) const GATES: [(&str, CommandKind); 19] = [
    ("focused", CommandKind::FocusedTest),
    ("workspace", CommandKind::WorkspaceTest),
    ("format", CommandKind::Format),
    ("lint", CommandKind::Lint),
    ("build-cli", CommandKind::Build),
    ("build-tui", CommandKind::Build),
    ("surface-cli", CommandKind::Surface),
    ("surface-tui", CommandKind::Surface),
    ("surface-repl", CommandKind::Surface),
    ("surface-onboard", CommandKind::Surface),
    ("surface-api", CommandKind::Surface),
    ("surface-websocket", CommandKind::Surface),
    ("surface-channel", CommandKind::Surface),
    ("lifecycle", CommandKind::Surface),
    ("failure-injection", CommandKind::FocusedTest),
    ("visual", CommandKind::Review),
    ("correction", CommandKind::Review),
    ("redaction", CommandKind::Review),
    ("documentation", CommandKind::Review),
];

pub(super) const INCIDENTS: [&str; 3] = [
    "default-config-rewrite",
    "missing-spec034-fixture",
    "ambiguous-spec034-temp-roots",
];
