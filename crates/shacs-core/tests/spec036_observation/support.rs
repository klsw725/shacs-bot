use shacs_core::runtime::*;
use shacs_core::tools::ToolRegistry;
use shacs_session::{Session, SessionManager};
use std::cell::Cell;
use std::path::{Path, PathBuf};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;

pub struct Fixture {
    pub root: tempfile::TempDir,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub work_id: String,
    pub bus: MessageBus,
}

impl Fixture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let data_dir = root.path().join("data");
        let mut manager = SessionManager::new(&workspace)?;
        manager.save_with_fsync(&Session::new("cli:owner"))?;
        let created = create_entrusted_work(
            &workspace,
            EntrustedWorkCreateRequest {
                owner_session_id: "cli:owner".to_owned(),
                goal_text: "watch local evidence".to_owned(),
                turn_budget: 10,
                consent: EntrustedWorkConsentProof {
                    kind: EntrustedWorkConsentKind::ExplicitRequest,
                    evidence_ref: "user:explicit".to_owned(),
                    recorded_at: "100".to_owned(),
                },
                conditions: conditions(),
                linked_targets: Vec::new(),
                observed_at: "100".to_owned(),
            },
        )?;
        std::fs::write(workspace.join("source.txt"), b"controlled evidence A")?;
        let fixture = Self {
            root,
            workspace,
            data_dir,
            work_id: created.work.work_id,
            bus: MessageBus::new(),
        };
        configure_entrusted_observation(
            &fixture.workspace,
            fixture.configuration(1, "source.txt"),
        )?;
        Ok(fixture)
    }

    pub fn request(&self) -> EntrustedWorkReadRequest {
        EntrustedWorkReadRequest {
            owner_session_id: "cli:owner".to_owned(),
            requester_target: "cli:owner".to_owned(),
            work_id: self.work_id.clone(),
        }
    }

    pub fn configuration(&self, expected_revision: u64, path: &str) -> ConfigureObservationRequest {
        ConfigureObservationRequest {
            work: self.request(),
            expected_revision,
            plan: EntrustedObservationPlan {
                source: ObservationSource::LocalFile {
                    relative_path: path.to_owned(),
                },
                cadence_seconds: 5,
                first_wake_at_ms: 1000,
                comparison: None,
            },
            changed_at: "200".to_owned(),
        }
    }

    pub fn dispatcher(&self) -> Result<DurableWorkDispatcher, ObservationError> {
        Ok(DurableWorkDispatcher::open(
            self.data_dir.join("runtime/durable-events"),
            self.data_dir.join("runtime/work-payloads"),
            self.bus.clone(),
            "owner",
            120_000,
        )?)
    }

    pub fn runtime(&self, now_ms: u64) -> ObservationRuntime<'_> {
        ObservationRuntime {
            workspace: &self.workspace,
            data_dir: &self.data_dir,
            now_ms,
        }
    }

    pub fn read(&self) -> Result<EntrustedObservationSnapshot, ObservationError> {
        read_entrusted_observation(&self.workspace, &self.data_dir, &self.request())
    }

    pub fn revise(&self) -> Result<(), EntrustedWorkError> {
        revise_entrusted_work(
            &self.workspace,
            EntrustedWorkReviseRequest {
                owner_session_id: "cli:owner".to_owned(),
                requester_target: "cli:owner".to_owned(),
                work_id: self.work_id.clone(),
                expected_revision: 2,
                conditions: EntrustedWorkConditions {
                    instructions: "new conditions".to_owned(),
                    ..conditions()
                },
                linked_targets: None,
                changed_at: "300".to_owned(),
            },
        )?;
        Ok(())
    }

    pub fn transition(
        &self,
        action: EntrustedWorkLifecycleAction,
    ) -> Result<(), EntrustedWorkError> {
        transition_entrusted_work(
            &self.workspace,
            EntrustedWorkLifecycleRequest {
                owner_session_id: "cli:owner".to_owned(),
                requester_target: "cli:owner".to_owned(),
                work_id: self.work_id.clone(),
                action,
                observed_at: "300".to_owned(),
            },
        )?;
        Ok(())
    }
}

fn conditions() -> EntrustedWorkConditions {
    EntrustedWorkConditions {
        instructions: "record local evidence".to_owned(),
        budget: None,
        prior_decisions: Vec::new(),
        remaining_questions: Vec::new(),
    }
}

pub struct Reader<'a> {
    pub workspace: &'a Path,
    pub reads: Cell<u32>,
    pub before_read: Option<Box<dyn Fn() + 'a>>,
    pub after_read: Option<Box<dyn Fn() + 'a>>,
    pub gate: Result<String, ObservationBlock>,
}

impl<'a> Reader<'a> {
    pub fn new(workspace: &'a Path) -> Self {
        Self {
            workspace,
            reads: Cell::new(0),
            before_read: None,
            after_read: None,
            gate: Ok("snapshot:test".to_owned()),
        }
    }
}

impl EntrustedObservationExecutor for Reader<'_> {
    fn prepare(&self, _wake: &ObservationWake) -> Result<String, ObservationBlock> {
        if let Some(action) = &self.before_read {
            action();
        }
        self.gate.clone()
    }
    fn observe(&self, wake: &ObservationWake) -> ObservationResult {
        self.reads.set(self.reads.get() + 1);
        let mut registry = ToolRegistry::new();
        registry.register(BoundedObservationTool::new(self.workspace, wake));
        let ObservationSource::LocalFile { relative_path } = &wake.plan.source;
        let report = RuntimeToolExecutor::new(&registry).execute_tool_calls(
            vec![RuntimeToolCall::new(
                wake.durable_work_id(),
                "read_file",
                serde_json::json!({"path": relative_path}),
            )],
            &ToolExecutionContext {
                active_workspace: Some(self.workspace.to_path_buf()),
                ..ToolExecutionContext::default()
            },
        );
        let result =
            serde_json::from_str(&report.messages[0].content).expect("typed bounded read result");
        if let Some(action) = &self.after_read {
            action();
        }
        result
    }
}
