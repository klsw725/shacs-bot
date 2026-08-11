use super::model::{Spec030ReleaseRunnerConfig, Spec030SurfaceOwnerSpawnSpec};

pub(super) fn spawn_spec(
    config: &Spec030ReleaseRunnerConfig,
    port: u16,
) -> Spec030SurfaceOwnerSpawnSpec {
    Spec030SurfaceOwnerSpawnSpec {
        executable: config
            .repo_root
            .join("crates/target/debug")
            .join(format!("shacs-bot{}", std::env::consts::EXE_SUFFIX))
            .display()
            .to_string(),
        config_path: config
            .evidence_root
            .join("surface/config.json")
            .display()
            .to_string(),
        workspace_path: config
            .evidence_root
            .join("surface/workspace")
            .display()
            .to_string(),
        bind: format!("127.0.0.1:{port}"),
        allow_api_side_effects: true,
    }
}
