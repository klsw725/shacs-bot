use shacs_config::{config_context, default_config_path};
use shacs_core::runtime::{
    recover_runtime_surface, request_runtime_control, request_surface_approval, SurfaceAction,
    SurfaceActionOutcome, SurfaceActionOutcomeKind, SurfaceActionRequestKind,
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

fn unavailable_outcome(error: shacs_core::runtime::SurfaceActionError) -> SurfaceActionOutcome {
    SurfaceActionOutcome {
        kind: SurfaceActionOutcomeKind::Unavailable,
        changed: false,
        detail: error.to_string(),
    }
}
