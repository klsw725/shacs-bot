use super::{ContextBuildRequest, ContextBuilder};
use crate::runtime::file_context::{
    route_stored_attachment_with_analyzer_invocation, AudioContextAnalyzer, MediaRootRouting,
    VideoContextAnalyzer,
};
use crate::runtime::video_analyzer_runtime::SupervisedVideoAnalyzer;
use crate::runtime::video_analyzer_spec035::VideoAnalyzerSpec035Publisher;
use crate::runtime::{
    AnalyzerInvocation, AnalyzerMediaProvenance, CancellationToken, Spec035MediaProjectionStore,
    VideoAnalyzerOwnerFactsProjection, VideoAnalyzerSnapshotProjection,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::{json, Value};
use shacs_projection::Spec031ExternalOwnerRef;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone)]
pub(super) struct MediaContext {
    audio: Option<Arc<dyn AudioContextAnalyzer>>,
    analyzer: Option<Arc<SupervisedVideoAnalyzer>>,
    staging_root: PathBuf,
    owner_ref: Option<Spec031ExternalOwnerRef>,
    snapshot_ref: Option<VideoAnalyzerSnapshotProjection>,
    publisher: Option<VideoAnalyzerSpec035Publisher>,
    provenance: AnalyzerMediaProvenance,
}

impl MediaContext {
    pub(super) fn new(workspace: &Path) -> Self {
        Self {
            audio: None,
            analyzer: None,
            staging_root: workspace.join(".shacs-video-analyzer-staging"),
            owner_ref: None,
            snapshot_ref: None,
            publisher: None,
            provenance: AnalyzerMediaProvenance::Inbound,
        }
    }
}

impl ContextBuilder {
    pub fn with_audio_analyzer(mut self, analyzer: Arc<dyn AudioContextAnalyzer>) -> Self {
        self.media.audio = Some(analyzer);
        self
    }

    pub fn with_video_analyzer(mut self, analyzer: Arc<dyn VideoContextAnalyzer>) -> Self {
        self.media.analyzer = Some(Arc::new(SupervisedVideoAnalyzer::new(analyzer)));
        self
    }

    pub fn with_video_analyzer_staging_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.media.staging_root = root.into();
        self
    }

    pub fn with_video_analyzer_owner_refs(
        mut self,
        owner_ref: Spec031ExternalOwnerRef,
        snapshot_ref: VideoAnalyzerSnapshotProjection,
    ) -> Self {
        self.media.owner_ref = Some(owner_ref);
        self.media.snapshot_ref = Some(snapshot_ref);
        self
    }

    pub fn with_video_media_provenance(mut self, provenance: AnalyzerMediaProvenance) -> Self {
        self.media.provenance = provenance;
        self
    }

    pub fn with_video_projection_publication(
        mut self,
        store: Spec035MediaProjectionStore,
        owner_facts: Option<VideoAnalyzerOwnerFactsProjection>,
    ) -> Self {
        self.media.publisher = Some(VideoAnalyzerSpec035Publisher::new(store, owner_facts));
        self
    }

    pub(crate) fn analyzer_invocation(
        &self,
        cancellation: CancellationToken,
        deadline: Option<Instant>,
    ) -> AnalyzerInvocation {
        let mut invocation = AnalyzerInvocation::new(self.media.staging_root.clone(), cancellation)
            .with_provenance(self.media.provenance);
        if let Some(deadline) = deadline {
            invocation = invocation.with_deadline(deadline);
        }
        if let (Some(owner_ref), Some(snapshot_ref)) = (
            self.media.owner_ref.clone(),
            self.media.snapshot_ref.clone(),
        ) {
            invocation = invocation.with_owner_refs(owner_ref, snapshot_ref);
        }
        invocation
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
        let invocation = request
            .analyzer_invocation
            .clone()
            .unwrap_or_else(|| self.analyzer_invocation(CancellationToken::new(), None));
        let mut blocks = Vec::new();
        for path in request.media {
            if path.starts_with("http://") || path.starts_with("https://") {
                continue;
            }
            let requested_path = PathBuf::from(path);
            match route_stored_attachment_with_analyzer_invocation(
                &requested_path,
                &self.media_roots,
                self.native_image_input_supported,
                self.media.audio.as_deref(),
                self.media.analyzer.clone(),
                &invocation,
                self.media.publisher.as_ref(),
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
