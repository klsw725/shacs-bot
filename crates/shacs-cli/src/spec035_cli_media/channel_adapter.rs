use super::super::*;
use shacs_channels::project_spec035_media_for_channel;

impl ExternalTransportRuntimeContext {
    pub(crate) fn media_projection(
        &self,
    ) -> Result<Option<shacs_channels::ChannelSpec035MediaProjection>, ChannelError> {
        let Some(data_dir) = self.durable_data_dir.as_ref() else {
            return Ok(None);
        };
        shacs_core::runtime::Spec035MediaProjectionStore::new(data_dir)
            .read()
            .map(|projection| projection.map(project_spec035_media_for_channel))
            .map_err(|_| ChannelError::Delivery("media projection is invalid".to_owned()))
    }
}

impl ChannelAdapter for ExternalTransportChannelAdapter {
    fn name(&self) -> &str {
        &self.channel
    }

    fn supports_streaming(&self) -> bool {
        self.spec.supports_streaming()
    }

    fn start(&mut self) -> Result<(), ChannelError> {
        if self.handle.is_some() {
            return Ok(());
        }
        let spec = self.spec.clone();
        let inbound_bus = self.inbound_bus.clone();
        let transport_context = self.transport_context.clone();
        let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundMessage>();
        let runner = self.runner.clone();
        let stop = Arc::new(AtomicBool::new(false));
        self.outbound_tx = Some(outbound_tx);
        self.worker_stop = Some(stop.clone());
        self.handle = Some(thread::spawn(move || {
            runner(spec, inbound_bus, outbound_rx, stop, transport_context);
        }));
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ChannelError> {
        if let Some(stop) = self.worker_stop.take() {
            stop.store(true, Ordering::SeqCst);
        }
        self.outbound_tx = None;
        if let Some(handle) = self.handle.take() {
            handle.join().map_err(|_| {
                ChannelError::Delivery(format!("channel worker panicked: {}", self.channel))
            })?;
        }
        Ok(())
    }

    fn send(&self, mut message: OutboundMessage) -> Result<(), ChannelError> {
        if let Some(media) = self.transport_context.media_projection()? {
            message.metadata.insert(
                "media_capability".to_owned(),
                serde_json::to_value(media.media_capability())
                    .map_err(|error| ChannelError::Delivery(error.to_string()))?,
            );
            message.metadata.insert(
                "media_delivery_status".to_owned(),
                serde_json::to_value(media.delivery_status())
                    .map_err(|error| ChannelError::Delivery(error.to_string()))?,
            );
        }
        self.outbound_tx
            .as_ref()
            .ok_or_else(|| {
                ChannelError::Delivery(format!("channel worker is not started: {}", self.channel))
            })?
            .send(message)
            .map_err(|error| ChannelError::Delivery(error.to_string()))
    }

    fn send_delta(
        &self,
        chat_id: &str,
        delta: &str,
        metadata: Map<String, Value>,
    ) -> Result<(), ChannelError> {
        let reply_to = metadata_string(&metadata, "reply_to");
        let mut message =
            OutboundMessage::new(&self.channel, chat_id, delta).with_metadata(metadata);
        message.reply_to = reply_to;
        self.send(message)
    }
}
