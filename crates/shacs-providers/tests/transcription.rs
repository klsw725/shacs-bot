use serde_json::json;
use shacs_providers::{
    build_audio_transcription_request, parse_transcription_response, resolve_transcription_api_url,
    AudioTranscriptionHttpResponse, AudioTranscriptionRequestParts, GroqTranscriptionClient,
    ProviderError, TranscriptionClient, TranscriptionRequest,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn resolve_transcription_url_accepts_full_endpoint_or_base() -> Result<(), Box<dyn Error>> {
    let joined = resolve_transcription_api_url(
        Some("https://api.groq.com/openai/v1/"),
        None,
        "https://fallback.invalid/audio/transcriptions",
    );
    let full = resolve_transcription_api_url(
        Some("https://example.test/v1/audio/transcriptions"),
        None,
        "https://fallback.invalid/audio/transcriptions",
    );
    let env_fallback = resolve_transcription_api_url(
        None,
        Some("https://env.example/v1"),
        "https://fallback.invalid/audio/transcriptions",
    );
    if joined != "https://api.groq.com/openai/v1/audio/transcriptions"
        || full != "https://example.test/v1/audio/transcriptions"
        || env_fallback != "https://env.example/v1/audio/transcriptions"
    {
        return Err(format!(
            "transcription URL resolution drifted: joined={joined} full={full} env={env_fallback}"
        )
        .into());
    }
    Ok(())
}

#[test]
fn build_audio_transcription_request_uses_openai_compatible_multipart() -> Result<(), Box<dyn Error>>
{
    let parts = build_audio_transcription_request(
        "https://api.example/v1/audio/transcriptions",
        "sk-test",
        "voice.ogg",
        b"audio-bytes",
        "whisper-large-v3",
        Some("ko"),
        Some("names are Korean"),
    );
    let body = String::from_utf8_lossy(&parts.body);
    if parts.url != "https://api.example/v1/audio/transcriptions"
        || parts.headers.get("Authorization").map(String::as_str) != Some("Bearer sk-test")
        || !parts
            .content_type
            .starts_with("multipart/form-data; boundary=shacs-bot-")
        || !body.contains("name=\"file\"; filename=\"voice.ogg\"")
        || !body.contains("audio-bytes")
        || !body.contains("name=\"model\"\r\n\r\nwhisper-large-v3")
        || !body.contains("name=\"response_format\"\r\n\r\njson")
        || !body.contains("name=\"language\"\r\n\r\nko")
        || !body.contains("name=\"prompt\"\r\n\r\nnames are Korean")
    {
        return Err(format!("multipart request drifted: parts={parts:?} body={body:?}").into());
    }
    Ok(())
}

#[test]
fn groq_transcription_client_posts_file_and_returns_text() -> Result<(), Box<dyn Error>> {
    let audio_path = temp_audio_path()?;
    fs::write(&audio_path, b"audio")?;
    let captured = Arc::new(Mutex::new(Vec::<AudioTranscriptionRequestParts>::new()));
    let captured_transport = captured.clone();
    let client = GroqTranscriptionClient::new(
        "gsk-test",
        "https://api.groq.com/openai/v1/audio/transcriptions",
        Some("ko".to_owned()),
        move |request: AudioTranscriptionRequestParts| {
            captured_transport
                .lock()
                .map_err(|error| ProviderError::Api {
                    status: None,
                    message: error.to_string(),
                    retryable: false,
                    headers: BTreeMap::new(),
                    body: None,
                })?
                .push(request);
            Ok(AudioTranscriptionHttpResponse {
                status: 200,
                headers: BTreeMap::new(),
                body: json!({"text": "안녕하세요"}).to_string(),
            })
        },
    );

    let mut request = TranscriptionRequest::new(audio_path.clone());
    request.prompt = Some("short greeting".to_owned());
    let text = client.transcribe(request)?;
    let _ = fs::remove_file(&audio_path);
    let captured = captured.lock().map_err(|error| error.to_string())?;
    let parts = captured.first().ok_or("missing transcription request")?;
    let body = String::from_utf8_lossy(&parts.body);
    if text != "안녕하세요"
        || !body.contains("name=\"model\"\r\n\r\nwhisper-large-v3")
        || !body.contains("name=\"language\"\r\n\r\nko")
        || !body.contains("short greeting")
    {
        return Err(format!("transcription client drifted: text={text:?} parts={parts:?}").into());
    }
    Ok(())
}

#[test]
fn parse_transcription_error_preserves_retryability_and_message() -> Result<(), Box<dyn Error>> {
    let error = match parse_transcription_response(AudioTranscriptionHttpResponse {
        status: 429,
        headers: BTreeMap::from([("retry-after".to_owned(), "2".to_owned())]),
        body: json!({"error": {"message": "rate limited"}}).to_string(),
    }) {
        Ok(value) => return Err(format!("expected error, got {value:?}").into()),
        Err(error) => error,
    };
    match error {
        ProviderError::Api {
            status,
            message,
            retryable,
            headers,
            ..
        } if status == Some(429)
            && message == "rate limited"
            && retryable
            && headers.get("retry-after").map(String::as_str) == Some("2") => {}
        other => return Err(format!("unexpected transcription error: {other:?}").into()),
    }
    Ok(())
}

fn temp_audio_path() -> Result<PathBuf, Box<dyn Error>> {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    Ok(std::env::temp_dir().join(format!("shacs-transcription-{nanos}.ogg")))
}
