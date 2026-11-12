use super::{
    discord_message_body, discord_message_chunks, json_id_string, post_json, read_json_response,
    redact_sensitive_url_text,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use shacs_channels::OutboundMessage;
use shacs_utils::attachments::{detect_attachment_mime, sanitize_attachment_filename};
use std::fs;
use std::path::Path;

const DISCORD_API_BASE: &str = "https://discord.com/api/v10";

struct DiscordAttachment {
    filename: String,
    mime_type: String,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct DiscordMultipartBody {
    content_type: String,
    bytes: Vec<u8>,
}

struct DiscordClient<'a> {
    agent: &'a ureq::Agent,
    token: &'a str,
    api_base: &'a str,
}

impl<'a> DiscordClient<'a> {
    const fn new(agent: &'a ureq::Agent, token: &'a str, api_base: &'a str) -> Self {
        Self {
            agent,
            token,
            api_base,
        }
    }

    /// Sends the message and returns the id Discord gave each part of it.
    fn send(&self, message: OutboundMessage) -> Result<Vec<String>, String> {
        let url = format!(
            "{}/channels/{}/messages",
            self.api_base.trim_end_matches('/'),
            message.chat_id
        );
        let authorization = format!("Bot {}", self.token);
        let chunks = discord_message_chunks(&message.content);
        let mut ids = Vec::new();
        if message.media.is_empty() {
            for (index, chunk) in chunks.into_iter().enumerate() {
                let reply_to = (index == 0)
                    .then_some(message.reply_to.as_deref())
                    .flatten();
                let body = discord_message_body(&message.chat_id, &chunk, reply_to);
                ids.push(sent_message_id(&post_json(
                    self.agent,
                    &url,
                    Some(authorization.clone()),
                    body,
                )?)?);
            }
            return Ok(ids);
        }

        let first_chunk = chunks.first().map(String::as_str).unwrap_or_default();
        let multipart = build_discord_multipart(&message, first_chunk)?;
        let request = self
            .agent
            .post(&url)
            .header("Authorization", &authorization)
            .header("Content-Type", &multipart.content_type);
        ids.push(sent_message_id(&read_json_response(
            request.send(multipart.bytes).map_err(|error| {
                format!(
                    "request to {} failed: {}",
                    redact_sensitive_url_text(&url),
                    redact_sensitive_url_text(&error.to_string())
                )
            })?,
        )?)?);

        for chunk in chunks.into_iter().skip(1) {
            let body = discord_message_body(&message.chat_id, &chunk, None);
            ids.push(sent_message_id(&post_json(
                self.agent,
                &url,
                Some(authorization.clone()),
                body,
            )?)?);
        }
        Ok(ids)
    }
}

/// A send Discord accepted names the message it made. One that does not is a failed send:
/// a reply to it could not be told from a reply to another message.
fn sent_message_id(response: &Value) -> Result<String, String> {
    response
        .get("id")
        .and_then(json_id_string)
        .ok_or_else(|| "Discord create message response missing id".to_owned())
}

pub(super) fn send_message(
    agent: &ureq::Agent,
    token: &str,
    message: OutboundMessage,
) -> Result<Vec<String>, String> {
    DiscordClient::new(agent, token, DISCORD_API_BASE).send(message)
}

fn build_discord_multipart(
    message: &OutboundMessage,
    content: &str,
) -> Result<DiscordMultipartBody, String> {
    let attachments = message
        .media
        .iter()
        .map(|item| read_attachment(Path::new(item)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut payload = discord_message_body(&message.chat_id, content, message.reply_to.as_deref());
    payload["attachments"] = json!(attachments
        .iter()
        .enumerate()
        .map(|(id, attachment)| json!({
            "id": id,
            "filename": attachment.filename,
        }))
        .collect::<Vec<_>>());
    let payload_json = serde_json::to_vec(&payload).map_err(|error| error.to_string())?;
    let boundary = multipart_boundary(&payload_json, &attachments);
    let mut bytes = Vec::new();
    append_text(
        &mut bytes,
        &format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"\r\nContent-Type: application/json\r\n\r\n"
        ),
    );
    bytes.extend_from_slice(&payload_json);
    append_text(&mut bytes, "\r\n");
    for (index, attachment) in attachments.iter().enumerate() {
        append_text(
            &mut bytes,
            &format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"files[{index}]\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                attachment.filename, attachment.mime_type
            ),
        );
        bytes.extend_from_slice(&attachment.bytes);
        append_text(&mut bytes, "\r\n");
    }
    append_text(&mut bytes, &format!("--{boundary}--\r\n"));
    Ok(DiscordMultipartBody {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        bytes,
    })
}

fn read_attachment(path: &Path) -> Result<DiscordAttachment, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("could not read Discord attachment: {error}"))?;
    let raw_filename = path.file_name().and_then(|value| value.to_str());
    let filename = sanitize_attachment_filename(raw_filename).replace('"', "_");
    let mime_type = detect_attachment_mime(&bytes, None, Some(&filename))
        .detected_mime
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    Ok(DiscordAttachment {
        filename,
        mime_type,
        bytes,
    })
}

fn multipart_boundary(payload: &[u8], attachments: &[DiscordAttachment]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    for attachment in attachments {
        hasher.update(&attachment.bytes);
    }
    format!("shacs-bot-{:x}", hasher.finalize())
}

fn append_text(buffer: &mut Vec<u8>, value: &str) {
    buffer.extend_from_slice(value.as_bytes());
}

#[cfg(test)]
#[path = "discord_outbound_tests.rs"]
mod tests;
