"""Groq를 사용한 음성 전사 제공자."""
import os
from pathlib import Path

import httpx
from loguru import logger


class GroqTranscriptionProvider:
    """
    Groq의 Whisper API를 사용하는 음성 전사 제공자입니다.

    Groq는 매우 빠른 전사 속도를 제공하며, 넉넉한 무료 요금제를 지원합니다.
    """

    def __init__(self, api_key: str | None = None):
        self.api_key: str = api_key or os.environ.get("GROQ_API_KEY")
        self.api_url: str = "https://api.groq.com/openai/v1/audio/transcriptions"

    async def transcribe(self, file_path: str | Path) -> str:
        """
        Groq를 사용하여 오디오 파일을 전사합니다.

        Args:
            file_path: 오디오 파일의 경로.

        Returns:
            전사된 텍스트.
        """
        if not self.api_key:
            logger.warning("음성 전사를 위한 Groq API 키가 설정되지 않았습니다.")
            return ""

        path: Path = Path(file_path)
        if not path.exists():
            logger.error("오디오 파일을 찾지 못했습니다: {}", file_path)
            return ""

        try:
            async with httpx.AsyncClient() as client:
                with open(path, "rb") as f:
                    files = {
                        "file": (path.name, f),
                        "model": (None, "whisper-large-v3"),
                    }
                    headers = {
                        "Authorization": f"Bearer {self.api_key}",
                    }

                    response = await client.post(
                        self.api_url,
                        headers=headers,
                        files=files,
                        timeout=60.0
                    )
                    response.raise_for_status()

                    data: dict = response.json()
                    return data.get("text", "")

        except Exception as e:
            logger.error("Groq transcription 에러: {}", e)
            return ""