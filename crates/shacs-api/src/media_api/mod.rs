mod adapter;
mod request;
mod stream;
mod websocket;

pub use adapter::ChatCompletionAdapter;
pub use request::handle_api_request;
pub use stream::stream_event_frame;
pub(crate) use websocket::dispatch_websocket_frame;

pub const MEDIA_DIAGNOSTICS_PATH: &str = "/v1/media/diagnostics";
