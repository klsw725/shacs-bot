use shacs_cli::{parse_cli_args, run_command};
use shacs_core::runtime::{
    EntrustedObservationSnapshot, EntrustedWorkSnapshot, ObservationNextWake,
};
use shacs_session::{Session, SessionManager};
use std::path::Path;

#[test]
fn observation_configuration_and_unknown_history_round_trip_through_cli(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let mut sessions = SessionManager::new(root.path())?;
    sessions.save(&Session::new("cli:owner"))?;
    let work: EntrustedWorkSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "create",
            "watch local file",
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:user",
            "--instructions",
            "read only",
        ],
    )?)?;

    let configured: EntrustedWorkSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "observe",
            &work.work.work_id,
            "--source",
            "input.txt",
            "--cadence-seconds",
            "5",
            "--expected-revision",
            "1",
        ],
    )?)?;
    let snapshot: EntrustedObservationSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "observations",
            &work.work.work_id,
            "--data-dir",
            root.path().to_str().ok_or("path")?,
        ],
    )?)?;

    assert_eq!(configured.work.current_revision, 2);
    assert!(snapshot.current_observation.is_none());
    assert!(matches!(
        snapshot.next_wake,
        ObservationNextWake::AwaitingEnqueue { .. }
    ));
    assert!(run(
        root.path(),
        &[
            "observe",
            &work.work.work_id,
            "--source",
            "../escape",
            "--cadence-seconds",
            "1",
            "--expected-revision",
            "2"
        ]
    )
    .is_err());
    Ok(())
}

fn run(workspace: &Path, action: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let mut args = vec!["work".to_owned()];
    args.extend(action.iter().map(|value| (*value).to_owned()));
    args.extend([
        "--workspace".to_owned(),
        workspace.display().to_string(),
        "--session".to_owned(),
        "cli:owner".to_owned(),
    ]);
    Ok(run_command(parse_cli_args(args)?)?)
}

#[test]
fn explicit_comparison_and_summary_preference_round_trip_with_cli_cas(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    SessionManager::new(root.path())?.save(&Session::new("cli:owner"))?;
    let created: EntrustedWorkSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "create",
            "watch local value",
            "--consent",
            "explicit-request",
            "--evidence-ref",
            "cli:user",
            "--instructions",
            "read only",
        ],
    )?)?;
    let args = [
        "observe",
        &created.work.work_id,
        "--source",
        "source.json",
        "--cadence-seconds",
        "5",
        "--expected-revision",
        "1",
        "--at-most",
        "100",
        "--max-age-seconds",
        "60",
        "--regular-summary",
        "true",
    ];

    let configured: EntrustedWorkSnapshot = serde_json::from_str(&run(root.path(), &args)?)?;

    let policy = configured
        .work
        .observation_plan
        .ok_or("plan")?
        .comparison
        .ok_or("policy")?;
    assert_eq!(
        policy.criterion,
        shacs_core::runtime::ObservationCriterion::IntegerAtMost { value: 100 }
    );
    assert!(policy.regular_summary);
    assert!(run(root.path(), &args).is_err());
    let read: EntrustedObservationSnapshot = serde_json::from_str(&run(
        root.path(),
        &[
            "observations",
            &created.work.work_id,
            "--data-dir",
            root.path().to_str().ok_or("path")?,
        ],
    )?)?;
    assert_eq!(read.plan.ok_or("plan")?.comparison, Some(policy));
    assert!(read.current_decision.is_none());
    Ok(())
}

#[test]
fn summary_flag_without_explicit_criterion_is_rejected() {
    assert!(parse_cli_args([
        "work",
        "observe",
        "work-1",
        "--workspace",
        "/tmp",
        "--session",
        "cli:owner",
        "--source",
        "source.json",
        "--cadence-seconds",
        "5",
        "--expected-revision",
        "1",
        "--regular-summary",
        "true"
    ])
    .is_err());
}
