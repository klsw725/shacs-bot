"""Multi-modal media generation tool."""

import asyncio
import base64
from datetime import datetime
from pathlib import Path
from typing import Any

import httpx
from loguru import logger

from shacs_bot.agent.tools.base import Tool
from shacs_bot.config.schema import MediaConfig


class MediaGenerateTool(Tool):
    _IMAGEN_URL = "https://generativelanguage.googleapis.com/v1beta/models/imagen-4.0-generate-preview-05-20:predict"
    _VEO_URL = "https://generativelanguage.googleapis.com/v1beta/models/veo-3.0-generate-preview:predictLongRunning"
    _VEO_POLL_BASE = "https://generativelanguage.googleapis.com/v1beta"
    _VEO_POLL_INTERVAL = 5
    _VEO_POLL_MAX = 60

    @property
    def name(self) -> str:
        return "media_generate"

    @property
    def description(self) -> str:
        return (
            "Generate media content (image or video) from a text prompt. "
            "Returns the file path of the generated media. "
            "After generating, use the message tool with the media parameter to send the file to the user."
        )

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "enum": ["image", "video"],
                    "description": "Type of media to generate.",
                },
                "prompt": {
                    "type": "string",
                    "description": "Text description of the media to generate.",
                },
            },
            "required": ["type", "prompt"],
        }

    def __init__(self, config: MediaConfig, api_key: str = "", base_url: str = ""):
        self._config: MediaConfig = config
        self._api_key: str = api_key
        self._base_url: str = base_url.rstrip("/") if base_url else ""
        self._save_dir: Path = Path(config.save_dir).expanduser()
        self._save_dir.mkdir(parents=True, exist_ok=True)

    async def execute(self, type: str, prompt: str, **kwargs: Any) -> str:
        if type == "image":
            return await self._generate_image(prompt)
        elif type == "video":
            return await self._generate_video(prompt)
        return f"Error: Unsupported media type: {type}"

    async def _generate_image(self, prompt: str) -> str:
        if not self._api_key:
            return f"Error: No API key configured in providers.image_gen."

        if self._config.backend == "gemini":
            return await self._imagen(prompt)
        return await self._openai_compatible_image(prompt)

    async def _imagen(self, prompt: str) -> str:
        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(
                self._IMAGEN_URL,
                headers={"x-goog-api-key": self._api_key},
                json={
                    "instances": [{"prompt": prompt}],
                    "parameters": {"sampleCount": 1, "aspectRatio": "1:1"},
                },
            )
            resp.raise_for_status()

        image_bytes: bytes = base64.b64decode(resp.json()["predictions"][0]["bytesBase64Encoded"])
        path: Path = self._save_dir / f"image_{self._ts()}.png"
        path.write_bytes(image_bytes)
        logger.info("Image generated via Imagen 4: {}", path)
        return f"Image generated: {path}"

    async def _openai_compatible_image(self, prompt: str) -> str:
        if not self._base_url:
            return f"Error: No base_url configured for {self._config.backend} provider."

        url: str = f"{self._base_url}/chat/completions"
        payload: dict[str, Any] = {
            "messages": [{"role": "user", "content": prompt}],
            "modalities": ["image", "text"],
        }
        if self._config.model:
            payload["model"] = self._config.model

        async with httpx.AsyncClient(timeout=60) as client:
            resp = await client.post(
                url,
                headers={"Authorization": f"Bearer {self._api_key}"},
                json=payload,
            )
            resp.raise_for_status()

        data: dict[str, Any] = resp.json()
        images: list[Any] = data["choices"][0]["message"].get("images", [])
        if not images:
            return "Error: No image returned from provider."

        data_url: str = images[0]["image_url"]["url"]
        base64_data: str = data_url.split(",", 1)[1]
        image_bytes: bytes = base64.b64decode(base64_data)
        path: Path = self._save_dir / f"image_{self._ts()}.png"
        path.write_bytes(image_bytes)
        logger.info(
            "Image generated via {} ({}): {}",
            self._config.backend,
            self._config.model,
            path,
        )
        return f"Image generated: {path}"

    async def _generate_video(self, prompt: str) -> str:
        if self._config.backend != "gemini" or not self._api_key:
            return "Error: Video generation requires gemini backend with API key."

        headers: dict[str, str] = {"x-goog-api-key": self._api_key}

        async with httpx.AsyncClient(timeout=300) as client:
            resp = await client.post(
                self._VEO_URL,
                headers=headers,
                json={
                    "instances": [{"prompt": prompt}],
                    "parameters": {
                        "durationSeconds": self._config.video_duration_seconds,
                        "sampleCount": 1,
                    },
                },
            )
            resp.raise_for_status()
            op_name: str = resp.json()["name"]

            for _ in range(self._VEO_POLL_MAX):
                await asyncio.sleep(self._VEO_POLL_INTERVAL)
                poll = await client.get(
                    f"{self._VEO_POLL_BASE}/{op_name}",
                    headers=headers,
                )
                data: dict[str, Any] = poll.json()
                if data.get("done"):
                    video_bytes: bytes = base64.b64decode(
                        data["response"]["predictions"][0]["bytesBase64Encoded"]
                    )
                    path: Path = self._save_dir / f"video_{self._ts()}.mp4"
                    path.write_bytes(video_bytes)
                    logger.info("Video generated via Veo 3: {}", path)
                    return f"Video generated: {path}"

        return "Error: Video generation timed out (5 minutes)."

    def _ts(self) -> str:
        return datetime.now().strftime("%Y%m%d_%H%M%S")
