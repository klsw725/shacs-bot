use super::{
    assemble_spec035_tasks, build_spec033_snapshot_from, Spec035LocatedOwnerFact,
    Spec035RecoveryOwnerFact, Spec035TasksOwnerSnapshots, Spec035TasksOwnerSource,
};
use shacs_app::app::AppRegistryStore;
use shacs_app::app_lifecycle::AppSupervisorJournal;
use shacs_projection::{Spec031Freshness, Spec031ObservedAtUnixMs, Spec035TasksProjection};
use shacs_session::durable_replay::evaluate_durable_recovery;
use shacs_session::SessionManager;
use shacs_workflow::WorkflowProjection;
use std::path::Path;

mod action;

pub use action::{
    accept_spec035_surface_action_outcome, validate_spec035_task_action,
    Spec035TasksSemanticAction, Spec035TasksSourceError,
};

pub fn build_spec035_tasks_projection(
    workspace: &Path,
    data_dir: &Path,
    session_id: &str,
) -> Result<Spec035TasksProjection, Spec035TasksSourceError> {
    let mut snapshots = Spec035TasksOwnerSnapshots::all_unavailable(Spec031Freshness::Unavailable);
    let spec033 = build_spec033_snapshot_from(workspace, data_dir, session_id)
        .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))?;
    if let Some(fact) = spec033.goal.fact {
        let observed_at = Spec031ObservedAtUnixMs::new(
            fact.latest_transition
                .as_ref()
                .and_then(|transition| transition.observed_at.parse().ok())
                .unwrap_or_default(),
        );
        snapshots.goal = available(
            observed_at,
            vec![Spec035LocatedOwnerFact {
                locator: fact.goal_id.clone(),
                fact,
            }],
        );
    }
    if let Some(fact) = spec033.automation.fact {
        snapshots.automation = available(
            Spec031ObservedAtUnixMs::new(0),
            vec![Spec035LocatedOwnerFact {
                locator: fact.work_id.clone(),
                fact,
            }],
        );
    }
    populate_session_owners(&mut snapshots, workspace, session_id)?;
    populate_durable_owners(&mut snapshots, data_dir, session_id);
    populate_app_owners(&mut snapshots, data_dir)?;
    assemble_spec035_tasks(snapshots).map_err(Into::into)
}

fn populate_session_owners(
    snapshots: &mut Spec035TasksOwnerSnapshots,
    workspace: &Path,
    session_id: &str,
) -> Result<(), Spec035TasksSourceError> {
    let Some(manager) = SessionManager::open_existing(workspace)
        .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))?
    else {
        return Ok(());
    };
    let Some(session) = manager.load_existing(session_id) else {
        return Ok(());
    };
    let Some(value) = session
        .metadata
        .get("runtime_workflow")
        .and_then(|value| value.get("projection"))
    else {
        return Ok(());
    };
    let fact: WorkflowProjection = serde_json::from_value(value.clone())
        .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))?;
    snapshots.workflow = available(
        Spec031ObservedAtUnixMs::new(0),
        vec![Spec035LocatedOwnerFact {
            locator: fact.workflow_id.clone(),
            fact,
        }],
    );
    Ok(())
}

fn populate_durable_owners(
    snapshots: &mut Spec035TasksOwnerSnapshots,
    data_dir: &Path,
    session_id: &str,
) {
    let event_root = data_dir.join("runtime/durable-events");
    if !event_root.join("events.log").exists() {
        return;
    }
    let admission =
        evaluate_durable_recovery(&event_root, data_dir.join("runtime/durable-checkpoints"));
    let children: Vec<_> = admission
        .state
        .as_ref()
        .map(|state| {
            state
                .children
                .items
                .values()
                .filter(|child| child.session_id == session_id)
                .cloned()
                .map(|fact| Spec035LocatedOwnerFact {
                    locator: fact.child_task_id.clone(),
                    fact,
                })
                .collect()
        })
        .unwrap_or_default();
    let observed_at = Spec031ObservedAtUnixMs::new(
        children
            .iter()
            .map(|located| {
                located
                    .fact
                    .finished_at_ms
                    .or(located.fact.started_at_ms)
                    .unwrap_or(located.fact.spawned_at_ms)
            })
            .max()
            .unwrap_or_default(),
    );
    if admission.state.is_some() {
        snapshots.child = available(observed_at, children);
    }
    snapshots.recovery = available(
        observed_at,
        vec![Spec035LocatedOwnerFact {
            locator: "runtime:recovery".to_owned(),
            fact: Spec035RecoveryOwnerFact::Admission(admission),
        }],
    );
}

fn populate_app_owners(
    snapshots: &mut Spec035TasksOwnerSnapshots,
    data_dir: &Path,
) -> Result<(), Spec035TasksSourceError> {
    let store = AppRegistryStore::new(data_dir);
    if !store.registry_path().exists() {
        return Ok(());
    }
    let journal = AppSupervisorJournal::new(store.apps_dir());
    let mut facts = Vec::new();
    for app in store
        .list()
        .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))?
    {
        if let Some(fact) = journal
            .replay(&app.app_id)
            .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))?
            .receipts
            .into_iter()
            .last()
        {
            facts.push(Spec035LocatedOwnerFact {
                locator: app.app_id.as_str().to_owned(),
                fact,
            });
        }
    }
    let observed_at = Spec031ObservedAtUnixMs::new(
        facts
            .iter()
            .map(|located| u64::try_from(located.fact.occurred_at_unix_ms).unwrap_or(u64::MAX))
            .max()
            .unwrap_or_default(),
    );
    snapshots.app = available(observed_at, facts);
    Ok(())
}

pub fn serialize_spec035_tasks_projection(
    projection: &Spec035TasksProjection,
) -> Result<String, Spec035TasksSourceError> {
    serde_json::to_value(projection)
        .and_then(|value| serde_json::to_string(&value))
        .map_err(|error| Spec035TasksSourceError::Owner(error.to_string()))
}

fn available<T>(
    observed_at_unix_ms: Spec031ObservedAtUnixMs,
    facts: Vec<Spec035LocatedOwnerFact<T>>,
) -> Spec035TasksOwnerSource<T> {
    Spec035TasksOwnerSource::Available {
        observed_at_unix_ms,
        freshness: Spec031Freshness::Current,
        facts,
    }
}
