use super::super::*;

impl AgentLoopChatCompletionAdapter {
    pub(crate) fn context_builder(&self) -> ContextBuilder {
        let mut extra_roots = Vec::new();
        let media_root = self.media_dir.parent().map(Path::to_path_buf);
        if let Some(data_dir) = self
            .media_dir
            .parent()
            .and_then(|media_dir| media_dir.parent())
        {
            extra_roots.push(data_dir.join("skills"));
        }
        extra_roots.extend(self.plugin_skill_roots.clone());
        let builder = ContextBuilder::new(&self.workspace)
            .with_timezone(self.defaults.timezone.clone())
            .with_disabled_skills(self.defaults.disabled_skills.clone())
            .with_skill_roots(extra_roots)
            .with_media_roots(media_root)
            .with_native_image_input_supported(self.native_image_input_supported)
            .with_configured_env(self.exec_env.clone());
        match self.config_path.parent() {
            Some(data_dir) => builder.with_video_projection_publication(
                shacs_core::runtime::Spec035MediaProjectionStore::new(data_dir),
                None,
            ),
            None => builder,
        }
    }
}
