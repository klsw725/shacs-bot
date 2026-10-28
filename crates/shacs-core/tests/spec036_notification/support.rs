use shacs_core::runtime::*;
use shacs_core::tools::ToolRegistry;
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderRequest};
use shacs_session::durable_event::*;
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::durable_work::*;
use std::path::PathBuf;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const OWNER: &str = "email:owner@example.test";

pub struct Fixture {
    pub _root: tempfile::TempDir,
    pub workspace: PathBuf,
    pub data: PathBuf,
    pub work_id: String,
    pub bus: MessageBus,
}

impl Fixture {
    pub fn new(summary: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        let data = root.path().join("data");
        SessionManager::new(&workspace)?.save_with_fsync(&Session::new(OWNER))?;
        let work = create_entrusted_work(
            &workspace,
            EntrustedWorkCreateRequest {
                owner_session_id: OWNER.to_owned(),
                goal_text: "watch integer".to_owned(),
                turn_budget: 10,
                consent: EntrustedWorkConsentProof {
                    kind: EntrustedWorkConsentKind::ExplicitRequest,
                    evidence_ref: "user:explicit".to_owned(),
                    recorded_at: "100".to_owned(),
                },
                conditions: EntrustedWorkConditions {
                    instructions: "read only".to_owned(),
                    budget: None,
                    prior_decisions: Vec::new(),
                    remaining_questions: Vec::new(),
                },
                linked_targets: Vec::new(),
                observed_at: "100".to_owned(),
            },
        )?;
        let fixture = Self {
            _root: root,
            workspace,
            data,
            work_id: work.work.work_id,
            bus: MessageBus::new(),
        };
        configure_entrusted_observation(
            &fixture.workspace,
            ConfigureObservationRequest {
                work: fixture.request(),
                expected_revision: 1,
                changed_at: "200".to_owned(),
                plan: EntrustedObservationPlan {
                    source: ObservationSource::LocalFile {
                        relative_path: "source.json".to_owned(),
                    },
                    cadence_seconds: 5,
                    first_wake_at_ms: 1000,
                    comparison: Some(ObservationComparisonPolicy {
                        criterion: ObservationCriterion::IntegerAtMost { value: 100 },
                        max_age_seconds: 3600,
                        regular_summary: summary,
                    }),
                },
            },
        )?;
        fixture
            .dispatcher()?
            .enqueue_entrusted_observations(&fixture.runtime(0))?;
        Ok(fixture)
    }

    pub fn request(&self) -> EntrustedWorkReadRequest {
        EntrustedWorkReadRequest {
            owner_session_id: OWNER.to_owned(),
            requester_target: OWNER.to_owned(),
            work_id: self.work_id.clone(),
        }
    }

