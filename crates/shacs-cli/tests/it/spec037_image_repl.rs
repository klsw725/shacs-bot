use serde_json::json;
use shacs_config::{save_config_to_path, Config};
use shacs_providers::{LlmResponse, ToolCallRequest};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

/// A local OpenAI-compatible image API: answers every request with `status`
/// and, on 200, a real PNG of the requested size.
fn image_api(status: u16) -> Result<String, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap_or(0);
                }
                line.clear();
            }
            let mut body = vec![0; length];
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let size = request["size"].as_str().unwrap_or("64x64");
            let (width, height) = size.split_once('x').unwrap_or(("64", "64"));
            let image = png(width.parse().unwrap_or(64), height.parse().unwrap_or(64));
            let encoded = base64(&image);
            let payload = json!({"created": 0, "data": [{"b64_json": encoded}]}).to_string();
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                payload.len()
            );
            let _ = reader.get_mut().write_all(response.as_bytes());
        }
    });
    Ok(format!("http://{address}/v1"))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let bits = chunk.iter().enumerate().fold(0u32, |acc, (at, byte)| {
            acc | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            out.push(if at <= chunk.len() {
                TABLE[(bits >> (18 - 6 * at) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

fn png(width: u32, height: u32) -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        !bytes.iter().fold(!0u32, |mut crc, byte| {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    0xEDB8_8320 ^ (crc >> 1)
                } else {
                    crc >> 1
                };
            }
            crc
        })
    }
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let body = [kind, data].concat();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    };
    let row = 1 + 3 * width as usize;
    let raw: Vec<u8> = (0..height as usize * row)
        .map(|at| if at % row == 0 { 0 } else { 90 })
        .collect();
    let (a, b) = raw.iter().fold((1u32, 0u32), |(a, b), byte| {
        let a = (a + u32::from(*byte)) % 65_521;
        (a, (b + a) % 65_521)
    });
    let length = raw.len() as u16;
    let mut zlib = vec![0x78, 0x01, 0x01];
    zlib.extend(length.to_le_bytes());
    zlib.extend((!length).to_le_bytes());
    zlib.extend(&raw);
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut header = width.to_be_bytes().to_vec();
    header.extend(height.to_be_bytes());
    header.extend([8, 2, 0, 0, 0]);
    chunk(b"IHDR", &header);
    chunk(b"IDAT", &zlib);
    chunk(b"IEND", &[]);
    out
}

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    responses: PathBuf,
}
impl Fixture {
    fn new(api_base: &str, api_key: Option<&str>) -> Result<Self, Box<dyn Error>> {
        let root = tempfile::tempdir()?;
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace)?;
        let mut openai = json!({"apiBase": api_base});
        if let Some(key) = api_key {
            openai["apiKey"] = json!(key);
        }
        let config: Config = serde_json::from_value(json!({
            "agents":{"defaults":{"workspace":workspace,"provider":"custom","model":"test-model"}},
            "providers":{"custom":{"apiKey":"sk-isolated-fixture","apiBase":"http://127.0.0.1:1/v1"},"openai":openai},
            "tools":{"exec":{"enable":false},"web":{"enable":false},
                "imageGeneration":{"enable":true,"provider":"openai","model":"gpt-image-1"}}
        }))?;
        let config_path = root.path().join("config.json");
        save_config_to_path(&config, &config_path)?;
        let generate = LlmResponse {
            finish_reason: "tool_calls".to_owned(),
            tool_calls: vec![ToolCallRequest::new(
                "image-1",
                "image_generate",
                serde_json::Map::from_iter([
                    ("prompt".to_owned(), json!("a red square")),
                    ("size".to_owned(), json!("64x48")),
                    ("format".to_owned(), json!("png")),
                ]),
            )],
            ..LlmResponse::default()
        };
        let done = LlmResponse {
            content: Some("Here is your image.".to_owned()),
            ..LlmResponse::default()
        };
        let responses = root.path().join("responses.json");
        fs::write(&responses, serde_json::to_vec(&[generate, done])?)?;
        Ok(Self {
            root,
            config: config_path,
            responses,
        })
    }

    /// Asks for an image, approves the call, and returns the answer.
    fn generate(&self) -> Result<String, Box<dyn Error>> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_shacs-bot"))
            .args(["--config"])
            .arg(&self.config)
            .args(["agent", "--session", "spec037-image"])
            .env("SHACS_DEBUG_FAKE_PROVIDER_RESPONSES", &self.responses)
            .env("HOME", self.root.path())
            .env_remove("SHACS_RUNTIME_PACKAGE")
            .env_remove("OPENAI_API_KEY")
            .env_remove("OPENAI_IMAGE_GENERATION_BASE_URL")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().ok_or("missing stdin")?;
        let output = reader(&mut child)?;
        until_prompt(&output)?;
        writeln!(input, "Generate an image of a red square, 64x48 png")?;
        until_prompt(&output)?;
        writeln!(input, "approve")?;
        let answer = until_prompt(&output)?;
        drop(input);
        let result = child.wait_with_output()?;
        if !result.status.success() {
            return Err(String::from_utf8_lossy(&result.stderr).into_owned().into());
        }
        Ok(answer)
    }

    fn saved_images(&self) -> usize {
        let dir = self.root.path().join("media").join("image-generation");
        fs::read_dir(dir).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "png"))
                .count()
        })
    }
}

fn reader(child: &mut Child) -> Result<Receiver<Vec<u8>>, Box<dyn Error>> {
    let mut stdout = child.stdout.take().ok_or("missing stdout")?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = stdout.read(&mut bytes) {
            if count == 0 || tx.send(bytes[..count].to_vec()).is_err() {
                return;
            }
        }
    });
    Ok(rx)
}

fn until_prompt(output: &Receiver<Vec<u8>>) -> Result<String, Box<dyn Error>> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"shacs-bot> ") {
        bytes.extend(output.recv_timeout(Duration::from_secs(20))?);
    }
    Ok(String::from_utf8(bytes)?)
}

#[test]
fn configured_image_provider_saves_a_checked_image_through_the_cli() -> Result<(), Box<dyn Error>> {
    // Given
    let fixture = Fixture::new(&image_api(200)?, Some("sk-local-image-fixture"))?;
    // When
    let answer = fixture.generate()?;
    // Then
    assert!(
        answer.contains("Image: generated — 1 image saved and checked on disk"),
        "{answer}"
    );
    assert!(answer.contains("Images: media/image-generation/img-"));
    assert!(answer.contains("size 64x48 — met (measured 64x48); format png — met (measured png)"));
    assert!(answer.contains("content and look — not checked automatically"));
    assert!(answer.contains("Image saved and checked on disk (see below)."));
    assert!(!answer.contains("Here is your image."), "{answer}");
    assert_eq!(fixture.saved_images(), 1);
    Ok(())
}

#[test]
fn failing_or_unauthenticated_image_providers_never_show_a_generated_image(
) -> Result<(), Box<dyn Error>> {
    for (status, key, expected) in [
        (500, Some("sk-local-image-fixture"), "Image: failed — image_generate did not save an image (generation failed)"),
        (200, None, "Image: unavailable — image_generate did not save an image (provider or credentials unavailable)"),
    ] {
        // Given
        let fixture = Fixture::new(&image_api(status)?, key)?;
        // When
        let answer = fixture.generate()?;
        // Then
        assert!(answer.contains(expected), "{answer}");
        assert!(!answer.contains("Here is your image."), "{answer}");
        assert!(!answer.contains("Images:"));
        assert_eq!(fixture.saved_images(), 0);
    }
    Ok(())
}
