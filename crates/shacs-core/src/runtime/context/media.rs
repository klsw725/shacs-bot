use super::{ContextBuildRequest, ContextBuilder};
use crate::runtime::file_context::{
    route_stored_attachment, AudioContextAnalyzer, MediaRootRouting,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Default)]
pub(super) struct MediaContext {
    audio: Option<Arc<dyn AudioContextAnalyzer>>,
}

impl ContextBuilder {
    pub fn with_audio_analyzer(mut self, analyzer: Arc<dyn AudioContextAnalyzer>) -> Self {
        self.media.audio = Some(analyzer);
        self
    }

    pub fn build_messages(&self, request: ContextBuildRequest<'_>) -> Vec<Value> {
        let runtime_context =
            self.build_runtime_context(request.channel, request.chat_id, request.session_summary);
        let user_content = self.build_user_content_for_request(&request);
        let merged = super::merge_runtime_context(runtime_context, user_content);
        let mut messages = vec![json!({
            "role": "system",
            "content": self.build_system_prompt(request.channel),
        })];
        messages.extend(request.history);
        if let Some(last) = messages.last_mut() {
            if last.get("role").and_then(Value::as_str) == Some(request.current_role) {
                let previous = last.get("content").cloned().unwrap_or(Value::Null);
                last["content"] = super::merge_message_content(previous, merged);
                return messages;
            }
        }
        messages.push(json!({"role": request.current_role, "content": merged}));
        messages
    }

    pub(super) fn build_user_content_for_request(
        &self,
        request: &ContextBuildRequest<'_>,
    ) -> Value {
        let mut blocks = Vec::new();
        for path in request.media {
            if path.starts_with("http://") || path.starts_with("https://") {
                continue;
            }
            let requested_path = PathBuf::from(path);
            match route_stored_attachment(
                &requested_path,
                &self.media_roots,
                self.native_image_input_supported,
                self.media.audio.as_deref(),
            ) {
                MediaRootRouting::Routed(routed_blocks) => {
                    blocks.extend(routed_blocks);
                    continue;
                }
                MediaRootRouting::IgnoredMediaRoot => continue,
                MediaRootRouting::OutsideMediaRoots => {}
            }
            let Ok(path) = self.resolve_workspace_media_path(&requested_path) else {
                continue;
            };
            let Ok(raw) = std::fs::read(&path) else {
                continue;
            };
            let Some(mime) =
                super::detect_image_mime(&raw).or_else(|| super::image_mime_from_extension(&path))
            else {
                continue;
            };
            if !self.native_image_input_supported {
                blocks.push(super::workspace_image_unsupported_note(&path, mime));
                continue;
            }
            blocks.push(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{mime};base64,{}", STANDARD.encode(raw))},
                "_meta": {"path": path.to_string_lossy()},
            }));
        }
        if blocks.is_empty() {
            Value::String(request.current_message.to_owned())
        } else {
            blocks.push(json!({"type": "text", "text": request.current_message}));
            Value::Array(blocks)
        }
    }
}
