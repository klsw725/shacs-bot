use serde_json::{json, Value};
use shacs_projection::*;
use std::{error::Error, fs};

fn owner(kind: Spec035TaskOwnerKind, locator: &str) -> Result<Spec035TaskOwner, Box<dyn Error>> {
    Ok(Spec035TaskOwner::try_new(kind, locator)?)
}

fn goal_fact() -> Spec033GoalFact {
    Spec033GoalFact {
        goal_id: "goal-1".to_owned(),
        session_id: "cli:direct".to_owned(),
        status: Spec033GoalStatus::Blocked,
        turn_budget: 8,
        turns_used: 3,
        last_verdict: Some(Spec033GoalVerdict::Blocked),
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
    }
}

fn row(
    owner_kind: Spec035TaskOwnerKind,
    locator: &str,
    state: Spec035TaskState,
    detail: Spec035TaskDetail,
) -> Result<Spec035TaskRow, Box<dyn Error>> {
    Ok(Spec035TaskRow::try_new(Spec035TaskRowInput {
        owner: owner(owner_kind, locator)?,
        observed_at_unix_ms: Spec031ObservedAtUnixMs::new(35),
        freshness: Spec031Freshness::Current,
        state,
        detail,
        action: None,
    })?)
}

fn missing_child(
    freshness: Spec031Freshness,
    state: Spec035TaskState,
) -> Result<Result<Spec035TaskRow, Spec035TasksValidationError>, Box<dyn Error>> {
    let mut input = row(
        Spec035TaskOwnerKind::Child,
        "owner:child:missing",
        Spec035TaskState::Running,
        Spec035TaskDetail::Child {
            attempt: Spec031Count::new(0),
            terminal: Some(Spec035ChildTerminal::Completed),
        },
    )?
    .to_input();
    input.freshness = freshness;
    input.state = state;
    Ok(Spec035TaskRow::try_new(input))
}

fn mixed_projection() -> Result<Spec035TasksProjection, Box<dyn Error>> {
    let rows = vec![
        row(
            Spec035TaskOwnerKind::Recovery,
            "owner:recovery:checkpoint-1",
            Spec035TaskState::Recovered,
            Spec035TaskDetail::Recovery {
                status: Spec035RecoveryStatus::Recovered,
                issue_count: Spec031Count::new(1),
            },
        )?,
        row(
            Spec035TaskOwnerKind::App,
            "owner:app:formatter",
            Spec035TaskState::Running,
            Spec035TaskDetail::App {
                process_state: Spec035AppProcessState::Running,
            },
        )?,
        Spec035TaskRow::try_new({
            let mut input = row(
                Spec035TaskOwnerKind::Workflow,
                "owner:workflow:release",
                Spec035TaskState::Blocked,
                Spec035TaskDetail::Workflow {
                    phase: Spec035WorkflowPhase::Blocked,
                    completed_steps: Spec031Count::new(2),
                    active_children: Spec031Count::new(1),
                },
            )?
            .to_input();
            input.freshness = Spec031Freshness::Stale;
            input
        })?,
        row(
            Spec035TaskOwnerKind::Child,
            "owner:child:reviewer",
            Spec035TaskState::Done,
            Spec035TaskDetail::Child {
                attempt: Spec031Count::new(1),
                terminal: Some(Spec035ChildTerminal::Completed),
            },
        )?,
        Spec035TaskRow::try_new({
            let mut input = row(
                Spec035TaskOwnerKind::Automation,
                "owner:automation:work-1",
                Spec035TaskState::Requested,
                Spec035TaskDetail::Automation {
                    job_status: Spec033AutomationJobStatus::Pending,
                    delivery_status: Spec033DeliveryStatus::NotRequested,
                },
            )?
            .to_input();
            input.action = Some(Spec035TaskAction {
                kind: Spec035TaskActionKind::Stop,
                status: Spec035TaskActionStatus::Requested,
            });
            input
        })?,
        row(
            Spec035TaskOwnerKind::Goal,
            "owner:goal:goal-1",
            Spec035TaskState::Blocked,
            Spec035TaskDetail::Goal { fact: goal_fact() },
        )?,
    ];
    Spec035TasksProjection::try_new(rows, Spec035TasksCoverage::all_available(1))
        .map_err(Into::into)
}

#[test]
fn spec035_tasks_mixed_rows_serialize_strictly_and_deterministically() -> Result<(), Box<dyn Error>>
{
    // Given
    let projection = mixed_projection()?;

    // When
    let first = serde_json::to_string(&projection)?;
    let second = serde_json::to_string(&projection)?;
    let round_trip = Spec035TasksProjection::parse_json(&first)?;

    // Then
    assert_eq!(first, second);
    assert_eq!(round_trip, projection);
    assert_eq!(projection.rows().len(), Spec035TaskOwnerKind::ALL.len());
    Ok(())
}

