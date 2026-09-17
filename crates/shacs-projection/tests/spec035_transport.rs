use serde_json::{json, Value};
use shacs_projection::*;
use std::{error::Error, fs};

const CONFLICTING_DUPLICATE: &str = r#"{"schema_version":2,"generation":"generation-current","capability_decisions":[{"capability":"task_stop","support":{"status":"supported"}},{"capability":"task_stop","support":{"status":"unsupported","reason":"capability_unavailable"}}]}"#;
const IDENTICAL_DUPLICATE: &str = r#"{"schema_version":2,"generation":"generation-current","capability_decisions":[{"capability":"task_stop","support":{"status":"supported"}},{"capability":"task_stop","support":{"status":"supported"}}]}"#;

fn client_id() -> Result<Spec035TransportClientId, Spec035TransportValidationError> {
    Spec035TransportClientId::try_new("client:local-ui")
}

fn generation(value: &str) -> Result<Spec035TransportGeneration, Spec035TransportValidationError> {
    Spec035TransportGeneration::try_new(value)
}

fn client_hello() -> Result<Spec035TransportClientHello, Spec035TransportValidationError> {
    Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
        client_id: client_id()?,
        schema_versions: vec![
            Spec035TransportSchemaVersion::try_new(1)?,
            Spec035TransportSchemaVersion::try_new(2)?,
        ],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskRetry,
        ],
        resume: Some(Spec035TransportResumeCursor::new(
            generation("generation-previous")?,
            Spec035TransportSequence::new(9),
        )),
    })
}

fn server_hello() -> Result<Spec035TransportServerHello, Box<dyn Error>> {
    let offer = Spec035TransportOffer::try_new(Spec035TransportOfferInput {
        schema_versions: vec![
            Spec035TransportSchemaVersion::try_new(2)?,
            Spec035TransportSchemaVersion::try_new(1)?,
        ],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskStop,
        ],
        generation: generation("generation-current")?,
    })?;
    Ok(offer.select(&client_hello()?)?)
}

fn metadata(
    kind: Spec035TransportEventKind,
    generation_value: &str,
    sequence: u64,
) -> Result<Spec035TransportEventMetadata, Spec035TransportValidationError> {
    Spec035TransportEventMetadata::try_new(
        kind,
        generation(generation_value)?,
        Spec035TransportSequence::new(sequence),
    )
}

fn current(
    kind: Spec035TransportEventKind,
    sequence: u64,
) -> Result<Spec035TransportEventMetadata, Spec035TransportValidationError> {
    metadata(kind, "generation-current", sequence)
}

fn apply(
    ordering: &mut Spec035TransportOrdering,
    kind: Spec035TransportEventKind,
    sequence: u64,
) -> Result<Spec035TransportDecision, Spec035TransportValidationError> {
    Ok(ordering.observe(&current(kind, sequence)?))
}

fn stale_delta(
    ordering: &mut Spec035TransportOrdering,
    sequence: u64,
) -> Result<Spec035TransportDecision, Spec035TransportValidationError> {
    Ok(ordering.observe(&metadata(
        Spec035TransportEventKind::Delta,
        "generation-stale",
        sequence,
    )?))
}

#[test]
fn spec035_transport_selects_supported_and_unsupported_capabilities_deterministically(
) -> Result<(), Box<dyn Error>> {
    // Given
    let hello = client_hello()?;
    let server = server_hello()?;

    // When
    let first = serde_json::to_string(&server)?;
    let second = serde_json::to_string(&server)?;

    // Then
    assert_eq!(first, second);
    assert_eq!(Spec035TransportServerHello::parse_json(&first)?, server);
    assert_eq!(server.schema_version().as_u32(), 2);
    assert_eq!(
        server
            .capability_decisions()
            .iter()
            .map(|decision| (decision.capability(), decision.support()))
            .collect::<Vec<_>>(),
        vec![
            (
                Spec035TransportCapability::TaskPause,
                Spec035TransportCapabilitySupport::Supported
            ),
            (
                Spec035TransportCapability::TaskStop,
                Spec035TransportCapabilitySupport::Supported
            ),
            (
                Spec035TransportCapability::TaskRetry,
                Spec035TransportCapabilitySupport::Unsupported {
                    reason: Spec035TransportUnsupportedReason::CapabilityUnavailable,
                },
            ),
        ]
    );
    assert_eq!(
        hello.resume().map(|cursor| cursor.sequence().as_u64()),
        Some(9)
    );
    Ok(())
}

#[test]
fn spec035_transport_rejects_conflicting_duplicate_capability_decisions() {
    // Given
    let input = CONFLICTING_DUPLICATE;

    // When
    let result = Spec035TransportServerHello::parse_json(input);

    // Then
    assert!(result.is_err());
}

#[test]
fn spec035_transport_rejects_identical_duplicate_capability_decisions() {
    // Given
    let input = IDENTICAL_DUPLICATE;

    // When
    let result = Spec035TransportServerHello::parse_json(input);

    // Then
    assert!(result.is_err());
}

#[test]
fn spec035_transport_advances_hello_snapshot_and_monotonic_delta() -> Result<(), Box<dyn Error>> {
    // Given
    let mut ordering = Spec035TransportOrdering::new();

    // When / Then
    assert_eq!(ordering.phase(), Spec035TransportPhase::AwaitingHello);
    assert_eq!(
        ordering.accept_hello(&server_hello()?),
        Spec035TransportDecision::AcceptHello
    );
    assert_eq!(ordering.phase(), Spec035TransportPhase::AwaitingSnapshot);
    assert_eq!(
        ordering.observe(&current(Spec035TransportEventKind::Snapshot, 4)?),
        Spec035TransportDecision::ApplySnapshot
    );
    assert_eq!(ordering.phase(), Spec035TransportPhase::Live);
    assert_eq!(
        ordering.observe(&current(Spec035TransportEventKind::Delta, 5)?),
        Spec035TransportDecision::ApplyDelta
    );
    assert_eq!(ordering.counters().applied_deltas().as_u64(), 1);
    Ok(())
}

