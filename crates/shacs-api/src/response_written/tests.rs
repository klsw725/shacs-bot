use super::*;
use crate::{
    serve_api_listener, ApiError, ChatCompletionAdapter, ChatCompletionInvocation,
    CHAT_COMPLETIONS_PATH,
};
use shacs_providers::types::text_response;
use shacs_providers::{LlmResponse, ProviderEvent};
use std::error::Error;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

/// Far more than the socket buffers of a loopback connection hold, so a client that
/// stops reading leaves the server in the middle of its write.
const UNREAD_BODY_BYTES: usize = 48 * 1024 * 1024;

struct ReportingAdapter {
    content: String,
    written: Arc<AtomicUsize>,
}

impl ReportingAdapter {
    fn new(content: String) -> (Arc<Self>, Arc<AtomicUsize>) {
        let written = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(Self {
                content,
                written: written.clone(),
            }),
            written,
        )
    }

    fn report(&self) -> Option<ChatResponseWritten> {
        let written = self.written.clone();
        Some(Box::new(move || {
            written.fetch_add(1, Ordering::SeqCst);
        }))
    }
}

impl ChatCompletionAdapter for ReportingAdapter {
    fn configured_model(&self) -> &str {
        "gpt-5"
    }

    fn complete_chat(&self, _: ChatCompletionInvocation) -> Result<LlmResponse, ApiError> {
        Ok(text_response(&self.content))
    }

    fn complete_chat_reporting(
        &self,
        invocation: ChatCompletionInvocation,
    ) -> Result<(LlmResponse, Option<ChatResponseWritten>), ApiError> {
        Ok((self.complete_chat(invocation)?, self.report()))
    }

    fn stream_chat_reporting(
        &self,
        invocation: ChatCompletionInvocation,
        on_event: &mut dyn FnMut(ProviderEvent),
    ) -> Result<(LlmResponse, Option<ChatResponseWritten>), ApiError> {
        Ok((self.stream_chat(invocation, on_event)?, self.report()))
    }
}

fn counter() -> (Arc<AtomicUsize>, ChatResponseWritten) {
    let count = Arc::new(AtomicUsize::new(0));
    let written = count.clone();
    (
        count,
        Box::new(move || {
            written.fetch_add(1, Ordering::SeqCst);
        }),
    )
}

fn chat_request(stream: bool) -> String {
    let body = format!(r#"{{"messages":[{{"role":"user","content":"hello"}}],"stream":{stream}}}"#);
    format!(
        "POST {CHAT_COMPLETIONS_PATH} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn waiter_runs_at_the_flush_after_its_bytes_and_not_before() -> Result<(), Box<dyn Error>> {
    let (server, mut client) = tokio::io::duplex(8);
    let mut io = WriteObservedIo::new(server);
    let (count, written) = counter();
    io.writes().after_next_flush(written);
    // 32 bytes do not fit an 8-byte pipe: the write cannot end before the reader drains.
    let writer = tokio::spawn(async move {
        io.write_all(&[7; 32]).await?;
        io.flush().await?;
        Ok::<_, io::Error>(io)
    });

    let mut part = [0; 8];
    client.read_exact(&mut part).await?;
    assert_eq!(count.load(Ordering::SeqCst), 0);

    let mut rest = [0; 24];
    client.read_exact(&mut rest).await?;
    let mut io = writer.await??;
    assert_eq!(count.load(Ordering::SeqCst), 1);

    io.flush().await?;
    assert_eq!(count.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn waiter_of_a_connection_that_ends_before_its_flush_never_runs() {
    let (server, client) = tokio::io::duplex(8);
    let io = WriteObservedIo::new(server);
    let (count, written) = counter();
    io.writes().after_next_flush(written);

    drop(io);
    drop(client);

    assert_eq!(count.load(Ordering::SeqCst), 0);
}

async fn exchange(
    adapter: Arc<ReportingAdapter>,
    request: String,
    read_all: bool,
) -> Result<String, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(serve_api_listener(listener, adapter.clone(), async {
        let _ = shutdown_rx.await;
    }));
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    if read_all {
        stream.read_to_string(&mut response).await?;
    } else {
        // The first bytes show that the server is writing the response. The rest is left
        // unread, and the connection is given up.
        let mut head = [0; 64];
        stream.read_exact(&mut head).await?;
        assert_eq!(adapter.written.load(Ordering::SeqCst), 0);
        response.push_str(&String::from_utf8_lossy(&head));
    }
    drop(stream);
    let _ = shutdown_tx.send(());
    server.await??;
    Ok(response)
}

#[tokio::test]
async fn json_chat_response_reports_after_the_client_got_all_of_it() -> Result<(), Box<dyn Error>> {
    let (adapter, written) = ReportingAdapter::new("answer".to_owned());

    let response = exchange(adapter, chat_request(false), true).await?;

    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or("response has no body")?;
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head
        .to_ascii_lowercase()
        .contains(&format!("content-length: {}", body.len())));
    assert!(!head.to_ascii_lowercase().contains("transfer-encoding"));
    let body: serde_json::Value = serde_json::from_str(body)?;
    assert_eq!(body["choices"][0]["message"]["content"], "answer");
    assert_eq!(written.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn json_chat_response_left_unread_never_reports() -> Result<(), Box<dyn Error>> {
    let (adapter, written) = ReportingAdapter::new("a".repeat(UNREAD_BODY_BYTES));

    let head = exchange(adapter, chat_request(false), false).await?;

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(written.load(Ordering::SeqCst), 0);
    Ok(())
}

#[tokio::test]
async fn streamed_chat_response_reports_after_its_end_was_written() -> Result<(), Box<dyn Error>> {
    let (adapter, written) = ReportingAdapter::new("answer".to_owned());

    let response = exchange(adapter, chat_request(true), true).await?;

    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains("data: [DONE]"));
    assert_eq!(written.load(Ordering::SeqCst), 1);
    Ok(())
}

#[tokio::test]
async fn streamed_chat_response_left_unread_never_reports() -> Result<(), Box<dyn Error>> {
    let (adapter, written) = ReportingAdapter::new("a".repeat(UNREAD_BODY_BYTES));

    let head = exchange(adapter, chat_request(true), false).await?;

    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert_eq!(written.load(Ordering::SeqCst), 0);
    Ok(())
}
