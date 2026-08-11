pub mod clients;
pub mod config;
pub mod error;
pub mod model;
pub mod provider;
pub mod registry;
pub mod retry;
pub mod transform;
pub mod types;

pub use clients::anthropic::{
    anthropic_client_from_config, build_anthropic_headers, build_anthropic_messages_request,
    parse_anthropic_response, parse_anthropic_stream, AnthropicClient, AnthropicHttpResponse,
    AnthropicHttpStreamResponse, AnthropicHttpTransport, AnthropicRequestParts,
    UreqAnthropicHttpTransport,
};
pub use clients::azure_openai::{
    azure_openai_client_from_config, build_azure_openai_headers,
    build_azure_openai_responses_request, resolve_azure_openai_api_base, AzureOpenAiClient,
};
pub use clients::codex::{
    build_codex_headers, build_codex_responses_request, codex_client_from_config,
    parse_codex_stream, CodexClient, CodexHttpStreamResponse, CodexHttpTransport,
    CodexRequestParts, UreqCodexHttpTransport,
};
pub use clients::image_generation::{
    build_openai_image_generation_request, build_openrouter_image_generation_request,
    image_generation_client_from_config, openai_image_generation_capability,
    openai_image_generation_client_from_config, openrouter_image_generation_client_from_config,
    parse_openai_image_generation_response, parse_openrouter_image_generation_response,
    resolve_image_generation_api_base, resolve_image_generation_client,
    DefaultModelImageGenerationClient, GeneratedImage, ImageGenerationCapability,
    ImageGenerationClient, ImageGenerationHttpResponse, ImageGenerationHttpTransport,
    ImageGenerationRequest, ImageGenerationRequestParts, ImageGenerationResult,
    OpenAiImageGenerationClient, OpenRouterImageGenerationClient, ResolvedImageGenerationClient,
    UreqImageGenerationHttpTransport,
};
pub use clients::openai_compatible::{
    build_chat_completions_request, build_chat_completions_stream_request, build_headers,
    build_responses_request, chat_completions_tool, merge_json_objects,
    normalize_chat_finish_reason, openai_compatible_client_from_config,
    parse_chat_completions_response, parse_chat_completions_stream,
    parse_openai_responses_response, parse_openai_responses_stream,
    resolve_openai_compatible_api_base, OpenAiApiKind, OpenAiCompatibleClient,
    OpenAiCompatibleRequestParts, OpenAiHttpResponse, OpenAiHttpStreamResponse,
    OpenAiHttpTransport, UreqOpenAiHttpTransport,
};
pub use clients::transcription::{
    build_audio_transcription_request, groq_transcription_client_from_config,
    openai_transcription_client_from_config, parse_transcription_response,
    resolve_transcription_api_url, transcription_client_from_config,
    AudioTranscriptionHttpResponse, AudioTranscriptionHttpTransport,
    AudioTranscriptionRequestParts, GroqTranscriptionClient, OpenAiTranscriptionClient,
    TranscriptionClient, TranscriptionRequest, UreqAudioTranscriptionHttpTransport,
};
pub use clients::{
    prepare_provider_request, provider_client_from_config, resolve_provider_client,
    ResolvedProviderClient,
};
pub use config::{interpolate_env, AgentDefaults, ProviderConfig, ProvidersConfig};
pub use error::ProviderError;
pub use model::{ModelCapabilities, ModelCost, ModelInfo, ModelLimits, ModelModalities};
pub use provider::{Provider, ProviderClient, ProviderEvent, ProviderInvocation, ProviderRequest};
pub use registry::{find_by_name, provider_specs, ProviderMatch, ProviderRegistry, ProviderSpec};
pub use retry::{
    chat_stream_with_retry, chat_stream_with_retry_using_waiter, chat_with_retry,
    chat_with_retry_using_waiter, is_transient_provider_error, is_transient_response,
    retry_after_from_response, retry_decision_for_error,
    retry_decision_for_error_with_identical_count, retry_decision_for_response,
    ProviderRetryDecision, ProviderRetryMode, ProviderRetryWaiter, RetryStopReason,
    ThreadRetryWaiter,
};
pub use transform::{IdentityTransform, RequestTransform, ToolSchemaTransform};
pub use types::{
    finish_reason_from_openai_responses, GenerationSettings, LlmResponse, ToolCallRequest,
};
