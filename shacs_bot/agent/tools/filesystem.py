"""파일 시스템 도구: 파일 읽기, 쓰기, 수정"""
import difflib
from pathlib import Path
from typing import Any, Literal

from shacs_bot.agent.tools.base import Tool


def _resolve_path(path: str, workspace: Path | None, allowed_dir: Path | None) -> Path:
    """상대 경로라면 workspace 기준으로 경로를 해석하고, 지정된 허용 디렉토리 범위를 벗어나지 않도록 제한한다."""
    path: Path = Path(path).expanduser()
    if not path.is_absolute() and workspace:
        path = workspace / path

    resolve: Path = path.resolve()
    if allowed_dir:
        try:
            resolve.relative_to(allowed_dir.resolve())
        except ValueError:
            raise PermissionError(f"경로 {path}는 허용된 디렉터리 {allowed_dir} 외부에 있습니다.")

    return resolve

def _short_hash(text: str, *, salt: str = "", len: int = 8) -> str:
    """긴 텍스트를 간결하게 표현하기 위해 텍스트의 짧은 해시를 생성합니다."""
    import hashlib
    return hashlib.sha256((salt + "\n" + text).encode("utf-8")).hexdigest()[:len]

def _make_tag(lineno: int, line_text: str, *, salt: str = "", hash_len: int = 8) -> str:
    """텍스트 라인에 대한 태그를 생성하여 긴 텍스트를 간결하게 표현합니다."""
    stripped: str = line_text.rstrip("\n")
    return f"L{lineno}#{_short_hash(stripped, salt=salt, len=hash_len)}"

class ReadFileTool(Tool):
    """파일의 콘텐츠를 읽는 도구입니다."""
    name = "read_file"
    description = "주어진 경로의 파일 콘텐츠를 읽습니다. (옵션: hashlines로 반환 가능)"
    parameters =  {
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "읽을 파일의 경로"
            },
            "hashlines": {
                "type": "boolean",
                "description": "true면 각 라인에 hashline 태그(Ln#hash| ...)를 붙여 반환",
                "default": False,
            },
            "hash_len": {
                "type": "integer",
                "description": "hashline 해시 길이(기본 8)",
                "default": 8,
            },
        },
        "required": ["path"]
    }

    def __init__(self, workspace: Path | None = None, allowed_dir: Path | None = None):
        self._workspace = workspace
        self._allowed_dir = allowed_dir

    async def execute(self, path: str, hashlines: bool = False, hash_len: int = 8, **kwargs: Any) -> str:
        try:
            file_path = _resolve_path(path, self._workspace, self._allowed_dir)
            if not file_path.exists():
                return f"오류: 파일을 찾을 수 없습니다: {path}"

            if not file_path.is_file():
                return f"오류: 파일이 아닙니다: {path}"

            content: str = file_path.read_text(encoding="utf-8")
            if not hashlines:
                return content

            # hashlines 모드
            salt: str = f"path:{file_path.as_posix()}"
            out_lines: list[str] = []
            for idx, line in enumerate(content.splitlines(), start=1):
                tag: str = _make_tag(idx, line, salt=salt, hash_len=hash_len)
                out_lines.append(f"{tag}| {line}")

            return "\n".join(out_lines)
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"파일 읽기 오류: {str(e)}"

class WriteFileTool(Tool):
    """파일에 콘텐츠를 쓰는 도구입니다."""
    name = "write_file"
    description =  "주어진 경로의 파일에 콘텐츠를 씁니다. 만일 부모 디렉터리가 존재하지 않으면 생성합니다."
    parameters = {
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "쓰기할 파일의 경로"
            },
            "content": {
                "type": "string",
                "description": "쓰기할 콘텐츠"
            }
        },
        "required": ["path", "content"]
    }

    def __init__(self, workspace: Path | None = None, allowed_dir: Path | None = None):
        self._workspace = workspace
        self._allowed_dir = allowed_dir

    async def execute(self, path: str, content: str, **kwargs: Any) -> str:
        try:
            file_path = _resolve_path(path, self._workspace, self._allowed_dir)
            file_path.parent.mkdir(parents=True, exist_ok=True)
            file_path.write_text(content, encoding="utf-8")
            return f"{len(content)} bytes를 {path}에 성공적으로 썼습니다"
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"파일 쓰기 오류: {str(e)}"

def _parse_tag(tag: str) -> tuple[int, str]:
    """hashline 태그에서 라인 번호와 해시를 추출합니다."""
    # 태그 형식은 "L{lineno}#{hash}"입니다. 예: "L42#abc123ef| some text"
    if not tag.startswith("L") or ("#" not in tag):
        raise ValueError(f"잘못된 태그 형식: {tag}")

    left, height = tag.split("#", 1)
    lineno: int = int(left[1:])
    return lineno, height