#[test]
fn spec035_transport_rejects_pre_snapshot_delta_explicitly() -> Result<(), Box<dyn Error>> {
    // Given
    let mut ordering = Spec035TransportOrdering::new();

    // When
    let decision = apply(&mut ordering, Spec035TransportEventKind::Delta, 1)?;

    // Then
    assert_eq!(decision, Spec035TransportDecision::RejectPreSnapshot);
    assert_eq!(ordering.phase(), Spec035TransportPhase::AwaitingHello);
    assert_eq!(ordering.counters().pre_snapshot_deltas().as_u64(), 1);
    Ok(())
}

#[test]
fn spec035_transport_rejects_stale_duplicate_and_gap_with_independent_counters(
) -> Result<(), Box<dyn Error>> {
    // Given
    let mut ordering = Spec035TransportOrdering::new();
    assert_eq!(
        ordering.accept_hello(&server_hello()?),
        Spec035TransportDecision::AcceptHello
    );
    assert_eq!(
        ordering.observe(&current(Spec035TransportEventKind::Snapshot, 7)?),
        Spec035TransportDecision::ApplySnapshot
    );

    // When / Then
    assert_eq!(
        stale_delta(&mut ordering, 8)?,
        Spec035TransportDecision::RejectStaleGeneration
    );
    assert_eq!(
        ordering.observe(&current(Spec035TransportEventKind::Delta, 7)?),
        Spec035TransportDecision::RejectDuplicate
    );
    assert_eq!(
        ordering.observe(&current(Spec035TransportEventKind::Delta, 9)?),
        Spec035TransportDecision::RejectGap
    );
    assert_eq!(ordering.counters().stale_generations().as_u64(), 1);
    assert_eq!(ordering.counters().duplicate_sequences().as_u64(), 1);
    assert_eq!(ordering.counters().sequence_gaps().as_u64(), 1);
    assert_eq!(
        ordering
            .last_sequence()
            .map(Spec035TransportSequence::as_u64),
        Some(7)
    );
    Ok(())
}

#[test]
fn spec035_transport_metadata_parsing_does_not_bypass_projection_schema_parser(
) -> Result<(), Box<dyn Error>> {
    // Given
    let encoded = serde_json::to_string(&current(Spec035TransportEventKind::Snapshot, 0)?)?;
    let unsupported_tasks = r#"{"schema_version":2,"kind":"tasks","rows":[],"coverage":{}}"#;

    // When
    let parsed = Spec035TransportEventMetadata::parse_json(&encoded)?;

    // Then
    assert_eq!(parsed.kind(), Spec035TransportEventKind::Snapshot);
    assert_eq!(
        Spec035TasksProjection::parse_json(unsupported_tasks)
            .expect_err("payload schema remains delegated")
            .kind(),
        Spec035TasksParseErrorKind::InvalidSchema
    );
    assert!(Spec035TransportEventMetadata::parse_json(
        r#"{"kind":"delta","generation":"generation-current","sequence":1,"payload":{}}"#
    )
    .is_err());
    Ok(())
}

#[test]
fn spec035_transport_library_driver_writes_manual_qa_artifact() -> Result<(), Box<dyn Error>> {
    // Given
    let Some(path) = std::env::var_os("SPEC035_TRANSPORT_ARTIFACT") else {
        return Ok(());
    };
    let server = server_hello()?;
    let mut ordering = Spec035TransportOrdering::new();

    // When
    let decisions = vec![
        apply(&mut ordering, Spec035TransportEventKind::Delta, 1)?,
        ordering.accept_hello(&server),
        apply(&mut ordering, Spec035TransportEventKind::Snapshot, 4)?,
        apply(&mut ordering, Spec035TransportEventKind::Delta, 5)?,
        stale_delta(&mut ordering, 6)?,
        apply(&mut ordering, Spec035TransportEventKind::Delta, 5)?,
        apply(&mut ordering, Spec035TransportEventKind::Delta, 7)?,
    ];
    let malformed_input_rejected = Spec035TransportEventMetadata::parse_json("{broken").is_err();
    let unsupported_payload_schema_rejected = Spec035TasksProjection::parse_json(
        r#"{"schema_version":2,"kind":"tasks","rows":[],"coverage":{}}"#,
    )
    .is_err();
    let artifact: Value = json!({
        "server_hello": server,
        "decisions": decisions,
        "counters": ordering.counters(),
        "final_phase": ordering.phase(),
        "malformed_input_rejected": malformed_input_rejected,
        "unsupported_payload_schema_rejected": unsupported_payload_schema_rejected,
        "duplicate_capability_decisions_rejected": {
            "conflicting": Spec035TransportServerHello::parse_json(CONFLICTING_DUPLICATE).is_err(),
            "identical": Spec035TransportServerHello::parse_json(IDENTICAL_DUPLICATE).is_err()
        },
        "acknowledgement_claimed": false,
        "exactly_once_claimed": false
    });
    fs::write(path, serde_json::to_vec_pretty(&artifact)?)?;

    // Then
    assert_eq!(artifact["final_phase"], "live");
    assert_eq!(artifact["counters"]["pre_snapshot_deltas"], 1);
    assert_eq!(artifact["counters"]["stale_generations"], 1);
    assert_eq!(artifact["counters"]["duplicate_sequences"], 1);
    assert_eq!(artifact["counters"]["sequence_gaps"], 1);
    Ok(())
}
