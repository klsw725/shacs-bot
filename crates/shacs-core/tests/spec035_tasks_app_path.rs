use shacs_config::{config_context, ConfigContext};
use shacs_core::app::{AppId, AppLifecycleState, AppRegistry, AppRegistryEntry, AppRegistryStore};
use shacs_core::app_lifecycle::{AppLifecycleAction, AppProcessState, AppSupervisorJournal};
use shacs_core::runtime::{build_spec035_tasks_projection, Spec035TasksSourceError};
use shacs_projection::{
    Spec035TaskActionStatus, Spec035TaskCount, Spec035TaskOwnerKind, Spec035TaskState,
};
use std::error::Error;

struct Fixture {
    root: tempfile::TempDir,
    context: ConfigContext,
    app_id: AppId,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = tempfile::Builder::new()
            .prefix("tap-")
            .tempdir_in(std::env::temp_dir().canonicalize()?)?;
        let context = config_context(
            Some(root.path().join("data/config.json")),
            Some(root.path().join("workspace")),
        );
        Ok(Self {
            root,
            context,
            app_id: AppId::parse("fixture-app")?,
        })
    }

    fn register(&self) -> Result<(), Box<dyn Error>> {
        let entry = AppRegistryEntry {
            app_id: self.app_id.clone(),
            version: "1".to_owned(),
            digest: "sha256:fixture".to_owned(),
            bundle_path: self.context.data_dir.join("apps/fixture-app.shacsapp"),
            lifecycle_state: AppLifecycleState::Enabled,
            permission_requests: Vec::new(),
            secret_requests: Vec::new(),
            resource_summaries: Vec::new(),
            grant_reference: None,
            unavailable_reasons: Vec::new(),
            process_snapshots: Vec::new(),
            installed_at_unix_ms: 1,
        };
        AppRegistryStore::new(&self.context.data_dir).save(&AppRegistry {
            entries: [(self.app_id.clone(), entry)].into(),
        })?;
        Ok(())
    }

    fn journal(&self) -> AppSupervisorJournal {
        AppSupervisorJournal::new(self.context.runtime_subdir("apps"))
    }
}

#[test]
fn tasks_reads_running_app_from_cli_owner_journal() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.register()?;
    let journal = fixture.journal();
    let started = journal.request(&fixture.app_id, AppLifecycleAction::Start)?;
    journal.complete(&started, AppProcessState::Running, "", "", Vec::new())?;

    let projection = build_spec035_tasks_projection(
        &fixture.context.workspace,
        &fixture.context.data_dir,
        "cli:direct",
    )?;

    assert_eq!(projection.coverage().app, Spec035TaskCount::available(1));
    let app = projection.rows().first().ok_or("app row missing")?;
    assert_eq!(app.owner().kind(), Spec035TaskOwnerKind::App);
    assert_eq!(app.owner().locator().as_str(), fixture.app_id.as_str());
    assert_eq!(app.state(), Spec035TaskState::Running);
    assert_eq!(
        app.action().map(|action| action.status),
        Some(Spec035TaskActionStatus::Available)
    );
    assert!(!fixture.context.data_dir.join("runtime/apps").exists());
    fixture.root.close()?;
    Ok(())
}

#[test]
fn tasks_keeps_stop_requested_until_canonical_owner_completes() -> Result<(), Box<dyn Error>> {
    for completed in [false, true] {
        let fixture = Fixture::new()?;
        fixture.register()?;
        let journal = fixture.journal();
        let started = journal.request(&fixture.app_id, AppLifecycleAction::Start)?;
        journal.complete(&started, AppProcessState::Running, "", "", Vec::new())?;
        let stopped = journal.request(&fixture.app_id, AppLifecycleAction::Stop)?;
        if completed {
            journal.complete(&stopped, AppProcessState::Stopped, "", "", Vec::new())?;
        }

        let projection = build_spec035_tasks_projection(
            &fixture.context.workspace,
            &fixture.context.data_dir,
            "cli:direct",
        )?;

        let app = projection.rows().first().ok_or("app row missing")?;
        let (state, status) = if completed {
            (Spec035TaskState::Done, Spec035TaskActionStatus::Completed)
        } else {
            (
                Spec035TaskState::Requested,
                Spec035TaskActionStatus::Requested,
            )
        };
        assert_eq!(app.state(), state);
        assert_eq!(app.action().map(|action| action.status), Some(status));
        fixture.root.close()?;
    }
    Ok(())
}

#[test]
fn tasks_keeps_absent_registry_unavailable() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;

    let projection = build_spec035_tasks_projection(
        &fixture.context.workspace,
        &fixture.context.data_dir,
        "cli:direct",
    )?;

    assert_eq!(projection.coverage().app, Spec035TaskCount::Unavailable);
    assert!(projection.rows().is_empty());
    assert!(!fixture.context.data_dir.exists());
    fixture.root.close()?;
    Ok(())
}

#[test]
fn tasks_keeps_missing_owner_receipts_empty_without_wrong_path_fallback(
) -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.register()?;
    let wrong_journal = AppSupervisorJournal::new(fixture.context.data_dir.join("runtime/apps"));
    wrong_journal.request(&fixture.app_id, AppLifecycleAction::Start)?;

    let projection = build_spec035_tasks_projection(
        &fixture.context.workspace,
        &fixture.context.data_dir,
        "cli:direct",
    )?;

    assert_eq!(projection.coverage().app, Spec035TaskCount::available(0));
    assert!(projection.rows().is_empty());
    assert!(!fixture
        .context
        .runtime_subdir("apps")
        .join("app-supervisor")
        .exists());
    fixture.root.close()?;
    Ok(())
}

#[test]
fn tasks_rejects_corrupt_canonical_owner_receipt() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    fixture.register()?;
    let requested = fixture
        .journal()
        .request(&fixture.app_id, AppLifecycleAction::Start)?;
    let receipt_path = fixture
        .context
        .runtime_subdir("apps")
        .join("app-supervisor")
        .join(fixture.app_id.as_str())
        .join("receipts")
        .join(format!("{}.json", requested.receipt_id));
    std::fs::write(receipt_path, b"corrupt owner receipt")?;

    let result = build_spec035_tasks_projection(
        &fixture.context.workspace,
        &fixture.context.data_dir,
        "cli:direct",
    );

    assert!(matches!(result, Err(Spec035TasksSourceError::Owner(_))));
    fixture.root.close()?;
    Ok(())
}
