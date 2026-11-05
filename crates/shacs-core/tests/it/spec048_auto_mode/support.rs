use serde_json::Value;
use shacs_config::AutoApprovalConfig;
use shacs_core::runtime::{
    ceiling_for_origin, ApprovalActor, ApprovalCacheEntry, ApprovalDecision, ApprovalDecisionKind,
    ApprovalRequest, AutoEvaluatorVerdict, AutoEvaluatorVerdictKind, AutoModeEscalation,
    ContainerNetworkMode, ContainerRuntimeKind, ContainmentSnapshotRef, DockerContainmentSnapshot,
    EvaluatorConfidence, EvaluatorScopeMatch, PermissionEnvironment, PermissionEnvironmentInput,
    PermissionMode, PermissionModeSnapshot, PermissionRuleInput, ProcessAdapterKind, ProcessGate,
    ProcessSpawnReport, ProcessTerminalOutcome, RawRuleLists, RuntimeBoundaryOrigin,
    RuntimeInterrupt, RuntimeToolCall, RuntimeToolExecutionReport, RuntimeToolExecutor,
    SafetyCapability, SlotOverrides, ToolExecutionContext,
};
use shacs_core::tools::{
    EditFileTool, ExecConfig, ExecTool, GlobTool, GrepTool, JsonMap, ListDirTool, PathContext,
    ReadFileTool, SchemaFragment, Tool, ToolCallExecutionContext, ToolParameters, ToolRegistry,
    ToolResult, WriteFileTool,
};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub type TestResult = Result<(), Box<dyn Error>>;

pub const SCOPE: &str = "workspace:spec048-test";
pub const ALL_MODES: [PermissionMode; 6] = [
    PermissionMode::Plan,
    PermissionMode::Default,
    PermissionMode::AcceptEdits,
    PermissionMode::Auto,
    PermissionMode::DontAsk,
    PermissionMode::BypassPermissions,
];
pub const SECRET_TOKEN: &str = "spec048-secret-token-value";
/// What row 16 returns when the call had no classifier (Spec 048 PRD 020).
pub const NOT_CHECKED: &str = "Permission denied: auto mode could not check this call (auto mode's classifier is not available in this run), so it did not run.";
/// What row 16 returns for the test block verdict.
pub const BLOCKED: &str = "Permission denied by auto mode: [Data Exfiltration] sends files to an unnamed host. This call did not run.";
pub const PLUGIN_TOOL: &str = "pdf_merge";
pub const RECORDED_TOOLS: [&str; 8] = [
    "web_fetch",
    "web_search",
    "message",
    "cron",
    "spawn",
    "image_generate",
    "my",
    "mcp_github_get_issue",
];

/// A workspace, a data folder holding the auth store, a home and a media
/// folder for attachments, all under one temporary root.
pub struct Setup {
    _root: tempfile::TempDir,
    pub root: PathBuf,
    pub workspace: PathBuf,
    pub data: PathBuf,
    pub home: PathBuf,
    pub media: PathBuf,
}

