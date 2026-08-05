use super::lock::RuntimeOwnershipMutationLock;
use super::marker::{
    read_runtime_marker_json, read_runtime_ownership_marker, runtime_ownership_marker_path,
};
use super::{
    classify_runtime_ownership, RuntimeOwnershipState, SurfaceActionError, SurfaceActionOutcome,
    SurfaceActionOutcomeKind,
};
use serde_json::json;
use shacs_session::durable_event::{
    DurableEventInput, DurableEventPayload, DurableEventStore, RUNTIME_OWNER_LIFECYCLE,
};
use shacs_session::durable_replay::{evaluate_durable_recovery, DurableRecoveryStatus};
use std::fs;
use std::path::{Path, PathBuf};

pub fn recover_runtime_surface(
    data_dir: &Path,
    now_ms: u64,
) -> Result<SurfaceActionOutcome, SurfaceActionError> {
    let ownership_path = runtime_ownership_marker_path(data_dir);
    let _mutation_lock = RuntimeOwnershipMutationLock::acquire(&ownership_path)?;
    let ownership = read_runtime_ownership_marker(&ownership_path)?;
    if let Some(owner) = ownership.as_ref() {
        match classify_runtime_ownership(owner, now_ms) {
            RuntimeOwnershipState::Active => {
                return unavailable(
                    "runtime recover blocked: active runtime owner must stop first",
                );
            }
            RuntimeOwnershipState::Stale if owner.process_evidence.pid_alive => {
                return unavailable("runtime recover blocked: owner is live but lease-expired; request stop or inspect before takeover");
            }
            RuntimeOwnershipState::Stale => {}
        }
    }
    if update_marker_phase(data_dir)? == Some("partial_migration".to_owned()) {
        return unavailable(
            "runtime recover blocked: partial migration marker requires manual inspection",
        );
    }
    let durable_recovery = evaluate_durable_recovery(
        runtime_durable_event_root(data_dir),
        runtime_durable_checkpoint_root(data_dir),
    );
    match durable_recovery.status {
        DurableRecoveryStatus::Healthy => {}
        DurableRecoveryStatus::InspectOnly
        | DurableRecoveryStatus::Recoverable
        | DurableRecoveryStatus::Blocked => {
            return unavailable(&format!(
                "durable recovery status {} requires runtime recover repair path",
                durable_recovery.status.as_str()
            ));
        }
    }
    let Some(owner) = ownership else {
        return Ok(SurfaceActionOutcome {
            kind: SurfaceActionOutcomeKind::Completed,
            changed: false,
            detail: "no runtime update or stale ownership marker found".to_owned(),
        });
    };
    append_runtime_owner_lifecycle(data_dir, now_ms, &owner)?;
    fs::remove_file(&ownership_path)?;
    Ok(SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Completed,
        changed: true,
        detail: "cleared stale runtime ownership marker".to_owned(),
    })
}

fn unavailable(detail: &str) -> Result<SurfaceActionOutcome, SurfaceActionError> {
    Ok(SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail: detail.to_owned(),
    })
}

fn runtime_durable_event_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("durable-events")
}

fn runtime_durable_checkpoint_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("durable-checkpoints")
}

fn runtime_update_marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("update-marker.json")
}

fn update_marker_phase(data_dir: &Path) -> Result<Option<String>, SurfaceActionError> {
    let Some(value) = read_runtime_marker_json(&runtime_update_marker_path(data_dir))? else {
        return Ok(None);
    };
    value
        .get("phase")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            SurfaceActionError::InvalidMarker("runtime update marker missing `phase`".to_owned())
        })
        .map(Some)
}

fn append_runtime_owner_lifecycle(
    data_dir: &Path,
    observed_at_ms: u64,
    owner: &super::marker::RuntimeOwnershipMarker,
) -> Result<(), SurfaceActionError> {
    let mut events = DurableEventStore::open(runtime_durable_event_root(data_dir))
        .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    events
        .append(DurableEventInput::new(
            "runtime",
            RUNTIME_OWNER_LIFECYCLE,
            DurableEventPayload::inline(
                "runtime_owner_lifecycle",
                json!({
                    "lifecycle": "recover_observed_stale_owner",
                    "observed_at_ms": observed_at_ms,
                    "state": "stale",
                    "owner": {
                        "owner_id": owner.owner_id,
                        "pid": owner.pid,
                        "expires_at_ms": owner.expires_at_ms,
                    },
                }),
            ),
        ))
        .map_err(|error| SurfaceActionError::Durable(error.to_string()))?;
    Ok(())
}
