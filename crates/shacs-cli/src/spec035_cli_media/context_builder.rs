use super::super::AgentLoopChatCompletionAdapter;
use shacs_core::runtime::ContextBuilder;
use std::path::Path;

impl AgentLoopChatCompletionAdapter {
    pub(crate) fn context_builder(&self) -> ContextBuilder {
        let media_root = self.media_dir.parent().map(Path::to_path_buf);
        ContextBuilder::new(&self.workspace)
            .with_timezone(self.defaults.timezone.clone())
            .with_disabled_skills(self.defaults.disabled_skills.clone())
            .with_skill_config(&self.config_path)
            .with_skill_roots(self.plugin_skill_roots.clone())
            .with_media_roots(media_root)
            .with_native_image_input_supported(self.native_image_input_supported)
            .with_configured_env(self.exec_env.clone())
    }
}
