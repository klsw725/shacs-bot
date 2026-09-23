use shacs_config::{config_context, default_config_path};
use shacs_core::app::AppRegistryStore;
use shacs_core::app_lifecycle::AppSupervisorJournal;
use shacs_core::runtime::{
    apply_goal_surface_action, build_spec035_tasks_projection, recover_runtime_surface,
    request_runtime_control, request_surface_approval, validate_spec035_task_action, AppSupervisor,
    GoalSurfaceAction, Spec035TasksSemanticAction, SurfaceAction, SurfaceActionOutcome,
    SurfaceActionOutcomeKind, SurfaceActionRequestKind,
};
use shacs_projection::{
    negotiate_spec035_task_mutation, Spec035TransportCapability, Spec035TransportClientHello,
    Spec035TransportClientHelloInput, Spec035TransportClientId, Spec035TransportMutationRejection,
    Spec035TransportSchemaVersion, Spec035TransportValidationError,
};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run_surface_action(
    config_path: Option<&Path>,
    workspace: &Path,
    action: SurfaceAction,
) -> SurfaceActionOutcome {
    let context = config_context(
        Some(config_path.map_or_else(default_config_path, Path::to_path_buf)),
        Some(workspace.to_path_buf()),
    );
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default();
    match action {
        SurfaceAction::Stop => {
            request_runtime_control(&context.data_dir, SurfaceActionRequestKind::Stop, now_ms)
                .unwrap_or_else(unavailable_outcome)
        }
        SurfaceAction::Restart => {
            request_runtime_control(&context.data_dir, SurfaceActionRequestKind::Restart, now_ms)
                .unwrap_or_else(unavailable_outcome)
        }
        SurfaceAction::Recover => {
            recover_runtime_surface(&context.data_dir, now_ms).unwrap_or_else(unavailable_outcome)
        }
        SurfaceAction::Approve {
            session_key,
            lineage,
        } => request_surface_approval(&context.data_dir, &session_key, &lineage, true, now_ms)
            .unwrap_or_else(unavailable_outcome),
        SurfaceAction::Deny {
            session_key,
            lineage,
        } => request_surface_approval(&context.data_dir, &session_key, &lineage, false, now_ms)
            .unwrap_or_else(unavailable_outcome),
    }
}

pub struct TasksActionRequest<'a> {
    pub config_path: Option<&'a Path>,
    pub workspace: &'a Path,
    pub session_id: &'a str,
    pub action: Spec035TasksSemanticAction,
    pub transport_hello: Option<&'a Spec035TransportClientHello>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TasksActionResult {
    Owner(SurfaceActionOutcome),
    Rejected(Spec035TransportMutationRejection),
}

pub fn tui_transport_hello() -> Result<Spec035TransportClientHello, Spec035TransportValidationError>
{
    Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
        client_id: Spec035TransportClientId::try_new("client:tui-local")?,
        schema_versions: vec![Spec035TransportSchemaVersion::try_new(1)?],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskResume,
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskRecover,
        ],
        resume: None,
    })
}

pub fn run_tasks_action(request: TasksActionRequest<'_>) -> TasksActionResult {
    let Some(hello) = request.transport_hello else {
        return TasksActionResult::Rejected(
            Spec035TransportMutationRejection::capability_unavailable(),
        );
    };
    if let Err(rejection) = negotiate_spec035_task_mutation(
        hello,
        request.action.transport_capability(),
        "generation:tui-local",
    ) {
        return TasksActionResult::Rejected(rejection);
    }
    let context = config_context(
        Some(
            request
                .config_path
                .map_or_else(default_config_path, Path::to_path_buf),
        ),
        Some(request.workspace.to_path_buf()),
    );
    let projection = match build_spec035_tasks_projection(
        request.workspace,
        &context.data_dir,
        request.session_id,
    ) {
        Ok(projection) => projection,
        Err(error) => return TasksActionResult::Owner(tasks_unavailable(error.to_string())),
    };
    if let Err(error) = validate_spec035_task_action(&projection, &request.action) {
        return TasksActionResult::Owner(tasks_unavailable(error.to_string()));
    }
    TasksActionResult::Owner(dispatch_tasks_action(
        request.workspace,
        &context.data_dir,
        request.session_id,
        request.action,
    ))
}

fn dispatch_tasks_action(
    workspace: &Path,
    data_dir: &Path,
    session_id: &str,
    action: Spec035TasksSemanticAction,
) -> SurfaceActionOutcome {
    match action {
        Spec035TasksSemanticAction::GoalPause { .. } => apply_goal_surface_action(
            workspace,
            session_id,
            GoalSurfaceAction::Pause,
            &now_ms().to_string(),
        )
        .map_or_else(
            |error| tasks_unavailable(error.to_string()),
            |_| tasks_completed("goal pause completed"),
        ),
        Spec035TasksSemanticAction::GoalResume { .. } => apply_goal_surface_action(
            workspace,
            session_id,
            GoalSurfaceAction::Resume,
            &now_ms().to_string(),
        )
        .map_or_else(
            |error| tasks_unavailable(error.to_string()),
            |_| tasks_completed("goal resume completed"),
        ),
        Spec035TasksSemanticAction::AppStop { app_id } => {
            AppSupervisorJournal::new(AppRegistryStore::new(data_dir).apps_dir())
                .request(&app_id, shacs_core::app_lifecycle::AppLifecycleAction::Stop)
                .map_or_else(
                    |error| tasks_unavailable(error.to_string()),
                    |_| SurfaceActionOutcome {
                        kind: SurfaceActionOutcomeKind::Requested,
                        changed: true,
                        detail: "app stop requested".to_owned(),
                    },
                )
        }
        Spec035TasksSemanticAction::AppRecover { app_id } => {
            let journal = AppSupervisorJournal::new(AppRegistryStore::new(data_dir).apps_dir());
            AppSupervisor::new(&journal)
                .recover(&app_id, false)
                .map_or_else(
                    |error| tasks_unavailable(error.to_string()),
                    |_| tasks_completed("app recovery completed"),
                )
        }
        Spec035TasksSemanticAction::RuntimeRecover => {
            recover_runtime_surface(data_dir, now_ms()).unwrap_or_else(unavailable_outcome)
        }
    }
}

fn tasks_completed(detail: &str) -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Completed,
        changed: true,
        detail: detail.to_owned(),
    }
}

fn tasks_unavailable(detail: String) -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or_default()
}

fn unavailable_outcome(error: shacs_core::runtime::SurfaceActionError) -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail: error.to_string(),
    }
}
