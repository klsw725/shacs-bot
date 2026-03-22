"""쉘 실행 도구"""

import asyncio
import os
import re
from asyncio.subprocess import Process
from pathlib import Path
from typing import Any

from shacs_bot.agent.tools.base import Tool


class ExecTool(Tool):
    """쉘 명령을 실행하는 도구"""

    name: str = "exec"
    description: str = "쉘 명령을 실행합니다. 명령 출력 결과를 반환합니다. 주의해서 사용하세요."
    parameters: dict[str, object] = {
        "type": "object",
        "properties": {
            "command": {"type": "string", "description": "실행할 쉘 명령어"},
            "_working_dir": {
                "type": "string",
                "description": "명령어 실행을 위한 선택적 작업 디렉토리",
            },
        },
        "required": ["command"],
    }

    def __init__(
        self,
        timeout: int = 60,
        working_dir: str | None = None,
        deny_patterns: list[str] | None = None,
        allow_patterns: list[str] | None = None,
        restrict_to_workspace: bool = False,
        path_append: str = "",
    ):
        self._timeout: int = timeout
        self._working_dir: str | None = working_dir
        self._deny_patterns: list[str] = deny_patterns or [
            r"\brm\s+-[rf]{1,2}\b",  # rm -r, rm -rf, rm -fr
            r"\bdel\s+/[fq]\b",  # del /f, del /q
            r"\brmdir\s+/s\b",  # rmdir /s
            r"(?:^|[;&|]\s*)format\b",  # format (as standalone command only)
            r"\b(mkfs|diskpart)\b",  # disk operations
            r"\bdd\s+if=",  # dd
            r">\s*/dev/sd",  # write to disk
            r"\b(shutdown|reboot|poweroff)\b",  # system power
            r":\(\)\s*\{.*\};\s*:",  # fork bomb
        ]
        self._allow_patterns: list[str] = allow_patterns or []
        self._restrict_to_workspace: bool = restrict_to_workspace
        self._path_append: str = path_append

    async def execute(self, command: str, working_dir: str | None = None, **kwargs: Any) -> str:
        cwd: str = working_dir or self._working_dir or os.getcwd()
        guard_error: str | None = self._guard_command(command, cwd)
        if guard_error:
            return guard_error

        env: dict = os.environ.copy()

        if self._path_append:
            env["PATH"] = env.get("PATH", "") + os.pathsep + self._path_append

        try:
            process: Process = await asyncio.create_subprocess_shell(
                cmd=command,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=cwd,
                env=env,
            )
            try:
                stdout, stderr = await asyncio.wait_for(
                    fut=process.communicate(), timeout=self._timeout
                )
            except asyncio.TimeoutError:
                process.kill()
                # 프로세스가 완전히 종료될 때까지 대기하여
                # 파이프가 비워지고 파일 디스크립터가 해제되도록 합니다.
                try:
                    await asyncio.wait_for(fut=process.wait(), timeout=5.0)
                except asyncio.TimeoutError:
                    pass

                return f"에러: 명령이 {self._timeout}초 내에 완료되지 않았습니다."

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
            result = self._mask_env_secrets(result)

            max_len = 10_000
            if len(result) > max_len:
                result = result[:max_len] + f"\n... (생략됨, {len(result) - max_len}자 더 있음)"

            return result
        except Exception as e:
            return f"명령 실행 중 오류 발생: {str(e)}"

    def _guard_command(self, command: str, working_dir: str) -> str | None:
        """명령어를 검사하여 허용되지 않는 패턴이 있는지 확인합니다."""
        cmd: str = command.strip()
        cmd_lower: str = cmd.lower()

        for pattern in self._deny_patterns:
            if re.search(pattern, cmd_lower):
                return (
                    "에러: 명령어가 safety guard에 의해 차단되었습니다 (안전하지 않은 패턴 감지됨)"
                )

        if self._allow_patterns:
            if not any(re.search(pattern, cmd_lower) for pattern in self._allow_patterns):
                return "에러: 명령어가 safety guard에 의해 차단되었습니다 (허용 목록에 없음)"
        if self._restrict_to_workspace:
            if "..\\" in cmd or "../" in cmd:
                return "에러: 명령어가 safety guard에 의해 차단되었습니다 (디렉토리 탐색 감지됨)"

            cwd_path = Path(working_dir).resolve()

            for raw in self._extract_absolute_paths(cmd):
                try:
                    p: Path = Path(raw.strip()).resolve()
                except Exception:
                    continue

                if p.is_absolute() and (cwd_path not in p.parent) and (p != cwd_path):
                    return "에러: 명령어가 safety guard에 의해 차단되었습니다 (작업 디렉토리 외부 경로 감지됨)"

        return None

    _SENSITIVE_KEY_PATTERNS: tuple[str, ...] = (
        "KEY",
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "CREDENTIAL",
        "AUTH",
        "PRIVATE",
    )

    def _mask_env_secrets(self, text: str) -> str:
        """출력 텍스트에서 민감한 환경변수 값을 ***로 치환합니다."""
        values: set[str] = set()
        for name, value in os.environ.items():
            if len(value) <= 3:
                continue
            name_upper: str = name.upper()
            if any(p in name_upper for p in self._SENSITIVE_KEY_PATTERNS):
                values.add(value)

        for value in sorted(values, key=len, reverse=True):
            text = text.replace(value, "***")
        return text

    def _extract_absolute_paths(self, command: str) -> list[str]:
        win_paths: list[Any] = re.findall(r"[A-Za-z]:\\[^\s\"'|><;]+", command)
        # 절대 경로만 매칭합니다 — ".venv/bin/python"과 같은 상대 경로에서
        # 기존 패턴이 "/bin/python"을 잘못 추출하던
        # 오탐(false positive)을 방지하기 위함입니다.
        posix_paths: list[Any] = re.findall(r"(?:^|[\s|>])(/[^\s\"'>]+)", command)
        return win_paths + posix_paths
