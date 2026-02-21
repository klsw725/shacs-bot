"""파일 시스템 도구: 파일 읽기, 쓰기, 수정"""
from pathlib import Path
from typing import Any

from shacs_bot.agent.tools.base import Tool


def _resolve_path(path: str, allowed_dir: Path | None) -> Path:
   """경로를 확인하고 필요에 따라 디렉터리 제한을 적용합니다."""
   resolve: Path = Path(path).expanduser().resolve()
   if allowed_dir and not str(resolve).startswith(str(allowed_dir.resolve())):
       raise PermissionError(f"경로 {path}는 허용된 디렉터리 {allowed_dir} 외부에 있습니다.")
   return resolve

class ReadFileTool(Tool):
    """파일의 콘텐츠를 읽는 도구입니다."""

    def __init__(self, allowed_dir: Path | None = None):
        self._allowed_dir = allowed_dir

    @property
    def name(self) -> str:
        return "read_file"

    @property
    def description(self) -> str:
        return "주어진 경로의 파일 콘텐츠를 읽습니다."

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "읽을 파일의 경로"
                }
            },
            "required": ["path"]
        }

    async def execute(self, path: str, **kwargs: Any) -> str:
        try:
            file_path = _resolve_path(path, self._allowed_dir)
            if not file_path.exists():
                return f"오류: 파일을 찾을 수 없습니다: {path}"
            if not file_path.is_file():
                return f"오류: 파일이 아닙니다: {path}"

            content: str = file_path.read_text(encoding="utf-8")
            return content
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"파일 읽기 오류: {str(e)}"

class WriteFileTool(Tool):
     """파일에 콘텐츠를 쓰는 도구입니다."""

     def __init__(self, allowed_dir: Path | None = None):
         self._allowed_dir = allowed_dir

     @property
     def name(self) -> str:
         return "write_file"

     @property
     def description(self) -> str:
         return "주어진 경로의 파일에 콘텐츠를 씁니다. 만일 부모 디렉터리가 존재하지 않으면 생성합니다."

     @property
     def parameters(self) -> dict[str, Any]:
         return {
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

     async def execute(self, path: str, content: str, **kwargs: Any) -> str:
         try:
             file_path = _resolve_path(path, self._allowed_dir)
             file_path.parent.mkdir(parents=True, exist_ok=True)
             file_path.write_text(content, encoding="utf-8")
             return f"{len(content)} bytes를 {path}에 성공적으로 썼습니다"
         except PermissionError as e:
             return f"오류: {e}"
         except Exception as e:
             return f"파일 쓰기 오류: {str(e)}"


class EditFileTool(Tool):
    """파일의 콘텐츠를 수정하는 도구입니다."""

    def __init__(self, allowed_dir: Path | None = None):
        self._allowed_dir = allowed_dir

    @property
    def name(self) -> str:
        return "edit_file"

    @property
    def description(self) -> str:
        return "신규 텍스트로 기존 파일 콘텐츠를 대체하여 파일을 수정합니다. 기존 텍스트는 정확히 파일에 존재해야 합니다."

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "수정할 파일의 경로"
                },
                "old_text": {
                    "type": "string",
                    "description": "파일에서 대체할 기존 텍스트"
                },
                "new_text": {
                    "type": "string",
                    "description": "파일에서 기존 텍스트를 대체할 신규 텍스트"
                }
            },
            "required": ["path", "old_text", "new_text"]
        }

    async def execute(self, path: str, old_text: str, new_text: str, **kwargs: Any) -> str:
        try:
            file_path = _resolve_path(path, self._allowed_dir)
            if not file_path.exists():
                return f"오류: 파일을 찾을 수 없습니다: {path}"

            content: str = file_path.read_text(encoding="utf-8")

            if old_text not in content:
                return f"오류: 기존 텍스트가 파일에 존재하지 않습니다. 정확한 텍스트를 제공해야 합니다."

            # 발생 횟수
            count: int = content.count(old_text)
            if count > 1:
                return f"오류: 기존 텍스트가 파일에 {count}회 존재합니다. 명확해지기 위해 더 많은 정보를 제공해야 합니다."

            new_content = content.replace(old_text, new_text, 1)
            file_path.write_text(new_content, encoding="utf-8")

            return f"{path}의 파일이 성공적으로 수정되었습니다."
        except PermissionError as e:
            return f"오류: {e}"
        except Exception as e:
            return f"파일 수정 오류: {str(e)}"


class ListDirTool(Tool):
    """디렉터리의 파일 및 하위 디렉터리를 나열하는 도구입니다."""

    def __init__(self, allowed_dir: Path | None = None):
        self._allowed_dir = allowed_dir

    @property
    def name(self) -> str:
        return "list_dir"

    @property
    def description(self) -> str:
        return "주어진 경로의 디렉터리에 있는 파일과 하위 디렉터리를 나열합니다."

    @property
    def parameters(self) -> dict[str, Any]:
        return {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "나열할 디렉터리의 경로"
                }
            },
            "required": ["path"]
        }

    async def execute(self, path: str, **kwargs: Any) -> str:
        try:
            dir_path = _resolve_path(path, self._allowed_dir)
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