    pub fn runtime(&self, now_ms: u64) -> ObservationRuntime<'_> {
        ObservationRuntime {
            workspace: &self.workspace,
            data_dir: &self.data,
            now_ms,
        }
    }

    pub fn dispatcher(&self) -> Result<DurableWorkDispatcher, DurableDispatchError> {
        DurableWorkDispatcher::open(
            self.events(),
            self.data.join("runtime/work-payloads"),
            self.bus.clone(),
            "test-owner",
            120_000,
        )
    }

    pub fn events(&self) -> PathBuf {
        self.data.join("runtime/durable-events")
    }

    pub fn observe(&self, time: u64, value: i64) -> TestResult {
        std::fs::write(
            self.workspace.join("source.json"),
            serde_json::to_vec(
                &serde_json::json!({"schema":"integer_v1", "value":value, "as_of_ms":time}),
            )?,
        )?;
        self.dispatcher()?
            .process_entrusted_observations(&self.runtime(time), &Reader(self))?;
        Ok(())
    }

    pub fn route(&self) -> TestResult {
        self.dispatcher()?
            .enqueue_entrusted_notifications(&self.runtime(20_000))?;
        Ok(())
    }

    pub fn candidate(&self) -> TestResult {
        self.observe(1000, 120)?;
        self.observe(6000, 90)
    }

    pub fn read(&self) -> Result<EntrustedObservationSnapshot, ObservationError> {
        read_entrusted_observation(&self.workspace, &self.data, &self.request())
    }

    pub fn inbound(&self) -> Result<InboundMessage, Box<dyn std::error::Error>> {
        let state = evaluate_durable_recovery(self.events(), self.data.join("checkpoints"))
            .state
            .ok_or("replay")?;
        let admission = evaluate_durable_work_recovery_for_owner(
            &state.work,
            self.data.join("runtime/work-payloads"),
            20_000,
            Some("test-owner"),
        );
        self.dispatcher()?
            .dispatch_due(&state.work, &admission, 20_000)?;
        self.bus.consume_inbound().ok_or_else(|| "inbound".into())
    }

    pub fn consume(
        &self,
        message: InboundMessage,
    ) -> Result<AgentLoopTurnResult, Box<dyn std::error::Error>> {
        let tools = ToolRegistry::new();
        let mut config = AgentLoopConfig::new(&self.workspace, "unused");
        config.durable_event_root = Some(self.events());
        let mut runtime = AgentLoop::new(
            self.bus.clone(),
            SessionManager::new(&self.workspace)?,
            ContextBuilder::new(&self.workspace),
            &tools,
            &ForbiddenProvider,
            config,
        );
        Ok(runtime.process_message(message)?)
    }

    pub fn change(&self, action: Option<EntrustedWorkLifecycleAction>) -> TestResult {
        if let Some(action) = action {
            transition_entrusted_work(
                &self.workspace,
                EntrustedWorkLifecycleRequest {
                    owner_session_id: OWNER.to_owned(),
                    requester_target: OWNER.to_owned(),
                    work_id: self.work_id.clone(),
                    action,
                    observed_at: "7000".to_owned(),
                },
            )?;
        } else {
            let current = read_entrusted_work(&self.workspace, &self.request())?;
            revise_entrusted_work(
                &self.workspace,
                EntrustedWorkReviseRequest {
                    owner_session_id: OWNER.to_owned(),
                    requester_target: OWNER.to_owned(),
                    work_id: self.work_id.clone(),
                    expected_revision: current.work.current_revision,
                    conditions: EntrustedWorkConditions {
                        instructions: "changed condition".to_owned(),
                        budget: None,
                        prior_decisions: Vec::new(),
                        remaining_questions: Vec::new(),
                    },
                    linked_targets: None,
                    changed_at: "7000".to_owned(),
                },
            )?;
        }
        Ok(())
    }

    pub fn count(&self, kind: &str) -> Result<usize, Box<dyn std::error::Error>> {
        let mut count = 0;
        DurableEventStore::open(self.events())?.visit_from_sequence(0, |event| {
            if event.kind == kind {
                count += 1;
            }
        })?;
        Ok(count)
    }
}

struct Reader<'a>(&'a Fixture);
impl EntrustedObservationExecutor for Reader<'_> {
    fn prepare(&self, _: &ObservationWake) -> Result<String, ObservationBlock> {
        Ok("snapshot:test".to_owned())
    }
    fn observe(&self, wake: &ObservationWake) -> ObservationResult {
        let mut tools = ToolRegistry::new();
        tools.register(BoundedObservationTool::new(&self.0.workspace, wake));
        let result = RuntimeToolExecutor::new(&tools).execute_tool_calls(
            vec![RuntimeToolCall::new(
                "read",
                "read_file",
                serde_json::json!({"path":"source.json"}),
            )],
            &ToolExecutionContext {
                active_workspace: Some(self.0.workspace.clone()),
                ..ToolExecutionContext::default()
            },
        );
        serde_json::from_str(&result.messages[0].content).expect("bounded read")
    }
}

struct ForbiddenProvider;
impl ProviderClient for ForbiddenProvider {
    fn chat(&self, _: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        panic!("notification must not invoke provider")
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        _: &mut dyn FnMut(shacs_providers::ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        self.chat(request)
    }
}
