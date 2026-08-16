use serde::Serialize;
use shacs_projection::{Spec035MediaProjection, Spec035MediaState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSpec035MediaDelivery {
    Pending,
    Unknown,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ChannelSpec035MediaProjection {
    media_capability: Spec035MediaProjection,
    delivery_status: ChannelSpec035MediaDelivery,
}

impl ChannelSpec035MediaProjection {
    pub const fn media_capability(&self) -> &Spec035MediaProjection {
        &self.media_capability
    }

    pub const fn delivery_status(&self) -> ChannelSpec035MediaDelivery {
        self.delivery_status
    }
}

pub fn project_spec035_media_for_channel(
    media_capability: Spec035MediaProjection,
) -> ChannelSpec035MediaProjection {
    let delivery_status = match media_capability.state() {
        Spec035MediaState::Included | Spec035MediaState::Truncated => {
            ChannelSpec035MediaDelivery::Pending
        }
        Spec035MediaState::Unsupported | Spec035MediaState::ExtractionFailed => {
            ChannelSpec035MediaDelivery::Unknown
        }
        Spec035MediaState::AnalyzerMissing | Spec035MediaState::Unavailable => {
            ChannelSpec035MediaDelivery::Unavailable
        }
    };
    ChannelSpec035MediaProjection {
        media_capability,
        delivery_status,
    }
}
