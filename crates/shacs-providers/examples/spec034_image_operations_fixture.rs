use serde_json::json;
use shacs_providers::{
    ImageEditRequest, ImageFileInput, ImageGenerationClient, ImageMaskRequest,
    ImageOperationRequest, ImageOperationResult, OpenAiImageGenerationClient,
    UreqImageGenerationHttpTransport,
};
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

type FixtureError = Box<dyn Error + Send + Sync>;

fn main() -> Result<(), FixtureError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = thread::spawn(move || serve_fixture(listener));
    let transport = UreqImageGenerationHttpTransport::with_timeout(
        format!("http://{address}/v1"),
        Duration::from_secs(5),
    );
    let client = OpenAiImageGenerationClient::new(
        "fixture-token",
        format!("http://{address}/v1"),
        "gpt-image-2",
        transport,
    );

    let edit_source = ImageFileInput::new("source.png", "image/png", b"fixture-source".to_vec())?;
    let edit = client.execute_image_operation(ImageOperationRequest::Edit(
        ImageEditRequest::new("add a hat", edit_source),
    ))?;
    let mask_source = ImageFileInput::new("source.png", "image/png", b"fixture-source".to_vec())?;
    let mask = ImageFileInput::new("mask.png", "image/png", b"fixture-mask".to_vec())?;
    let masked = client.execute_image_operation(ImageOperationRequest::Mask(
        ImageMaskRequest::new("replace the sky", mask_source, mask),
    ))?;

    let observations = server
        .join()
        .map_err(|_| io::Error::other("fixture server panicked"))??;
    println!(
        "{}",
        json!({
            "editResult": result_name(&edit),
            "maskResult": result_name(&masked),
            "requests": observations,
        })
    );
    Ok(())
}

fn serve_fixture(listener: TcpListener) -> Result<Vec<serde_json::Value>, FixtureError> {
    let mut observations = Vec::new();
    for _ in 0..2 {
        let (stream, _) = listener.accept()?;
        observations.push(handle_request(stream)?);
    }
    Ok(observations)
}

fn handle_request(mut stream: TcpStream) -> Result<serde_json::Value, FixtureError> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    let mut content_type = String::new();
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = header_value(&line, "content-length") {
            content_length = value.parse()?;
        }
        if let Some(value) = header_value(&line, "content-type") {
            content_type = value.to_owned();
        }
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    let body_text = String::from_utf8_lossy(&body);
    let path = request_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request path"))?;
    let has_image = body_text.contains("name=\"image\"; filename=\"source.png\"");
    let has_mask = body_text.contains("name=\"mask\"; filename=\"mask.png\"");
    let valid = path == "/v1/images/edits"
        && content_type.starts_with("multipart/form-data; boundary=")
        && has_image
        && !body_text.contains("fixture-token");
    let response_body = if valid {
        r#"{"data":[{"b64_json":"Zml4dHVyZS1pbWFnZQ=="}]}"#
    } else {
        r#"{"error":{"message":"invalid fixture request"}}"#
    };
    let status = if valid { "200 OK" } else { "400 Bad Request" };
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
        response_body.len()
    )?;
    stream.flush()?;
    Ok(json!({
        "path": path,
        "multipart": valid,
        "sourcePart": has_image,
        "maskPart": has_mask,
        "bodyBytes": body.len(),
    }))
}

fn header_value<'a>(line: &'a str, expected_name: &str) -> Option<&'a str> {
    let (name, value) = line.split_once(':')?;
    name.eq_ignore_ascii_case(expected_name)
        .then(|| value.trim())
}

const fn result_name(result: &ImageOperationResult) -> &'static str {
    match result {
        ImageOperationResult::Generate(_) => "generate",
        ImageOperationResult::Edit(_) => "edit",
        ImageOperationResult::Mask(_) => "mask",
        ImageOperationResult::Variation(_) => "variation",
    }
}
