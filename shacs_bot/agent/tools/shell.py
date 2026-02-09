"""쉘 실행 도구"""
import asyncio
import os
from pathlib import Path
from typing import Any

from litellm.llms.huggingface.common_utils import output_parser

from shacs_bot.agent.tools.base import Tool


class ExecTool(Tool):
    """쉘 명령을 실행하는 도구"""

    def __init__(
            self,
            timeout: int = 60,
            working_dir: str | None = None,
            deny_patterns: list[str] | None = None,
            allow_patterns: list[str] | None = None,
            restrict_to_workspace: bool = False,
    ):
        self.timeout = timeout
        self.working_dir = working_dir
        self.deny_patterns = deny_patterns or [
            r"\brm\s+-[rf]{1,2}\b",          # rm -r, rm -rf, rm -fr
            r"\bdel\s+/[fq]\b",              # del /f, del /q
            r"\brmdir\s+/s\b",               # rmdir /s
            r"\b(format|mkfs|diskpart)\b",   # disk operations
            r"\bdd\s+if=",                   # dd
            r">\s*/dev/sd",                  # write to disk
            r"\b(shutdown|reboot|poweroff)\b",  # system power
            r":\(\)\s*\{.*\};\s*:",          # fork bomb
        ]
        self.allow_patterns = allow_patterns or []
        self.restrict_to_workspace = restrict_to_workspace

    @property
    def name(self) -> str:
        return "exec"

    @property
    def description(self) -> str:
        return "쉘 명령을 실행합니다. 명령 출력 결과를 반환합니다. 주의해서 사용하세요."

    @property
    def parameters(self) -> dict[str, object]:
        return {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "실행할 쉘 명령어"
                },
                "working_dir": {
                    "type": "string",
                    "description": "명령어 실행을 위한 선택적 작업 디렉토리"
                }
            },
            "required": ["command"]
        }

    async def execute(self, command: str, working_dir: str | None = None, **kwargs: Any) -> str:
        cwd = working_dir or self.working_dir or os.getcwd()
        guard_error = self._guard_command(command,cwd)
        if guard_error:
            return guard_error

        try:
            process = await asyncio.create_subprocess_shell(
                cmd=command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=cwd
            )

            try:
                stdout, stderr = await asyncio.wait_for(
                    fut=process.communicate(),
                    timeout=self.timeout
                )
            except asyncio.TimeoutError:
                process.kill()
                return f"에러: 명령이 {self.timeout}초 내에 완료되지 않았습니다."

            output_parts = []

            if stdout:
                output_parts.append(stdout.decode(encoding="utf-8", errors="replace"))

            if stderr:
                stderr_text = stderr.decode(encoding="utf-8", errors="replace")
                if stderr_text.strip():
                    output_parts.append(f"STDERR:\n{stderr_text}")

            if process.returncode != 0:
                output_parts.append(f"\n종료 코드: {process.returncode}")

            result = "\n".join(output_parts) if output_parts else "(출력 없음)"

            # 출력이 너무 길면 요약
            max_len = 10_000
            if len(result) > max_len:
                result = result[:max_len] + f"\n... (생략됨, {len(result) - max_len}자 더 있음)"

            return result

        except Exception as e:
            return f"명령 실행 중 오류 발생: {str(e)}"

    def _guard_command(self, command: str, working_dir: str) -> str | None:
        """명령어를 검사하여 허용되지 않는 패턴이 있는지 확인합니다."""
        cmd = command.strip()
        cmd_lower = cmd.lower()

        import re
        for pattern in self.deny_patterns:
            if re.search(pattern, cmd_lower):
                return "에러: 명령어가 safety guard에 의해 차단되었습니다 (안전하지 않은 패턴 감지됨)"

        if self.allow_patterns:
            if not any(re.search(p, cmd_lower) for p in self.allow_patterns):
                return "에러: 명령어가 safety guard에 의해 차단되었습니다 (허용 목록에 없음)"

        if self.restrict_to_workspace:
            if "..\\" in cmd or "../" in cmd:
                return "에러: 명령어가 safety guard에 의해 차단되었습니다 (디렉토리 탐색 감지됨)"

            cwd_path = Path(working_dir).resolve()

            win_paths: list[Any] = re.findall(r"[A-Za-z]:\\[^\\\"']+", cmd)
            posix_paths: list[Any] = re.findall(r"/[^\s\"']+", cmd)

            for raw in win_paths + posix_paths:
                try:
                    p: Path = Path(raw).resolve()
                except Exception:
                    continue
                if cwd_path not in p.parent and p != cwd_path:
                    return "에러: 명령어가 safety guard에 의해 차단되었습니다 (작업 디렉토리 외부 경로 감지됨)"

        return None

