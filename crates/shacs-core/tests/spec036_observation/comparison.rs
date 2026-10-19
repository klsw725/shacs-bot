use super::support::{Fixture, Reader, TestResult};
use shacs_core::runtime::*;

pub(super) fn configured(summary: bool) -> Result<Fixture, Box<dyn std::error::Error>> {
    let fixture = Fixture::new()?;
    let mut request = fixture.configuration(2, "source.txt");
    request.plan.comparison = Some(ObservationComparisonPolicy {
        criterion: ObservationCriterion::IntegerAtMost { value: 100 },
        max_age_seconds: 60,
        regular_summary: summary,
    });
    configure_entrusted_observation(&fixture.workspace, request)?;
    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(500))?;
    Ok(fixture)
}

pub(super) fn source(fixture: &Fixture, value: i64, as_of_ms: u64) -> TestResult {
    std::fs::write(
        fixture.workspace.join("source.txt"),
        serde_json::to_vec(
            &serde_json::json!({"schema":"integer_v1", "value": value, "as_of_ms": as_of_ms}),
        )?,
    )?;
    Ok(())
}

#[test]
fn user_condition_outcome_drives_a_a_irrelevant_b_without_outbound() -> TestResult {
    let fixture = configured(false)?;
    let reader = Reader::new(&fixture.workspace);
    for (time, value) in [
        (1000, 120),
        (6000, 120),
        (11000, 110),
        (16000, 90),
        (21000, 90),
    ] {
        source(&fixture, value, time)?;
        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(time), &reader)?;
    }

    let snapshot = fixture.read()?;

    let decisions = &snapshot.decisions;
    assert_eq!(
        decisions
            .iter()
            .map(|decision| decision.verdict)
            .collect::<Vec<_>>(),
        [
            ObservationVerdict::Baseline,
            ObservationVerdict::Suppress,
            ObservationVerdict::Suppress,
            ObservationVerdict::NotifyCandidate,
            ObservationVerdict::Suppress
        ]
    );
    assert_eq!(
        decisions[2].reason,
        ObservationComparisonReason::SameConditionOutcome
    );
    assert_eq!(
        decisions[3]
            .previous
            .as_ref()
            .ok_or("previous")?
            .summary
            .value,
        110
    );
    assert_eq!(
        decisions[3]
            .current
            .as_ref()
            .ok_or("current")?
            .summary
            .value,
        90
    );
    assert!(decisions
        .iter()
        .all(|decision| !decision.regular_summary_eligible));
    assert_eq!(
        (fixture.bus.inbound_size(), fixture.bus.outbound_size()),
        (0, 0)
    );
    Ok(())
}

#[test]
fn duplicate_and_restart_preserve_decision_identity_and_verdict() -> TestResult {
    let fixture = configured(false)?;
    source(&fixture, 120, 1000)?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;
    let before = fixture.read()?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    assert_eq!(fixture.read()?.decisions, before.decisions);
    assert_eq!(reader.reads.get(), 1);
    assert_eq!(
        before.comparison_baseline,
        fixture.read()?.comparison_baseline
    );
    Ok(())
}

#[test]
fn summary_opt_in_does_not_promote_suppression_to_notify() -> TestResult {
    let fixture = configured(true)?;
    source(&fixture, 120, 1000)?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &reader)?;

    let decision = fixture.read()?.current_decision.ok_or("decision")?;
    assert_eq!(decision.verdict, ObservationVerdict::Suppress);
    assert!(decision.regular_summary_eligible);
    assert_eq!(
        (fixture.bus.inbound_size(), fixture.bus.outbound_size()),
        (0, 0)
    );
    Ok(())
}