impl Setup {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        Self::create(false)
    }

    pub fn workspace_is_data() -> Result<Self, Box<dyn Error>> {
        Self::create(true)
    }

    fn create(workspace_is_data: bool) -> Result<Self, Box<dyn Error>> {
        let root_dir = tempfile::tempdir()?;
        let root = fs::canonicalize(root_dir.path())?;
        let data = root.join("data");
        let workspace = if workspace_is_data {
            data.clone()
        } else {
            root.join("ws")
        };
        let home = root.join("home");
        let media = data.join("media");
        for folder in [&data, &workspace, &home, &media, &data.join("auth")] {
            fs::create_dir_all(folder)?;
        }
        fs::write(data.join("config.json"), "{}")?;
        fs::write(
            data.join("auth.json"),
            format!("{{\"token\": \"{SECRET_TOKEN}\"}}"),
        )?;
        fs::write(data.join("auth").join("oauth.json"), SECRET_TOKEN)?;
        fs::write(media.join("photo.txt"), "attachment text")?;
        fs::write(workspace.join("notes.md"), "workspace notes")?;
        fs::create_dir_all(workspace.join(".git"))?;
        fs::write(workspace.join(".git").join("config"), "[core]")?;
        Ok(Self {
            _root: root_dir,
            root,
            workspace,
            data,
            home,
            media,
        })
    }

    pub fn path_context(&self) -> PathContext {
        PathContext {
            workspace: Some(self.workspace.clone()),
            allowed_dir: Some(self.workspace.clone()),
            media_dir: Some(self.media.clone()),
            extra_allowed_dirs: Vec::new(),
        }
    }

    pub fn environment(&self, rules: &Rules) -> Arc<PermissionEnvironment> {
        self.environment_with(rules, true)
    }

    /// `env_file_current` false is a turn whose `.env.json` could not be
    /// read again.
    pub fn environment_with(
        &self,
        rules: &Rules,
        env_file_current: bool,
    ) -> Arc<PermissionEnvironment> {
        let env_file =
            shacs_config::EnvFile::load(&self.data.join("config.json")).unwrap_or_default();
        self.environment_from(rules, &env_file, env_file_current)
    }

    /// An environment with given `.env.json` values, as the CLI builds from
    /// the values it last read when the file cannot be read again.
    pub fn environment_from(
        &self,
        rules: &Rules,
        env_file: &shacs_config::EnvFile,
        env_file_current: bool,
    ) -> Arc<PermissionEnvironment> {
        Arc::new(PermissionEnvironment::new(PermissionEnvironmentInput {
            workspace: &self.workspace,
            data_folder: &self.data,
            config_file: &self.data.join("config.json"),
            home: &self.home,
            rules: RawRuleLists {
                allow: &rules.allow,
                ask: &rules.ask,
                deny: &rules.deny,
            },
            mcp_servers: &["github".to_owned()],
            extra_skill_roots: &[],
            extra_context_files: &[],
            configured_targets: &[],
            classify_all_shell: rules.classify_all_shell,
            slots: SlotOverrides::default(),
            classifier_model: None,
            env_file,
            env_file_current,
        }))
    }

    /// The production file tools, `exec` and recording stand-ins for the
    /// tools the table names by capability.
    pub fn registry(&self, exec: Exec) -> (ToolRegistry, Recorded) {
        let context = self.path_context();
        let recorded = Recorded::default();
        let mut registry = ToolRegistry::new();
        registry.register(ReadFileTool::new(context.clone()));
        registry.register(WriteFileTool::new(context.clone()));
        registry.register(EditFileTool::new(context.clone()));
        registry.register(ListDirTool::new(context.clone()));
        registry.register(GlobTool::new(context.clone()));
        registry.register(GrepTool::new(context.clone()));
        match exec {
            Exec::Gated => registry.register(GatedExec {
                guard: ExecTool::new(ExecConfig::new(context)),
                launched: recorded.clone(),
            }),
            Exec::GatedPermissive => {
                let mut config = ExecConfig::new(context);
                config.deny_patterns = Vec::new();
                config.sandbox = Some("bwrap".to_owned());
                registry.register(GatedExec {
                    guard: ExecTool::new(config),
                    launched: recorded.clone(),
                });
            }
        }
        for name in RECORDED_TOOLS {
            registry.register(RecordedTool {
                name,
                calls: recorded.clone(),
            });
        }
        registry.register(PluginStandIn {
            launched: recorded.clone(),
        });
        (registry, recorded)
    }

    pub fn context(&self, options: &Options) -> ToolExecutionContext {
        let containment = if options.contained {
            DockerContainmentSnapshot {
                contained: Some(true),
                runtime: ContainerRuntimeKind::Docker,
                root_user: Some(false),
                privileged: Some(false),
                host_mounts_summary: Vec::new(),
                network_mode: ContainerNetworkMode::Bridge,
                digest: Some("spec048-contained".to_owned()),
                summary: Some("spec048 test container".to_owned()),
            }
        } else {
            DockerContainmentSnapshot::unknown()
        };
        ToolExecutionContext {
            channel: "cli".to_owned(),
            chat_id: "direct".to_owned(),
            session_key: Some("cli:direct".to_owned()),
            containment_snapshot: options.contained.then(|| ContainmentSnapshotRef {
                contained: Some(true),
                backend: Some("official-container+test".to_owned()),
                digest: Some("spec048-contained".to_owned()),
                summary: Some("spec048 test container".to_owned()),
            }),
            permission_mode_snapshot: PermissionModeSnapshot {
                mode: options.mode,
                source: Some("test".to_owned()),
                scope_ref: Some(SCOPE.to_owned()),
            },
            permission_rule_input: PermissionRuleInput {
                containment,
                protected_targets: Vec::new(),
                proc_exec_summary: None,
            },
            permission_auto_approval: AutoApprovalConfig {
                enabled: options.mode == PermissionMode::Auto,
                allow_workspace_edits: options.allow_workspace_edits,
                ..AutoApprovalConfig::default()
            },
            permission_ceiling_snapshot: Some(ceiling_for_origin(
                options.mode,
                SafetyCapability::all().to_vec(),
                vec![SCOPE.to_owned()],
                RuntimeBoundaryOrigin::UserTurn,
            )),
            permission_evaluator: options.verdict.map(verdict),
            permission_auto_mode_escalation: options.escalation,
            permission_interactive: options.interactive,
            active_workspace: Some(self.workspace.clone()),
            permission_environment: Some(self.environment(&options.rules)),
            ..ToolExecutionContext::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exec {
    /// The production safety checks and process gate, with the launch only
    /// recorded.
    Gated,
    /// As `Gated`, with no deny patterns and a sandbox, so the guard leaves
    /// paths and `rm -rf` alone.
    GatedPermissive,
}

#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
    pub classify_all_shell: bool,
}

impl Rules {
    pub fn allow(rules: &[&str]) -> Self {
        Self {
            allow: owned(rules),
            ..Self::default()
        }
    }

    pub fn ask(rules: &[&str]) -> Self {
        Self {
            ask: owned(rules),
            ..Self::default()
        }
    }

    pub fn deny(rules: &[&str]) -> Self {
        Self {
            deny: owned(rules),
            ..Self::default()
        }
    }
}

fn owned(rules: &[&str]) -> Vec<String> {
    rules.iter().map(|rule| (*rule).to_owned()).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Block,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub mode: PermissionMode,
    pub interactive: bool,
    pub rules: Rules,
    pub contained: bool,
    pub allow_workspace_edits: bool,
    pub verdict: Option<Verdict>,
    pub escalation: Option<AutoModeEscalation>,
}

impl Options {
    pub fn auto(interactive: bool) -> Self {
        Self::mode(PermissionMode::Auto, interactive)
    }

    pub fn mode(mode: PermissionMode, interactive: bool) -> Self {
        Self {
            mode,
            interactive,
            rules: Rules::default(),
            contained: true,
            allow_workspace_edits: true,
            verdict: None,
            escalation: None,
        }
    }

    pub fn rules(mut self, rules: Rules) -> Self {
        self.rules = rules;
        self
    }

    pub fn verdict(mut self, verdict: Verdict) -> Self {
        self.verdict = Some(verdict);
        self
    }

    pub fn native(mut self) -> Self {
        self.contained = false;
        self
    }

    /// The block counts reached the limit in a row with this call, so row 16
    /// asks instead of refusing.
    pub fn escalated(mut self) -> Self {
        self.escalation = Some(AutoModeEscalation::Consecutive);
        self
    }
}

fn verdict(kind: Verdict) -> AutoEvaluatorVerdict {
    let (verdict, summary) = match kind {
        Verdict::Allow => (AutoEvaluatorVerdictKind::AllowCandidate, "allowed"),
        Verdict::Block => (
            AutoEvaluatorVerdictKind::DenyCandidate,
            "[Data Exfiltration] sends files to an unnamed host",
        ),
    };
    AutoEvaluatorVerdict {
        verdict,
        confidence: EvaluatorConfidence::High,
        scope_match: EvaluatorScopeMatch::Requested,
        risk_summary: summary.to_owned(),
        evidence_refs: vec!["auto-mode-classifier".to_owned()],
        expires_at_unix_ms: now_unix_ms() + 60_000,
        evaluator_ref: Some("auto-mode-classifier".to_owned()),
        prompt_injection_signals: Vec::new(),
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Ran(String),
    Asked(Box<Question>),
    Refused(String),
}

#[derive(Debug, Clone)]
pub struct Question {
    pub text: String,
    pub request: ApprovalRequest,
    pub call: RuntimeToolCall,
}

impl Question {
    pub fn once_only(&self) -> bool {
        self.request.allowed_decisions
            == [ApprovalDecisionKind::Approved, ApprovalDecisionKind::Denied]
    }

    /// The approval that a `1`/`approve` reply stores for this call.
    pub fn approval(&self, decision: ApprovalDecisionKind) -> ApprovalCacheEntry {
        ApprovalCacheEntry {
            request: self.request.clone(),
            decision: ApprovalDecision {
                approval_request_id: self.request.approval_request_id.clone(),
                action_digest: self.request.action_digest.clone(),
                snapshot_digest: self.request.snapshot_digest.clone(),
                decision,
                approved_scope: self.request.requested_scope.clone(),
                actor: ApprovalActor::LocalUser,
                decided_at_unix_ms: now_unix_ms(),
                consumed: false,
                policy_safety_snapshot_ref: self.request.policy_safety_snapshot_ref.clone(),
                secret_ref_evidence: self.request.secret_ref_evidence.clone(),
            },
        }
    }
}

impl Outcome {
    pub fn ran(&self) -> bool {
        matches!(self, Self::Ran(_))
    }

    pub fn question(&self) -> Option<&Question> {
        match self {
            Self::Asked(question) => Some(question),
            Self::Ran(_) | Self::Refused(_) => None,
        }
    }

    pub fn refusal(&self) -> Option<&str> {
        match self {
            Self::Refused(reason) => Some(reason),
            Self::Ran(_) | Self::Asked(_) => None,
        }
    }

    /// The call reached row 16 with no classifier and was refused there.
    pub fn not_checked(&self) -> bool {
        self.refusal()
            .is_some_and(|reason| reason.starts_with(NOT_CHECKED))
    }
}

pub fn call(tool: &str, arguments: Value) -> RuntimeToolCall {
    RuntimeToolCall::new("call_1", tool, arguments)
}

pub fn run(
    registry: &ToolRegistry,
    context: &ToolExecutionContext,
    call: RuntimeToolCall,
) -> Outcome {
    outcome(run_report(registry, context, call))
}

pub fn run_report(
    registry: &ToolRegistry,
    context: &ToolExecutionContext,
    call: RuntimeToolCall,
) -> RuntimeToolExecutionReport {
    RuntimeToolExecutor::new(registry).execute_tool_calls(vec![call], context)
}

pub fn outcome(report: RuntimeToolExecutionReport) -> Outcome {
    if let Some(RuntimeInterrupt::PermissionApproval {
        approval_request,
        tool_call,
        question,
        ..
    }) = report.interrupt
    {
        return Outcome::Asked(Box::new(Question {
            text: question,
            request: *approval_request,
            call: tool_call,
        }));
    }
    let content = report
        .messages
        .first()
        .map(|message| message.content.clone())
        .unwrap_or_default();
    if content.starts_with("Permission denied") || content.starts_with("Permission approval") {
        Outcome::Refused(content)
    } else {
        Outcome::Ran(content)
    }
}

/// Runs `call`, and when it is asked, answers once with `decision` and runs
/// it again as the agent loop does.
pub fn run_answered(
    registry: &ToolRegistry,
    context: &ToolExecutionContext,
    call: RuntimeToolCall,
    decision: ApprovalDecisionKind,
) -> Result<Outcome, Box<dyn Error>> {
    let first = run(registry, context, call);
    let question = first
        .question()
        .ok_or_else(|| format!("not asked: {first:?}"))?;
    let mut answered = context.clone();
    answered.permission_approval_cache = Some(question.approval(decision));
    Ok(run(registry, &answered, question.call.clone()))
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default)]
pub struct Recorded(Arc<Mutex<Vec<(String, Value)>>>);

impl Recorded {
    fn push(&self, name: &str, arguments: Value) {
        if let Ok(mut calls) = self.0.lock() {
            calls.push((name.to_owned(), arguments));
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|calls| calls.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    pub fn launched(&self) -> Vec<String> {
        self.0
            .lock()
            .map(|calls| {
                calls
                    .iter()
                    .filter(|(name, _)| name == "exec")
                    .filter_map(|(_, arguments)| arguments["command"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }
}

struct RecordedTool {
    name: &'static str,
    calls: Recorded,
}

impl Tool for RecordedTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Records the call."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.calls.push(self.name, Value::Object(params));
        format!("ran {}", self.name).into()
    }
}

/// A plugin tool (`pdf_merge`) that Tool Search defers, launched through the
/// process gate like a real plugin tool, with the launch only recorded.
struct PluginStandIn {
    launched: Recorded,
}

impl Tool for PluginStandIn {
    fn name(&self) -> &str {
        PLUGIN_TOOL
    }

    fn description(&self) -> &str {
        "Merges PDF files (recorded)."
    }

    fn parameters(&self) -> Value {
        ToolParameters::new().to_json_schema()
    }

    fn process_adapter_kind(&self) -> Option<ProcessAdapterKind> {
        Some(ProcessAdapterKind::PluginTool)
    }

    fn to_schema(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": PLUGIN_TOOL,
                "description": self.description(),
                "parameters": self.parameters(),
                "x-shacs-source-kind": "plugin_tool",
                "x-shacs-plugin-id": "pdf-tools",
            }
        })
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.execute_with_context(params, &ToolCallExecutionContext::default())
    }

    fn execute_with_context(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> ToolResult {
        let Some(input) = context.process_gate_input() else {
            return "Error: Process launch blocked by permission gate (no gate input)".into();
        };
        let arguments = Value::Object(params);
        match ProcessGate::new().evaluate_and_maybe_spawn(input.clone(), |_| {
            self.launched.push(PLUGIN_TOOL, arguments.clone());
            ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
        }) {
            Ok(receipt) if receipt.dispatch_count == 1 => "launched".into(),
            Ok(receipt) => format!(
                "Error: Process launch blocked by permission gate ({:?})",
                receipt.terminal_outcome
            )
            .into(),
            Err(error) => format!("Error: process gate failed: {error}").into(),
        }
    }
}

/// `exec` with the production safety checks and process gate, where the
/// launch the gate admits is recorded instead of started.
struct GatedExec {
    guard: ExecTool,
    launched: Recorded,
}

impl Tool for GatedExec {
    fn name(&self) -> &str {
        "exec"
    }

    fn description(&self) -> &str {
        "Runs a shell command (recorded)."
    }

    fn parameters(&self) -> Value {
        self.guard.parameters()
    }

    fn preflight_refusal(&self, params: &JsonMap) -> Option<String> {
        self.guard.preflight_refusal(params)
    }

    fn execute(&self, params: JsonMap) -> ToolResult {
        self.execute_with_context(params, &ToolCallExecutionContext::default())
    }

    fn execute_with_context(
        &self,
        params: JsonMap,
        context: &ToolCallExecutionContext,
    ) -> ToolResult {
        if let Some(refusal) = self.guard.preflight_refusal(&params) {
            return refusal.into();
        }
        let Some(input) = context.process_gate_input() else {
            return "Error: Process launch blocked by permission gate (no gate input)".into();
        };
        let arguments = Value::Object(params);
        let receipt = ProcessGate::new().evaluate_and_maybe_spawn(input.clone(), |_| {
            self.launched.push("exec", arguments.clone());
            ProcessSpawnReport::terminal(ProcessTerminalOutcome::Succeeded)
        });
        match receipt {
            Ok(receipt) if receipt.dispatch_count == 1 => "launched".into(),
            Ok(receipt) => format!(
                "Error: Process launch blocked by permission gate ({:?})",
                receipt.terminal_outcome
            )
            .into(),
            Err(error) => format!("Error: process gate failed: {error}").into(),
        }
    }
}
