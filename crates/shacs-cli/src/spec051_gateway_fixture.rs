//! Integration-test access to the production construction path, not a worker copy.
use crate::{
    AgentLoopChatCompletionAdapter, CliError, GatewayDurableWork, RuntimeOwnershipLease,
    SurfaceApprovalWorker,
};
use shacs_config::ConfigBundle;
use shacs_core::runtime::BrowserHost;
use shacs_providers::{LlmResponse, ProviderClient, ProviderError, ProviderEvent, ProviderRequest};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

struct ForbiddenProvider {
    calls: Arc<AtomicUsize>,
    replies: Mutex<VecDeque<LlmResponse>>,
}
impl ProviderClient for ForbiddenProvider {
    fn chat(&self, _request: ProviderRequest) -> Result<LlmResponse, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .replies
            .lock()
            .expect("fixture replies lock poisoned")
            .pop_front()
            .expect("a rendered watcher must not call a provider"))
    }
    fn chat_stream(
        &self,
        request: ProviderRequest,
        event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<LlmResponse, ProviderError> {
        let response = self.chat(request)?;
        for call in &response.tool_calls {
            event(ProviderEvent::ToolCallStart {
                id: call.id.clone(),
                name: call.name.clone(),
            });
            event(ProviderEvent::ToolCallReady {
                id: call.id.clone(),
                name: call.name.clone(),
                input: serde_json::Value::Object(call.arguments.clone()),
            });
        }
        Ok(response)
    }
}

pub struct Gateway {
    worker: Option<SurfaceApprovalWorker>,
    adapter: Arc<AgentLoopChatCompletionAdapter>,
    calls: Arc<AtomicUsize>,
    ownership: RuntimeOwnershipLease,
}

impl Gateway {
    pub fn start(bundle: ConfigBundle) -> Result<Self, CliError> {
        Self::start_with_replies(bundle, Vec::new())
    }

    pub fn start_with_replies(
        bundle: ConfigBundle,
        replies: Vec<LlmResponse>,
    ) -> Result<Self, CliError> {
        let ownership = RuntimeOwnershipLease::acquire(&bundle, "gateway")?;
        let data_dir = bundle.context.data_dir.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut adapter = AgentLoopChatCompletionAdapter::from_bundle(bundle, true)?;
        // Only the provider tripwire changes; browser and reader come from
        // production_tool_registry and are never inserted here.
        adapter.client = Arc::new(ForbiddenProvider {
            calls: calls.clone(),
            replies: Mutex::new(replies.into()),
        });
        let adapter = Arc::new(adapter);
        let work = GatewayDurableWork::new(data_dir, ownership.owner_id().to_owned());
        let worker = work.start_worker(adapter.clone())?;
        Ok(Self {
            worker: Some(worker),
            adapter,
            calls,
            ownership,
        })
    }

    pub fn host(&self) -> Option<&dyn BrowserHost> {
        self.adapter.browser_host.as_deref()
    }

    pub fn owner_id(&self) -> &str {
        self.ownership.owner_id()
    }

    pub fn provider_calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn say(&self, session: &str, text: &str) -> shacs_api::ApiHttpResponse {
        shacs_api::handle_chat_completions(
            shacs_api::ApiHttpRequest::post_json(
                "/v1/chat/completions",
                serde_json::json!({
                    "session_id":session,"messages":[{"role":"user","content":text}],"stream":false
                }),
            ),
            self.adapter.as_ref(),
        )
    }

    pub fn external_takeover(&self) -> Result<String, Box<dyn std::error::Error>> {
        use crate::{
            external_transport_channel_manager_with_runner, publish_external_inbound_with_hint,
            run_external_agent_processor_with_channels, ExternalChannelSupervisor,
            ExternalProcessorShutdownControl, ExternalTransportRunner,
            ExternalTransportRuntimeContext, ExternalTransportSpec, RuntimeShutdownReason,
            TelegramRuntimeConfig, TELEGRAM_CHANNEL,
        };
        use shacs_core::runtime::{InboundMessage, MessageBus};
        use std::sync::atomic::AtomicBool;
        use std::sync::mpsc;
        use std::time::Duration;

        let data = self.adapter.config_path.parent().ok_or("data directory")?;
        let bus = MessageBus::new();
        let mut context = ExternalTransportRuntimeContext::new(data.join("metadata"), 1);
        let gateway = GatewayDurableWork::new(data.to_path_buf(), self.owner_id().to_owned());
        let ingress = gateway.open_ingress(&mut context, &bus, None)?;
        let (delivered, receive) = mpsc::channel();
        let runner: ExternalTransportRunner = Arc::new(move |_, _, outbound, stop, _| {
            while !stop.load(Ordering::SeqCst) {
                if let Ok(message) = outbound.recv_timeout(Duration::from_millis(10)) {
                    if delivered.send(message).is_err() {
                        break;
                    }
                }
            }
        });
        let stop = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let processor = ExternalChannelSupervisor::spawn_processor(exited.clone(), {
            let (adapter, bus, context, stop) = (
                self.adapter.clone(),
                bus.clone(),
                context.clone(),
                stop.clone(),
            );
            move || {
                let hello = context.spec035_hello;
                let channels = external_transport_channel_manager_with_runner(
                    vec![ExternalTransportSpec::Telegram(TelegramRuntimeConfig {
                        token: "fixture".to_owned(),
                        poll_timeout_seconds: 1,
                        poll_limit: 1,
                    })],
                    bus.clone(),
                    1,
                    context,
                    runner,
                );
                run_external_agent_processor_with_channels(
                    adapter,
                    bus,
                    channels,
                    hello,
                    ExternalProcessorShutdownControl {
                        stop,
                        reason: Arc::new(Mutex::new(RuntimeShutdownReason::Stop)),
                        exited,
                        startup_tx: None,
                    },
                    ingress,
                )
            }
        });
        let admitted = publish_external_inbound_with_hint(
            &bus,
            InboundMessage::new(TELEGRAM_CHANNEL, "owner", "takeover", "/browser takeover"),
            &context.metadata_path("telegram"),
            &context,
        );
        let reply = if admitted {
            receive
                .recv_timeout(Duration::from_secs(5))
                .map_err(|e| e.to_string())
        } else {
            Err("external ingress refused takeover".to_owned())
        };
        stop.store(true, Ordering::SeqCst);
        processor
            .join()
            .map_err(|_| "external processor panicked")??;
        Ok(reply?.content)
    }

    pub fn stop(&mut self) -> Result<(), CliError> {
        if let Some(worker) = self.worker.take() {
            worker.stop()?;
        }
        Ok(())
    }
}
