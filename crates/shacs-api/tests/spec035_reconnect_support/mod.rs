use serde_json::{json, Value};
use shacs_api::{
    ApiError, ChatCompletionAdapter, ChatCompletionInvocation, Spec035TasksStreamEvent,
};
use shacs_channels::WebSocketServerEvent;
use shacs_providers::LlmResponse;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};

pub struct ReconnectAdapter {
    workspace: PathBuf,
    reached: Arc<Barrier>,
    release: Arc<Barrier>,
    payload: Value,
}

pub struct ReconnectFixture {
    pub adapter: Arc<ReconnectAdapter>,
    pub reached: Arc<Barrier>,
    pub release: Arc<Barrier>,
}

impl ChatCompletionAdapter for ReconnectAdapter {
    fn configured_model(&self) -> &str {
        "fixture"
    }

    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        Ok(LlmResponse {
            content: Some("done".to_owned()),
            ..LlmResponse::default()
        })
    }

    fn session_workspace(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn runtime_data_dir(&self) -> Option<PathBuf> {
        Some(self.workspace.clone())
    }

    fn process_websocket_frame_streaming(
        &self,
        _: Value,
        _: &str,
        default_chat_id: &str,
        on_event: &mut dyn FnMut(WebSocketServerEvent),
    ) -> Result<(), ApiError> {
        on_event(WebSocketServerEvent::Delta {
            chat_id: default_chat_id.to_owned(),
            text: "progress".to_owned(),
            stream_id: Some("stream:accounting".to_owned()),
        });
        on_event(WebSocketServerEvent::Message {
            chat_id: default_chat_id.to_owned(),
            text: "final".to_owned(),
            buttons: Vec::new(),
            button_prompt: None,
            media: Vec::new(),
            reply_to: None,
            kind: None,
        });
        Ok(())
    }

    fn stream_spec035_tasks_events(
        &self,
        _: &str,
        on_event: &mut dyn FnMut(Spec035TasksStreamEvent),
    ) -> Result<(), ApiError> {
        self.reached.wait();
        self.release.wait();
        on_event(Spec035TasksStreamEvent::owner_projection(
            json!({"schema_version": 999}),
        ));
        on_event(Spec035TasksStreamEvent::owner_projection(
            self.payload.clone(),
        ));
        Ok(())
    }
}

pub fn adapter(workspace: PathBuf) -> Result<ReconnectFixture, Box<dyn std::error::Error>> {
    let reached = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let payload = serde_json::to_value(shacs_core::runtime::build_spec035_tasks_projection(
        &workspace, &workspace, "qa",
    )?)?;
    Ok(ReconnectFixture {
        adapter: Arc::new(ReconnectAdapter {
            workspace,
            reached: reached.clone(),
            release: release.clone(),
            payload,
        }),
        reached,
        release,
    })
}

pub async fn release_events(
    reached: &Arc<Barrier>,
    release: &Arc<Barrier>,
) -> Result<(), tokio::task::JoinError> {
    let reached = reached.clone();
    tokio::task::spawn_blocking(move || reached.wait()).await?;
    let release = release.clone();
    tokio::task::spawn_blocking(move || release.wait()).await?;
    Ok(())
}
