use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapResponse {
    pub token: String,
    pub ws_path: String,
    pub expires_in: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderOption {
    pub name: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSettings {
    pub model: String,
    pub provider: String,
    pub resolved_provider: Option<String>,
    pub has_api_key: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSettings {
    pub config_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsPayload {
    pub agent: AgentSettings,
    pub providers: Vec<ProviderOption>,
    pub runtime: RuntimeSettings,
    pub requires_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SettingsUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaKind {
    Image,
    Video,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaUrl {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub kind: MediaKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum InboundEvent {
    Ready {
        chat_id: String,
        client_id: String,
    },
    Attached {
        chat_id: String,
    },
    Message {
        chat_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply_to: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media_urls: Vec<MediaUrl>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        buttons: Vec<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        button_prompt: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
    },
    Delta {
        chat_id: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
    },
    StreamEnd {
        chat_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stream_id: Option<String>,
    },
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundMedia {
    pub data_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundEvent {
    NewChat,
    Attach {
        chat_id: String,
    },
    Message {
        chat_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        media: Vec<OutboundMedia>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMessagesPayload {
    pub key: String,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub messages: Vec<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn protocol_serializes_webui_wire_shapes() -> Result<(), serde_json::Error> {
        let boot = BootstrapResponse {
            token: "nbwt_token".to_owned(),
            ws_path: "/".to_owned(),
            expires_in: 30,
            model_name: Some("model".to_owned()),
        };
        assert_eq!(serde_json::to_value(&boot)?["ws_path"], "/");

        let outbound = OutboundEvent::Message {
            chat_id: "chat".to_owned(),
            content: "hi".to_owned(),
            media: vec![OutboundMedia {
                data_url: "data:image/png;base64,AA==".to_owned(),
                name: Some("a.png".to_owned()),
            }],
        };
        assert_eq!(serde_json::to_value(outbound)?["type"], "message");

        let inbound: InboundEvent = serde_json::from_value(json!({
            "event": "ready",
            "chat_id": "chat",
            "client_id": "client"
        }))?;
        assert!(matches!(inbound, InboundEvent::Ready { .. }));
        Ok(())
    }
}
