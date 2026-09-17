use shacs_projection::{
    Spec031Count, Spec031Freshness, Spec031ObservedAtUnixMs, Spec033AutomationJobStatus,
    Spec033DeliveryStatus, Spec033GoalBudgetFact, Spec033GoalFact, Spec033GoalStatus,
    Spec033GoalUsageSummary, Spec035AppProcessState, Spec035ChildTerminal, Spec035RecoveryStatus,
    Spec035TaskAction, Spec035TaskActionKind, Spec035TaskActionStatus, Spec035TaskDetail,
    Spec035TaskOwner, Spec035TaskRow, Spec035TaskRowInput, Spec035TaskState, Spec035TasksCoverage,
    Spec035TasksProjection, Spec035WorkflowPhase,
};
use shacs_tui::tasks_view::tasks_projection_lines;

type TestError = Box<dyn std::error::Error>;

#[test]
fn mixed_owner_projection_renders_canonical_tasks_fields() -> Result<(), TestError> {
    // Given
    let projection = mixed_projection()?;

    // When
    let rendered = tasks_projection_lines(&projection).join("\n");

    // Then
    assert!(rendered
        .contains("tasks coverage: goal=1 child=1 workflow=1 automation=1 app=1 recovery=1"));
    assert!(rendered
        .contains("task: owner=goal locator=goal-1 observed=10 freshness=current state=blocked"));
    assert!(rendered
        .contains("goal: id=goal-1 status=blocked stop=owner_blocked budget=3/8 remaining=5"));
    assert!(rendered.contains(
        "task: owner=workflow locator=workflow-1 observed=12 freshness=stale state=blocked"
    ));
    assert!(rendered.contains("workflow: phase=blocked completed_steps=2 active_children=1"));
    assert!(rendered.contains("action: stop status=requested"));
    assert!(rendered.contains("action: recover status=completed"));
    assert!(rendered.contains("task action: unavailable (unadvertised)"));
    Ok(())
}

#[test]
fn unavailable_owner_coverage_does_not_render_a_synthetic_row() -> Result<(), TestError> {
    // Given
    let projection =
        Spec035TasksProjection::try_new(Vec::new(), Spec035TasksCoverage::all_unavailable())?;

    // When
    let rendered = tasks_projection_lines(&projection);

    // Then
    assert_eq!(rendered.len(), 2);
    assert!(rendered[0].contains("goal=unavailable"));
    assert_eq!(rendered[1], "tasks: no owner rows");
    Ok(())
}

fn mixed_projection() -> Result<Spec035TasksProjection, TestError> {
    let goal = Spec033GoalFact {
        goal_id: "goal-1".to_owned(),
        session_id: "cli:direct".to_owned(),
        status: Spec033GoalStatus::Blocked,
        turn_budget: 8,
        turns_used: 3,
        last_verdict: None,
        blocked: true,
        stop_reason: Some("owner_blocked".to_owned()),
        budget: Spec033GoalBudgetFact {
            turn_budget: 8,
            turns_used: 3,
            remaining_turns: 5,
        },
        usage: Spec033GoalUsageSummary {
            turn_limit: 8,
            turns_used: 3,
            remaining_turns: 5,
            exhausted: false,
        },
        user_interrupted: false,
        latest_transition: None,
    };
    let rows = vec![
        row(
            "goal-1",
            RowFixture {
                observed_at: 10,
                freshness: Spec031Freshness::Current,
                state: Spec035TaskState::Blocked,
                detail: Spec035TaskDetail::Goal { fact: goal },
                action: None,
            },
        )?,
        row(
            "child-1",
            RowFixture {
                observed_at: 11,
                freshness: Spec031Freshness::Current,
                state: Spec035TaskState::Done,
                detail: Spec035TaskDetail::Child {
                    attempt: Spec031Count::new(2),
                    terminal: Some(Spec035ChildTerminal::Completed),
                },
                action: None,
            },
        )?,
        row(
            "workflow-1",
            RowFixture {
                observed_at: 12,
                freshness: Spec031Freshness::Stale,
                state: Spec035TaskState::Blocked,
                detail: Spec035TaskDetail::Workflow {
                    phase: Spec035WorkflowPhase::Blocked,
                    completed_steps: Spec031Count::new(2),
                    active_children: Spec031Count::new(1),
                },
                action: None,
            },
        )?,
        row(
            "work-1",
            RowFixture {
                observed_at: 13,
                freshness: Spec031Freshness::Current,
                state: Spec035TaskState::Pending,
                detail: Spec035TaskDetail::Automation {
                    job_status: Spec033AutomationJobStatus::Pending,
                    delivery_status: Spec033DeliveryStatus::NotRequested,
                },
                action: None,
            },
        )?,
        row(
            "formatter",
            RowFixture {
                observed_at: 14,
                freshness: Spec031Freshness::Current,
                state: Spec035TaskState::Requested,
                detail: Spec035TaskDetail::App {
                    process_state: Spec035AppProcessState::Stopping,
                },
                action: Some(Spec035TaskAction {
                    kind: Spec035TaskActionKind::Stop,
                    status: Spec035TaskActionStatus::Requested,
                }),
            },
        )?,
        row(
            "runtime:recovery",
            RowFixture {
                observed_at: 15,
                freshness: Spec031Freshness::Current,
                state: Spec035TaskState::Recovered,
                detail: Spec035TaskDetail::Recovery {
                    status: Spec035RecoveryStatus::Recovered,
                    issue_count: Spec031Count::new(0),
                },
                action: Some(Spec035TaskAction {
                    kind: Spec035TaskActionKind::Recover,
                    status: Spec035TaskActionStatus::Completed,
                }),
            },
        )?,
    ];
    Ok(Spec035TasksProjection::try_new(
        rows,
        Spec035TasksCoverage::all_available(1),
    )?)
}

struct RowFixture {
    observed_at: u64,
    freshness: Spec031Freshness,
    state: Spec035TaskState,
    detail: Spec035TaskDetail,
    action: Option<Spec035TaskAction>,
}

fn row(locator: &str, fixture: RowFixture) -> Result<Spec035TaskRow, TestError> {
    Ok(Spec035TaskRow::try_new(Spec035TaskRowInput {
        owner: Spec035TaskOwner::try_new(fixture.detail.owner_kind(), locator)?,
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(fixture.observed_at),
        freshness: fixture.freshness,
        state: fixture.state,
        detail: fixture.detail,
        action: fixture.action,
    })?)
}
