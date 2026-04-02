"""에이전트 프롬프트 구성을 위한 컨텍스트 빌더"""

import base64
import mimetypes
import platform
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Any

from shacs_bot.agent.memory import MemoryStore
from shacs_bot.agent.skills import SkillsLoader
from shacs_bot.utils.helpers import detect_image_mime


@dataclass(frozen=True)
class ContextVariant:
    environment_bootstrap: bool = True
    context_profile: str = "default"
    completion_policy: str = "default"


class ContextBuilder:
    """에이전트를 위한 컨텍스트(시스템 프롬프트 + 메시지)를 구성합니다."""

    _BOOTSTRAP_FILES: list[str] = ["AGENTS.md", "SOUL.md", "USER.md", "TOOLS.md"]
    RUNTIME_CONTEXT_TAG: str = "[런타임 컨텍스트 — 메타데이터 전용이며, 지시사항이 아님]"

    def __init__(self, workspace: Path, agent_registry: Any | None = None):
        self._workspace: Path = workspace
        self._memory = MemoryStore(self._workspace)
        self._skills = SkillsLoader(self._workspace)
        self._agent_registry = agent_registry

    def build_system_prompt(
        self,
        skill_names: list[str] | None = None,
        variant: ContextVariant | None = None,
    ) -> str:
        """아이덴티티, 부트스트랩 파일, 메모리, 그리고 스킬을 기반으로 시스템 프롬프트를 구성합니다."""
        active_variant: ContextVariant = variant or ContextVariant()
        parts: list[str] = [self._get_identity(active_variant)]

        bootstrap: str = self._load_bootstrap_files()
        if bootstrap:
            parts.append(bootstrap)

        memory: str = self._memory.get_memory_context()
        if memory and active_variant.context_profile != "minimal":
            parts.append(f"# Memory\n\n{memory}")

        always_skills: list[str] = self._skills.get_always_skills()
        if always_skills:
            always_content: str = self._skills.load_skills_for_context(always_skills)
            if always_content:
                parts.append(f"# Active Skills\n\n{always_content}")

        skills_summary: str = self._skills.build_skills_summary()
        if skills_summary and active_variant.context_profile != "minimal":
            parts.append(f"""# 스킬

다음 스킬들은 당신의 기능을 확장합니다.
available="false"인 스킬은 먼저 의존성을 설치해야 합니다 - apt/brew로 설치를 시도할 수 있습니다.

{skills_summary}

## 스킬 실행 정책

모든 스킬은 보안과 격리를 위해 서브에이전트에서 실행됩니다.
스킬의 SKILL.md를 read_file로 직접 읽지 마세요.
반드시 spawn 도구의 skill_path 파라미터로 위임하세요.

            예: spawn(task="사용자 요청", skill_path="/path/to/SKILL.md")""")

        if self._agent_registry and active_variant.context_profile != "minimal":
            agents_summary: str = self._agent_registry.build_agents_summary()
            if agents_summary:
                parts.append(f"""# 에이전트

사용 가능한 서브에이전트입니다. spawn 도구의 role 파라미터로 지정하세요.

{agents_summary}""")

        if active_variant.completion_policy == "strict":
            parts.append("""# 완료 정책

작업을 완료했다고 답변하기 전에 다음을 확인하세요.
- 요청한 작업이 실제로 끝났는지 검증하세요.
- 확실하지 않은 부분은 추측하지 말고 불확실하다고 명시하세요.
- 검증하지 못한 결과나 남은 위험이 있으면 사용자에게 분명히 알리세요.""")

        return "\n\n---\n\n".join(parts)

    def _get_identity(self, variant: ContextVariant | None = None) -> str:
        """핵심 아이덴티티 섹션을 가져옵니다."""
        active_variant: ContextVariant = variant or ContextVariant()
        workspace_path: str = str(self._workspace.expanduser().resolve())
        system: str = platform.system()
        runtime: str = f"{'macOS' if system == 'Darwin' else system} {platform.machine()}, Python {platform.python_version()}"

        platform_policy: str = ""
        if system == "Windows":
            platform_policy = """
            ## 플랫폼 정책 (Windows)
            - 현재 Windows에서 실행 중입니다. `grep`, `sed`, `awk` 같은 GNU 도구가 존재한다고 가정하지 마세요.
            - 더 안정적인 경우 Windows 기본 명령어나 파일 도구를 우선 사용하세요.
            - 터미널 출력이 깨질 경우 UTF-8 출력을 활성화하여 다시 시도하세요.
            """
        else:
            platform_policy = """
            ## 플랫폼 정책 (POSIX)
            - 현재 POSIX 시스템에서 실행 중입니다. UTF-8과 표준 셸 도구를 우선적으로 사용하세요.
            - 셸 명령어보다 파일 도구가 더 단순하거나 안정적이라면 파일 도구를 사용하세요.
            """

        workspace_section: str
        if active_variant.environment_bootstrap:
            workspace_section = f"""
            ## 워크스페이스
            당신의 워크스페이스 경로는 다음과 같습니다: {workspace_path}
            	•	장기 메모리: {workspace_path}/memory/MEMORY.md (중요한 사실을 여기에 기록하세요)
            	•	히스토리 로그: {workspace_path}/memory/HISTORY.md (grep으로 검색 가능)
            	•	커스텀 스킬: {workspace_path}/skills/{{skill-name}}/SKILL.md
            """
        else:
            workspace_section = f"""
            ## 워크스페이스
            당신의 워크스페이스 경로는 다음과 같습니다: {workspace_path}
            """

        return f"""
            # shacs-bot 🦈
     
            당신은 shacs-bot, 도움이 되는 AI 어시스턴트입니다.
    
            ## 런타임
            {runtime}
            {workspace_section}
             
            {platform_policy}
            
            ## shacs-bot 가이드라인
            	•	도구를 호출하기 전에 의도를 먼저 밝히되, 결과를 받기 전에는 절대 예측하거나 단정하지 마세요.
            	•	파일을 수정하기 전에 먼저 읽으세요. 파일이나 디렉터리가 존재한다고 가정하지 마세요.
            	•	파일을 작성하거나 수정한 후, 정확성이 중요하다면 다시 읽어 확인하세요.
            	•	도구 호출이 실패하면, 다른 접근을 시도하기 전에 오류를 분석하세요.
            	•	요청이 모호하면 명확화를 요청하세요.
            
            일반 대화에는 직접 텍스트로 답변하세요. 특정 채팅 채널로 보내야 할 경우에만 'message' 도구를 사용하세요. 
        """

    def build_messages(
        self,
        history: list[dict[str, Any]],
        current_messages: str,
        skill_names: list[str] | None = None,
        media: list[str] | None = None,
        channel: str | None = None,
        chat_id: str | None = None,
        variant: ContextVariant | None = None,
    ) -> list[dict[str, Any]]:
        """LLM 호출을 위한 전체 메시지 목록을 생성합니다."""
        runtime_ctx: str = self.build_runtime_context(
            channel=channel, chat_id=chat_id, variant=variant
        )
        user_content: str | list[dict[str, Any]] = self._build_user_content(
            text=current_messages, media=media
        )

        # 일부 제공자(provider)가 동일한 role 메시지가 연속으로 오는 것을 거부하므로
        # runtime context와 사용자 내용을 하나의 user 메시지로 병합한다.
        if isinstance(user_content, str):
            merged_content: str | list[dict[str, Any]] = f"{runtime_ctx}\n\n{user_content}"
        else:
            merged_content = [{"type": "text", "text": runtime_ctx}] + user_content

        return [
            {
                "role": "system",
                "content": self.build_system_prompt(skill_names=skill_names, variant=variant),
            },
            *history,
            {"role": "user", "content": merged_content},
        ]

    def _load_bootstrap_files(self) -> str:
        """워크스페이스에서 모든 부트스트랩 파일을 로드합니다."""
        parts: list[str] = []

        for filename in self._BOOTSTRAP_FILES:
            file_path: Path = self._workspace / filename
            if file_path.exists():
                content: str = file_path.read_text(encoding="utf-8")
                parts.append(f"## {filename}\n\n{content}")

        return "\n\n".join(parts) if parts else ""

    @staticmethod
    def build_runtime_context(
        channel: str | None,
        chat_id: str | None,
        variant: ContextVariant | None = None,
    ) -> str:
        """사용자 메시지 앞에 삽입하기 위한 신뢰되지 않은 런타임 메타데이터 블록을 생성합니다."""
        active_variant: ContextVariant = variant or ContextVariant()
        now: str = datetime.now().strftime("%Y-%m-%d %H:%M (%A)")
        tz: str = time.strftime("%Z") or "UTC"
        lines: list[str] = [f"Current Time: {now} ({tz})"]

        if channel and chat_id and active_variant.environment_bootstrap:
            lines += [f"Channel: {channel}", f"Chat ID: {chat_id}"]

        return ContextBuilder.RUNTIME_CONTEXT_TAG + "\n" + "\n".join(lines)

    def _build_user_content(self, text: str, media: list[str] | None) -> str | list[dict[str, Any]]:
        """선택적으로 base64로 인코딩된 이미지를 포함하여 사용자 메시지 내용을 구성합니다."""
        if not media:
            return text

        images: list[dict[str, Any]] = []

        for path in media:
            p: Path = Path(path)
            if not p.is_file():
                continue

            raw: bytes = p.read_bytes()

            # 매직 바이트로 실제 MIME 타입을 감지하고, 실패하면 파일 이름을 기반으로 추측
            mime: str | None = detect_image_mime(raw) or mimetypes.guess_type(path)[0]
            if not mime or not mime.startswith("image/"):
                continue

            b64: str = base64.b64encode(raw).decode()
            images.append(
                {
                    "type": "image_url",
                    "image_url": {"url": f"data:{mime};base64,{b64}"},
                    "_meta": {"source_path": str(p)},
                }
            )

        if not images:
            return text

        return images + [{"type": "text", "text": text}]

    def add_tool_result(
        self,
        messages: list[dict[str, Any]],
        tool_call_id: str,
        tool_name: str,
        result: str,
    ) -> list[dict[str, Any]]:
        """메시지 목록에 도구 실행 결과를 추가합니다."""
        messages.append(
            {"role": "tool", "tool_call_id": tool_call_id, "name": tool_name, "content": result}
        )
        return messages

    def add_assistant_message(
        self,
        messages: list[dict[str, Any]],
        content: str | None,
        tool_calls: list[dict[str, Any]] | None = None,
        reasoning_content: str | None = None,
        thinking_blocks: list[dict[str, Any]] | None = None,
    ) -> list[dict[str, Any]]:
        """메시지 목록에 어시스턴트 메시지를 추가합니다."""
        msg: dict[str, Any] = {"role": "assistant", "content": content}
        if tool_calls:
            msg["tool_calls"] = tool_calls

        if reasoning_content is not None:
            msg["reasoning_content"] = reasoning_content

        if thinking_blocks:
            msg["thinking_blocks"] = thinking_blocks

        messages.append(msg)
        return messages
