use super::*;
use serde_json::{Map, Value};
use shacs_channels::{OutboundMessage, DISCORD_CHANNEL};
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

#[test]
fn multipart_contains_image_and_document_when_media_is_present() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let image = root.path().join("preview.png");
    let document = root.path().join("summary.md");
    fs::write(&image, b"\x89PNG\r\n\x1a\nimage-bytes")?;
    fs::write(&document, b"# Summary")?;
    let message = OutboundMessage {
        channel: DISCORD_CHANNEL.to_owned(),
        chat_id: "channel-1".to_owned(),
        content: "artifacts".to_owned(),
        reply_to: Some("message-1".to_owned()),
        media: vec![
            image.to_string_lossy().into_owned(),
            document.to_string_lossy().into_owned(),
        ],
        metadata: Map::new(),
        buttons: Vec::new(),
    };

    let multipart = build_discord_multipart(&message, "artifacts")?;

    let body = String::from_utf8_lossy(&multipart.bytes);
    assert!(multipart
        .content_type
        .starts_with("multipart/form-data; boundary="));
    assert!(body.contains("name=\"payload_json\""));
    assert!(body.contains("name=\"files[0]\"; filename=\"preview.png\""));
    assert!(body.contains("Content-Type: image/png"));
    assert!(body.contains("name=\"files[1]\"; filename=\"summary.md\""));
    assert!(body.contains("# Summary"));
    assert!(body.contains("\"message_id\":\"message-1\""));
    assert!(body.contains("\"filename\":\"preview.png\""));
    assert!(body.contains("\"filename\":\"summary.md\""));
    assert!(body.ends_with("--\r\n"));
    Ok(())
}

#[test]
fn multipart_rejects_missing_media_file() {
    let message = OutboundMessage {
        channel: DISCORD_CHANNEL.to_owned(),
        chat_id: "channel-1".to_owned(),
        content: String::new(),
        reply_to: None,
        media: vec!["/missing/shacs-attachment.png".to_owned()],
        metadata: Map::<String, Value>::new(),
        buttons: Vec::new(),
    };

    let error =
        build_discord_multipart(&message, "").expect_err("missing media should prevent delivery");

    assert!(error.contains("could not read Discord attachment"));
}

#[test]
fn discord_client_posts_attachment_multipart_to_messages_endpoint() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let (request_tx, request_rx) = mpsc::channel();
    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|error| error.to_string())?;
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            let Some(header_end) = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map(|position| position + 4)
            else {
                continue;
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .ok_or_else(|| "request has no content-length".to_owned())?;
            if request.len() >= header_end + content_length {
                break;
            }
        }
        request_tx
            .send(request)
            .map_err(|error| error.to_string())?;
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            )
            .map_err(|error| error.to_string())
    });
    let root = tempfile::tempdir()?;
    let image = root.path().join("preview.png");
    fs::write(&image, b"\x89PNG\r\n\x1a\nimage-bytes")?;
    let mut message = OutboundMessage::new(DISCORD_CHANNEL, "channel-1", "preview");
    message.media.push(image.to_string_lossy().into_owned());
    let agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent();
    let api_base = format!("http://{address}");

    DiscordClient::new(&agent, "discord-token", &api_base).send(message)?;

    let request = request_rx.recv()?;
    server
        .join()
        .map_err(|_| "Discord test server panicked")??;
    let request = String::from_utf8_lossy(&request);
    assert!(request.starts_with("POST /channels/channel-1/messages HTTP/1.1\r\n"));
    assert!(request.contains("authorization: Bot discord-token\r\n"));
    assert!(request.contains("content-type: multipart/form-data; boundary="));
    assert!(request.contains("filename=\"preview.png\""));
    assert!(request.contains("image-bytes"));
    Ok(())
}
