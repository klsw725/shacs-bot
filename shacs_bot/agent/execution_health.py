import hashlib
import json
from collections import deque
from typing import Any

from loguru import logger

_TOOL_REPEAT_THRESHOLD = 3
_ERROR_CASCADE_THRESHOLD = 3
_FILE_BURST_THRESHOLD = 10


class ExecutionHealthMonitor:
    def __init__(self, window_size: int = 15) -> None:
        self._recent: deque[tuple[str, str]] = deque(maxlen=window_size)
        self._consecutive_errors: int = 0

    def check(self, tool_name: str, args: dict[str, Any], result: str) -> None:
        args_key: str = hashlib.md5(
            json.dumps(args, sort_keys=True, ensure_ascii=False)[:1024].encode()
        ).hexdigest()[:8]
        entry: tuple[str, str] = (tool_name, args_key)

        result_head: str = result[:200].lower()
        if "error" in result_head or "실패" in result_head or "에러" in result_head:
            self._consecutive_errors += 1
            if self._consecutive_errors >= _ERROR_CASCADE_THRESHOLD:
                logger.warning(
                    "Execution health: {}회 연속 도구 에러 감지 (마지막: {})",
                    self._consecutive_errors,
                    tool_name,
                )
        else:
            self._consecutive_errors = 0

        same_count: int = sum(1 for e in self._recent if e == entry)
        if same_count >= _TOOL_REPEAT_THRESHOLD:
            logger.warning(
                "Execution health: {}이 동일 인자로 {}회 반복 호출됨",
                tool_name,
                same_count + 1,
            )

        if tool_name in ("write_file", "edit_file"):
            write_count: int = sum(1 for e in self._recent if e[0] in ("write_file", "edit_file"))
            if write_count >= _FILE_BURST_THRESHOLD:
                logger.warning(
                    "Execution health: 윈도우 내 {}회 파일 쓰기 감지",
                    write_count + 1,
                )

        self._recent.append(entry)
