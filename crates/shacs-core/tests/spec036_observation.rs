#[path = "spec036_observation/comparison.rs"]
mod comparison;
#[path = "spec036_observation/comparison_lineage.rs"]
mod comparison_lineage;
#[path = "spec036_observation/gates.rs"]
mod gates;
#[path = "spec036_observation/recovery.rs"]
mod recovery;
#[path = "spec036_observation/support.rs"]
mod support;

use shacs_core::runtime::*;
use support::{Fixture, Reader, TestResult};

#[test]
fn due_observation_and_next_wake_survive_dispatcher_restart_without_messages() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = Reader::new(&fixture.workspace);
    fixture
        .dispatcher()?
        .enqueue_entrusted_observations(&fixture.runtime(500))?;
    let before = fixture.read()?;
    assert!(before.current_observation.is_none());
    assert!(matches!(
        before.next_wake,
        ObservationNextWake::Queued {
            due_at_ms: 1000,
            ..
        }
    ));

    fixture
        .dispatcher()?
        .process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    let after = fixture.read()?;
    assert_eq!(after.history.len(), 1);
    assert!(matches!(
        after
            .current_observation
            .as_ref()
            .map(|record| &record.result),
        Some(ObservationResult::Observed { byte_count: 21, .. })
    ));
    assert!(matches!(
        after.next_wake,
        ObservationNextWake::Queued {
            due_at_ms: 6000,
            ..
        }
    ));
    assert_eq!(fixture.bus.outbound_size(), 0);
    assert_eq!(fixture.bus.inbound_size(), 0);
    Ok(())
}

#[test]
fn duplicate_tick_does_not_read_or_commit_again_before_next_wake() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = Reader::new(&fixture.workspace);
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;
    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    assert_eq!(reader.reads.get(), 1);
    assert_eq!(fixture.read()?.history.len(), 1);
    Ok(())
}

#[test]
fn missing_source_records_unavailable_and_stops_cadence() -> TestResult {
    let fixture = Fixture::new()?;
    std::fs::remove_file(fixture.workspace.join("source.txt"))?;
    let reader = Reader::new(&fixture.workspace);
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;

    dispatcher.process_entrusted_observations(&fixture.runtime(1000), &reader)?;

    let snapshot = fixture.read()?;
    assert_eq!(
        snapshot.current_observation.ok_or("observation")?.result,
        ObservationResult::Unavailable {
            reason: ObservationBlock::SourceMissing
        }
    );
    assert_eq!(
        snapshot.next_wake,
        ObservationNextWake::Stopped {
            reason: ObservationBlock::SourceMissing
        }
    );
    Ok(())
}

#[test]
fn directory_and_oversized_sources_are_not_successful_observations() -> TestResult {
    for (source, expected) in [
        ("directory", ObservationBlock::UnsupportedSource),
        ("large", ObservationBlock::SourceTooLarge),
    ] {
        let fixture = Fixture::new()?;
        if source == "directory" {
            std::fs::create_dir(fixture.workspace.join(source))?;
        } else {
            std::fs::write(fixture.workspace.join(source), vec![b'a'; 65_537])?;
        }
        configure_entrusted_observation(&fixture.workspace, fixture.configuration(2, source))?;
        let mut dispatcher = fixture.dispatcher()?;
        dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;

        dispatcher.process_entrusted_observations(
            &fixture.runtime(1000),
            &Reader::new(&fixture.workspace),
        )?;

        assert_eq!(
            fixture
                .read()?
                .current_observation
                .ok_or("observation")?
                .result,
            ObservationResult::Unavailable { reason: expected }
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_source_cannot_escape_workspace() -> TestResult {
    let fixture = Fixture::new()?;
    std::fs::write(fixture.root.path().join("outside"), b"secret")?;
    std::os::unix::fs::symlink(
        fixture.root.path().join("outside"),
        fixture.workspace.join("link"),
    )?;
    configure_entrusted_observation(&fixture.workspace, fixture.configuration(2, "link"))?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;

    dispatcher
        .process_entrusted_observations(&fixture.runtime(1000), &Reader::new(&fixture.workspace))?;

    assert_eq!(
        fixture
            .read()?
            .current_observation
            .ok_or("observation")?
            .result,
        ObservationResult::Unavailable {
            reason: ObservationBlock::UnsupportedSource
        }
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn unreadable_local_source_records_read_failure() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new()?;
    std::fs::set_permissions(
        fixture.workspace.join("source.txt"),
        std::fs::Permissions::from_mode(0o000),
    )?;
    let mut dispatcher = fixture.dispatcher()?;
    dispatcher.enqueue_entrusted_observations(&fixture.runtime(500))?;

    dispatcher
        .process_entrusted_observations(&fixture.runtime(1000), &Reader::new(&fixture.workspace))?;

    assert_eq!(
        fixture
            .read()?
            .current_observation
            .ok_or("observation")?
            .result,
        ObservationResult::Unavailable {
            reason: ObservationBlock::ReadFailed
        }
    );
    Ok(())
}