#[test]
fn unknown_source_never_establishes_baseline_or_summary_eligibility() -> TestResult {
    for (input, reason) in [
        (
            r#"{"schema":"integer_v1","value":90,"as_of_ms":100000}"#,
            ObservationComparisonReason::SourceFromFuture,
        ),
        (
            r#"{"schema":"integer_v1","value":90,"as_of_ms":0}"#,
            ObservationComparisonReason::SourceStale,
        ),
        (
            r#"{"schema":"integer_v1","value":90,"as_of_ms":1,"credential":"secret"}"#,
            ObservationComparisonReason::SummaryUnavailable,
        ),
        (
            "model says important",
            ObservationComparisonReason::SummaryUnavailable,
        ),
    ] {
        let fixture = configured(true)?;
        std::fs::write(fixture.workspace.join("source.txt"), input)?;

        fixture.dispatcher()?.process_entrusted_observations(
            &fixture.runtime(65000),
            &Reader::new(&fixture.workspace),
        )?;

        let snapshot = fixture.read()?;
        let decision = snapshot.current_decision.ok_or("decision")?;
        assert_eq!(decision.verdict, ObservationVerdict::Unknown);
        assert_eq!(decision.reason, reason);
        assert!(!decision.regular_summary_eligible);
        assert!(snapshot.comparison_baseline.is_none());
        let serialized = serde_json::to_string(&snapshot.history)?;
        assert!(!serialized.contains("credential"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("model says"));
    }
    Ok(())
}

#[test]
fn failed_or_denied_read_does_not_update_baseline() -> TestResult {
    for gate in [
        Ok("snapshot:test".to_owned()),
        Err(ObservationBlock::HookDenied),
    ] {
        let fixture = configured(false)?;
        source(&fixture, 120, 1000)?;
        fixture.dispatcher()?.process_entrusted_observations(
            &fixture.runtime(1000),
            &Reader::new(&fixture.workspace),
        )?;
        let baseline = fixture.read()?.comparison_baseline;
        std::fs::remove_file(fixture.workspace.join("source.txt"))?;
        let mut reader = Reader::new(&fixture.workspace);
        reader.gate = gate.clone();

        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(6000), &reader)?;

        let snapshot = fixture.read()?;
        assert_eq!(snapshot.comparison_baseline, baseline);
        assert_eq!(
            snapshot.current_decision.ok_or("decision")?.verdict,
            if gate.is_ok() {
                ObservationVerdict::Unknown
            } else {
                ObservationVerdict::Blocked
            }
        );
    }
    Ok(())
}

#[test]
fn missing_summary_requires_first_valid_baseline_before_verified_change() -> TestResult {
    let fixture = configured(false)?;
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &Reader::new(&fixture.workspace))?;
    source(&fixture, 90, 6000)?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(6000), &Reader::new(&fixture.workspace))?;

    let snapshot = fixture.read()?;
    let decision = snapshot.current_decision.ok_or("decision")?;
    assert_eq!(decision.verdict, ObservationVerdict::Baseline);
    assert_eq!(
        decision.reason,
        ObservationComparisonReason::FirstComparableObservation
    );
    assert!(decision.previous.is_none());
    assert_eq!(
        snapshot
            .comparison_baseline
            .ok_or("baseline")?
            .summary
            .value,
        90
    );
    Ok(())
}

#[test]
fn first_valid_observation_after_unknown_establishes_baseline_not_change() -> TestResult {
    for (first_at, input) in [
        (1000, "not a comparable summary"),
        (
            1000,
            r#"{"schema":"integer_v1","value":90,"as_of_ms":100000}"#,
        ),
        (65000, r#"{"schema":"integer_v1","value":90,"as_of_ms":0}"#),
    ] {
        let fixture = configured(false)?;
        let reader = Reader::new(&fixture.workspace);
        std::fs::write(fixture.workspace.join("source.txt"), input)?;
        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(first_at), &reader)?;
        for (offset, value) in [(5000, 120), (10000, 120), (15000, 90)] {
            let time = first_at + offset;
            source(&fixture, value, time)?;
            fixture
                .dispatcher()?
                .process_entrusted_observations(&fixture.runtime(time), &reader)?;
        }

        let verdicts: Vec<_> = fixture
            .read()?
            .decisions
            .iter()
            .map(|decision| decision.verdict)
            .collect();
        assert_eq!(
            verdicts,
            [
                ObservationVerdict::Unknown,
                ObservationVerdict::Baseline,
                ObservationVerdict::Suppress,
                ObservationVerdict::NotifyCandidate
            ]
        );
    }
    Ok(())
}

#[test]
fn baseline_survives_history_eviction_but_stale_evidence_blocks_comparison() -> TestResult {
    let fixture = configured(false)?;
    source(&fixture, 120, 1000)?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;
    let baseline = fixture.read()?.comparison_baseline;
    std::fs::write(fixture.workspace.join("source.txt"), b"unsupported")?;
    for index in 1..=33 {
        fixture
            .dispatcher()?
            .process_entrusted_observations(&fixture.runtime(1000 + index * 5000), &reader)?;
    }
    source(&fixture, 90, 171000)?;

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(171000), &reader)?;

    let snapshot = fixture.read()?;
    assert_eq!(snapshot.history.len(), 32);
    assert_eq!(snapshot.comparison_baseline, baseline);
    assert_eq!(
        snapshot.current_decision.ok_or("decision")?.reason,
        ObservationComparisonReason::BaselineStale
    );
    Ok(())
}