#[test]
fn spec035_tasks_rejects_unknown_schema_fields_and_malformed_locators() -> Result<(), Box<dyn Error>>
{
    // Given
    let value = serde_json::to_value(mixed_projection()?)?;
    let mut wrong_schema = value.clone();
    wrong_schema["schema_version"] = json!(2);
    let mut unknown_field = value.clone();
    unknown_field["raw_process_output"] = json!("secret-token");
    let mut missing_locator = value.clone();
    missing_locator["rows"][0]["owner"]
        .as_object_mut()
        .ok_or("owner fixture")?
        .remove("locator");
    let mut unsafe_locator = value;
    unsafe_locator["rows"][0]["owner"]["locator"] = json!("/Users/private/task.json");

    // When / Then
    assert!(Spec035TasksProjection::parse_json(&serde_json::to_string(&wrong_schema)?).is_err());
    assert!(Spec035TasksProjection::parse_json(&serde_json::to_string(&unknown_field)?).is_err());
    assert!(Spec035TasksProjection::parse_json(&serde_json::to_string(&missing_locator)?).is_err());
    assert!(Spec035TasksProjection::parse_json(&serde_json::to_string(&unsafe_locator)?).is_err());
    Ok(())
}

#[test]
fn spec035_tasks_enforces_row_bounds_order_and_coverage() -> Result<(), Box<dyn Error>> {
    // Given
    let template = row(
        Spec035TaskOwnerKind::Child,
        "owner:child:template",
        Spec035TaskState::Running,
        Spec035TaskDetail::Child {
            attempt: Spec031Count::new(1),
            terminal: None,
        },
    )?;
    let rows = (0..=SPEC035_TASKS_ROWS_MAX)
        .map(|index| {
            let locator = format!("owner:child:item-{index}");
            let mut input = template.to_input();
            input.owner = owner(Spec035TaskOwnerKind::Child, &locator).expect("bounded locator");
            Spec035TaskRow::try_new(input).expect("valid row")
        })
        .collect();

    // When
    let result = Spec035TasksProjection::try_new(rows, Spec035TasksCoverage::all_available(0));

    // Then
    assert_eq!(
        result.expect_err("row bound must fail").kind(),
        Spec035TasksValidationErrorKind::TooManyRows
    );
    let mismatch =
        Spec035TasksProjection::try_new(vec![template], Spec035TasksCoverage::all_available(0));
    assert_eq!(
        mismatch.expect_err("coverage must match rows").kind(),
        Spec035TasksValidationErrorKind::CoverageMismatch
    );
    Ok(())
}

#[test]
fn spec035_tasks_missing_evidence_never_becomes_success_or_zero() -> Result<(), Box<dyn Error>> {
    // Given
    let coverage = Spec035TasksCoverage::all_unavailable();
    let value = serde_json::to_value(Spec035TasksProjection::try_new(Vec::new(), coverage)?)?;

    // When / Then
    assert_eq!(
        value["coverage"]["goal"],
        json!({"availability":"unavailable"})
    );
    for (freshness, state) in [
        (Spec031Freshness::Unavailable, Spec035TaskState::Unavailable),
        (Spec031Freshness::Unknown, Spec035TaskState::Unknown),
    ] {
        assert_eq!(
            missing_child(freshness, state)?
                .expect_err("missing evidence cannot carry detail")
                .kind(),
            Spec035TasksValidationErrorKind::MisleadingSuccess
        );
    }
    Ok(())
}

#[test]
fn spec035_tasks_library_driver_writes_manual_qa_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let Some(path) = std::env::var_os("SPEC035_TASKS_ARTIFACT") else {
        return Ok(());
    };
    let projection = mixed_projection()?;
    let canonical: Value = serde_json::from_str(&serde_json::to_string(&projection)?)?;
    let malformed = json!({"schema_version": 2, "kind": "tasks", "rows": [], "coverage": {}});
    let malformed_json = serde_json::to_string(&malformed)?;

    // When
    let artifact = json!({
        "valid_projection": canonical,
        "rejected_malformed_cases": {
            "invalid_schema": Spec035TasksProjection::parse_json(&malformed_json).is_err(),
            "unsafe_locator": Spec031SubjectRef::try_new("/private/task").is_err(),
            "missing_evidence_detail": missing_child(
                Spec031Freshness::Unavailable,
                Spec035TaskState::Unavailable
            )?.is_err()
        }
    });
    fs::write(path, serde_json::to_vec_pretty(&artifact)?)?;

    // Then
    assert_eq!(artifact["rejected_malformed_cases"]["invalid_schema"], true);
    assert_eq!(artifact["rejected_malformed_cases"]["unsafe_locator"], true);
    assert_eq!(
        artifact["rejected_malformed_cases"]["missing_evidence_detail"],
        true
    );
    Ok(())
}
