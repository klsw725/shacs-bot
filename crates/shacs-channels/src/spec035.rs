use crate::{metadata_bool, metadata_string, metadata_truthy, OutboundMessage};
use crate::{project_spec031_channel_event, ChannelSpec031ProjectionInput};
use serde::Serialize;
use shacs_projection::{
    project_spec035_revised_owner_facts, Spec030RuntimeProjection, Spec030UnavailableReason,
    Spec031ConstructionError, Spec031Envelope, Spec031ProgressDelivery, Spec035MediaProjection,
    Spec035MediaState, Spec035OwnerSurface, Spec035RevisedOwnerFacts, Spec035RevisedParseError,
    Spec035RevisedProjection, Spec035TransportCapability, Spec035TransportClientHello,
    Spec035TransportClientHelloInput, Spec035TransportClientId, Spec035TransportGeneration,
    Spec035TransportMutationRejection, Spec035TransportNegotiationError, Spec035TransportOffer,
    Spec035TransportOfferInput, Spec035TransportSchemaVersion,
};
use std::fmt::{Display, Formatter};

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

pub fn project_spec035_revised_json_for_channel(
    input: &str,
) -> Result<Spec035RevisedProjection, Spec035RevisedParseError> {
    Spec035RevisedProjection::parse_json(input)
}

pub fn project_spec035_revised_channel_event(
    trusted_runtime: &Spec030RuntimeProjection,
    delivery: &Spec031Envelope,
) -> Result<Spec035RevisedProjection, Spec031ConstructionError> {
    let surface = if delivery
        .lineage()
        .subject_ref
        .as_str()
        .starts_with("subject:channel:websocket:")
    {
        Spec035OwnerSurface::Websocket
    } else {
        Spec035OwnerSurface::Channel
    };
    project_spec035_revised_owner_facts(
        Spec035RevisedOwnerFacts::new(surface, trusted_runtime).with_delivery(delivery),
    )
}

pub(crate) fn project_successful_outbound(
    message: &OutboundMessage,
) -> Result<Spec035RevisedProjection, Spec031ConstructionError> {
    let input = if metadata_truthy(&message.metadata, "_stream_delta") {
        ChannelSpec031ProjectionInput::progress_delivery(
            &message.channel,
            Spec031ProgressDelivery::Live,
            metadata_string(&message.metadata, "_stream_id").as_deref(),
        )
    } else if metadata_bool(&message.metadata, "_stream_end") {
        ChannelSpec031ProjectionInput::progress_delivery(
            &message.channel,
            Spec031ProgressDelivery::FinalPending,
            metadata_string(&message.metadata, "_stream_id").as_deref(),
        )
    } else {
        ChannelSpec031ProjectionInput::external_final(message.clone())
    };
    let delivery = project_spec031_channel_event(input)?;
    project_spec035_revised_channel_event(
        &Spec030RuntimeProjection::unavailable(Spec030UnavailableReason::OwnerFactsMissing),
        &delivery,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelSpec035MutationError(Spec035TransportMutationRejection);

impl ChannelSpec035MutationError {
    pub const fn status(self) -> &'static str {
        self.0.status().as_str()
    }

    pub const fn reason(self) -> &'static str {
        self.0.reason().as_str()
    }

    pub const fn rejection(self) -> Spec035TransportMutationRejection {
        self.0
    }
}

impl Display for ChannelSpec035MutationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl std::error::Error for ChannelSpec035MutationError {}

pub fn negotiate_spec035_channel_hello(
    client: &Spec035TransportClientHello,
) -> Result<shacs_projection::Spec035TransportServerHello, ChannelSpec035MutationError> {
    channel_offer()?.select(client).map_err(|_| {
        ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch())
    })
}

pub(crate) fn dispatch_spec035_channel_mutation<T>(
    client: &Spec035TransportClientHello,
    capability: Spec035TransportCapability,
    handler: impl FnOnce() -> T,
) -> Result<T, ChannelSpec035MutationError> {
    match channel_offer()?.negotiate_mutation(client, capability) {
        Ok(_) => Ok(handler()),
        Err(Spec035TransportNegotiationError::SchemaMismatch(_)) => Err(
            ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch()),
        ),
        Err(Spec035TransportNegotiationError::Unsupported(reason)) => match reason {
            shacs_projection::Spec035TransportUnsupportedReason::CapabilityUnavailable => {
                Err(ChannelSpec035MutationError(
                    Spec035TransportMutationRejection::capability_unavailable(),
                ))
            }
        },
    }
}

pub fn spec035_channel_worker_hello(
) -> Result<Spec035TransportClientHello, ChannelSpec035MutationError> {
    let client_id = Spec035TransportClientId::try_new("client:channel-worker").map_err(|_| {
        ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch())
    })?;
    let schema = Spec035TransportSchemaVersion::try_new(1).map_err(|_| {
        ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch())
    })?;
    Spec035TransportClientHello::try_new(Spec035TransportClientHelloInput {
        client_id,
        schema_versions: vec![schema],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskResume,
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskRecover,
        ],
        resume: None,
    })
    .map_err(|_| ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch()))
}

fn channel_offer() -> Result<Spec035TransportOffer, ChannelSpec035MutationError> {
    let schema = Spec035TransportSchemaVersion::try_new(1).map_err(|_| {
        ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch())
    })?;
    let generation =
        Spec035TransportGeneration::try_new("generation:channel-local").map_err(|_| {
            ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch())
        })?;
    Spec035TransportOffer::try_new(Spec035TransportOfferInput {
        schema_versions: vec![schema],
        mutation_capabilities: vec![
            Spec035TransportCapability::TaskPause,
            Spec035TransportCapability::TaskResume,
            Spec035TransportCapability::TaskStop,
            Spec035TransportCapability::TaskRecover,
        ],
        generation,
    })
    .map_err(|_| ChannelSpec035MutationError(Spec035TransportMutationRejection::schema_mismatch()))
}
