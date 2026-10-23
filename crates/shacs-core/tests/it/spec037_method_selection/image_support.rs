use super::*;
use shacs_core::tools::{ImageGenerateTool, ImageGenerateToolConfig};
use shacs_providers::{
    GeneratedImage, ImageGenerationClient, ImageGenerationItemId, ImageGenerationRequest,
    ImageGenerationResult, ImageMimeType,
};

/// A deterministic image provider injected at the `image_generate` client
/// boundary: it returns a real PNG of the requested (or a fixed) size.
pub(super) struct Images {
    pub(super) provider_id: &'static str,
    pub(super) fixed: Option<(u32, u32)>,
    pub(super) error: Option<fn() -> ProviderError>,
}

impl ImageGenerationClient for Images {
    fn generate_image(
        &self,
        request: ImageGenerationRequest,
    ) -> Result<ImageGenerationResult, ProviderError> {
        if let Some(error) = self.error {
            return Err(error());
        }
        let requested = request.size.as_deref().and_then(|size| {
            let (width, height) = size.split_once('x')?;
            Some((width.parse().ok()?, height.parse().ok()?))
        });
        let (width, height) = self.fixed.or(requested).unwrap_or((64, 64));
        let bytes = png(width, height);
        Ok(ImageGenerationResult {
            provider_id: self.provider_id.to_owned(),
            model: "test-image-model".to_owned(),
            images: vec![GeneratedImage {
                index: 0,
                mime_type: ImageMimeType::Png,
                byte_len: bytes.len(),
                bytes,
                revised_prompt: None,
                provider_item_id: Some(ImageGenerationItemId::from_provider("ig_test_1")),
            }],
            remote_images: Vec::new(),
            usage: None,
            request_id: None,
        })
    }
}

/// A valid RGB PNG (stored deflate block) so every consumer can decode it.
pub(super) fn png(width: u32, height: u32) -> Vec<u8> {
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
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let body = [kind, data].concat();
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let row = 1 + 3 * width as usize;
    let raw: Vec<u8> = (0..height as usize * row)
        .map(|at| if at % row == 0 { 0 } else { 200 })
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
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

pub(super) fn image_tools(media_root: &Path, images: Images) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(AskUserTool::new());
    add_image(&mut tools, media_root, images);
    tools
}

pub(super) fn add_image(tools: &mut ToolRegistry, media_root: &Path, images: Images) {
    let codex = images.provider_id == "openai_codex";
    let mut tool = ImageGenerateTool::new(
        Box::new(images),
        media_root.join("image-generation"),
        ImageGenerateToolConfig {
            provider_id: "auto".to_owned(),
            model_id: "test-image-model".to_owned(),
            default_format: "png".to_owned(),
            max_count: 2,
            max_bytes: 1 << 20,
        },
    );
    if codex {
        let store = shacs_core::generated_media::ArtifactStore::open(media_root);
        tool = tool.with_artifact_store(store.expect("artifact store"));
    }
    tools.register(tool);
}

pub(super) fn image_runtime<'a>(
    workspace: &Path,
    media_roots: Vec<std::path::PathBuf>,
    tools: &'a ToolRegistry,
    provider: &'a Provider,
) -> Result<AgentLoop<'a>, Box<dyn Error>> {
    let mut config = AgentLoopConfig::new(workspace, "test-model");
    config.permission_interactive = true;
    config.media_roots = media_roots;
    Ok(AgentLoop::new(
        MessageBus::new(),
        SessionManager::new(workspace)?,
        ContextBuilder::new(workspace),
        tools,
        provider,
        config,
    ))
}

pub(super) fn generate(size: &str) -> LlmResponse {
    call(
        "image_generate",
        json!({"prompt": "a red square", "size": size, "format": "png"}),
    )
}

pub(super) fn ok_images() -> Images {
    Images {
        provider_id: "openai",
        fixed: None,
        error: None,
    }
}