def _ensure_line_ends_with_nl(text: str) -> str:
    """텍스트가 개행으로 끝나도록 보장합니다."""
    return text if text.endswith("\n") else text + "\n"

def _normalize_block(block: str, *, keep_trailing_newline: bool) -> str:
    """
    - keep_trailing_newline=True: 블록이 파일 중간에 삽입될 가능성이 높으므로 끝에 \n 보장
    - keep_trailing_newline=False: 파일 끝 교체 등에서 사용자가 의도적으로 마지막 개행을 없앨 수도 있음
    """
    if block == "":
        return "" if not keep_trailing_newline else ""
    if keep_trailing_newline:
        return _ensure_line_ends_with_nl(block)
    return block

class EditFileTool(Tool):
    """파일의 콘텐츠를 수정하는 도구입니다."""
    name = "edit_file"
    description = (
        "Hashline 태그 기반으로 파일을 수정합니다. "
        "지원: 단일 줄 교체(replace_line), 삽입(insert_before/after), 삭제(delete_line/delete_range), "
        "범위 교체(replace_range). "
        "태그 해시가 현재 파일 내용과 일치해야 동작합니다."
    )

    parameters = {
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "수정할 파일의 경로"
            },
            "op": {
                "type": "string",
                "enum": [
                    "replace_line",
                    "insert_before",
                    "insert_after",
                    "delete_line",
                    "delete_range",
                    "replace_range",
                ],
                "description": "수행할 편집 작업 종류",
            },
            "hash_len": {
                "type": "integer",
                "description": "hashline 해시 길이(기본 8). read_file과 동일하게 맞추세요.",
                "default": 8,
            },
            # 단일 줄 대상
            "line_tag": {
                "type": "string",
                "description": "수정할 줄의 hashline 태그 (예: L12#9f86d081). 제공 시 이 값이 우선됩니다.",
            },
            # 범위 대상
            "start_tag": {
                "type": "string",
                "description": "범위 시작 태그(포함) (예: L10#aaaa1111)",
            },
            "end_tag": {
                "type": "string",
                "description": "범위 끝 태그(포함) (예: L20#bbbb2222)",
            },
            # 삽입/교체에 사용
            "text": {
                "type": "string",
                "description": "삽입하거나 교체할 텍스트 블록",
            },
        },
        "required": ["path", "op"],
        "allOf": [
            {
                "if": {"properties": {"op": {"const": "replace_line"}}},
                "then": {"required": ["line_tag", "text"]},
            },
            {
                "if": {"properties": {"op": {"const": "insert_before"}}},
                "then": {"required": ["line_tag", "text"]},
            },
            {
                "if": {"properties": {"op": {"const": "insert_after"}}},
                "then": {"required": ["line_tag", "text"]},
            },
            {
                "if": {"properties": {"op": {"const": "delete_line"}}},
                "then": {"required": ["line_tag"]},
            },
            {
                "if": {"properties": {"op": {"const": "delete_range"}}},
                "then": {"required": ["start_tag", "end_tag"]},
            },
            {
                "if": {"properties": {"op": {"const": "replace_range"}}},
                "then": {"required": ["start_tag", "end_tag", "text"]},
            },
        ],
    }

    def __init__(self, workspace: Path | None = None, allowed_dir: Path | None = None):
        self._workspace = workspace
        self._allowed_dir = allowed_dir

    async def execute(
            self,
            path: str,
            op: Literal[
                "replace_line",
                "insert_before",
                "insert_after",
                "delete_line",
                "delete_range",
                "replace_range",
            ],
            hash_len: int = 8,
            line_tag: str | None = None,
            start_tag: str | None = None,
            end_tag: str | None = None,
            text: str | None = None,
            **kwargs: Any
    ) -> str:
        try:
            file_path = _resolve_path(path, self._workspace, self._allowed_dir)
            if not file_path.exists():
                return f"오류: 파일을 찾을 수 없습니다: {path}"

            if not file_path.is_file():
                return f"오류: 파일이 아닙니다: {path}"

            # 파일 라인 유지(개행 포함)
            lines: list[str] = file_path.read_text(encoding="utf-8").splitlines(keepends=True)
            salt: str = f"path:{file_path.as_posix()}"

            def verify_tag(tag: str) -> int:
                """tag가 가리키는 줄번호를 반환. 해시 불일치면 오류."""
                lineno, expected_hash = _parse_tag(tag)
                if lineno < 1 or lineno > len(lines):
                    raise IndexError(f"줄번호 범위 오류: {tag}")

                current_line: str = lines[lineno - 1]
                current_tag: str = _make_tag(lineno, current_line, salt=salt, hash_len=hash_len)
                _, current_hash = _parse_tag(current_tag)
                if current_hash != expected_hash:
                    raise ValueError(
                        "tag 해시가 현재 파일 내용과 일치하지 않습니다. "
                        "파일이 변경되었을 수 있으니 read_file(hashlines=true)로 최신 태그를 다시 받아 요청하세요."
                    )

                return lineno

            def verify_range(start_tag: str, end_tag: str) -> tuple[int, int]:
                start: int = verify_tag(start_tag)
                end: int = verify_tag(end_tag)
                if start > end:
                    raise ValueError(f"범위 오류: start_tag가 end_tag보다 뒤에 있습니다. ({start} > {end})")

                return start, end

            # 작업 수행
            changed: bool = False
            if op == "replace_line":
                assert line_tag is not None and text is not None
                ln: int = verify_tag(line_tag)
                # 해당 줄의 기존 개행 유지
                had_nl: bool = lines[ln - 1].endswith("\n")
                replacement: str = text
                if had_nl and not replacement.endswith("\n"):
                    replacement += "\n"

                lines[ln - 1] = replacement
                changed = True

            elif op == "insert_before":
                assert line_tag is not None and text is not None
                ln: int = verify_tag(line_tag)
                inserts: str = _normalize_block(text, keep_trailing_newline=True)
                lines[ln - 1:ln - 1] = inserts.splitlines(keepends=True)
                changed = True

            elif op == "insert_after":
                assert line_tag is not None and text is not None
                ln: int = verify_tag(line_tag)
                inserts: str = _normalize_block(text, keep_trailing_newline=True)
                lines[ln:ln] = inserts.splitlines(keepends=True)
                changed = True

            elif op == "delete_line":
                assert line_tag is not None
                ln: int = verify_tag(line_tag)
                del lines[ln - 1]
                changed = True

            elif op == "delete_range":
                assert start_tag is not None and end_tag is not None
                start, end = verify_range(start_tag, end_tag)
                del lines[start - 1:end]
                changed = True

            elif op == "replace_range":
                assert start_tag is not None and end_tag is not None and text is not None
                start, end = verify_range(start_tag, end_tag)
                # 범위 교체 블록은 "중간 삽입" 가능성이 높으니 끝 개행 보장
                inserts: str = _normalize_block(text, keep_trailing_newline=True)
                lines[start - 1:end] = inserts.splitlines(keepends=True)
                changed = True

            else:
                return f"오류: 지원하지 않는 op입니다: {op}"

            if changed:
                file_path.write_text("".join(lines), encoding="utf-8")

            return f"{path}의 파일이 성공적으로 수정되었습니다. (op={op})"
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"파일 수정 오류: {str(e)}"

    @staticmethod
    def _not_found_message(old_text: str, content: str, path: str) -> str:
        """old_text가 content에 존재하지 않을 때 반환할 메시지를 생성합니다."""
        lines: list[str] = content.splitlines(keepends=True)
        old_lines: list[str] = old_text.splitlines(keepends=True)
        window: int = len(old_lines)

        best_ratio, best_start = 0.0, 0
        for idx in range(max(1, len(lines) - window + 1)):
            ratio: float = difflib.SequenceMatcher(None, old_lines, lines[idx : idx + window]).ratio()
            if ratio > best_ratio:
                best_ratio, best_start = ratio, idx

        if best_ratio > 0.5:
            diff: str = "\n".join(
                difflib.unified_diff(
                    old_lines, lines[best_start : best_start + window],
                    fromfile="old_text (provided)", tofile=f"{path} (actual, line {best_start + 1})",
                    lineterm="",
                ))
            return f"Error: old_text not found in {path}.\nBest match ({best_ratio:.0%} similar) at line {best_start + 1}:\n{diff}"

        return f"Error: old_text not found in {path}. No similar text found. Verify the file content."


class ListDirTool(Tool):
    """디렉터리의 파일 및 하위 디렉터리를 나열하는 도구입니다."""
    name = "list_dir"
    description = "주어진 경로의 디렉터리에 있는 파일과 하위 디렉터리를 나열합니다."
    parameters = {
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "나열할 디렉터리의 경로"
            }
        },
        "required": ["path"]
    }

    def __init__(self, workspace: Path | None = None, allowed_dir: Path | None = None):
        self._workspace = workspace
        self._allowed_dir = allowed_dir

    async def execute(self, path: str, **kwargs: Any) -> str:
        try:
            dir_path = _resolve_path(path, self._workspace, self._allowed_dir)
            if not dir_path.exists():
                return f"오류: 디렉터리를 찾을 수 없습니다: {path}"

            if not dir_path.is_dir():
                return f"오류: 디렉터리가 아닙니다: {path}"

            items: list[str] = []
            for item in sorted(dir_path.iterdir()):
                prefix = "📁 " if item.is_dir() else "📄 "
                items.append(f"{prefix}{item.name}")

            if not items:
                return f"폴더 {path}는 비어 있습니다."

            return "\n".join(items)
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"디렉터리 나열 오류: {str(e)}"


