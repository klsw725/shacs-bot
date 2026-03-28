"""에이전트 기능을 위한 스킬 로더."""
import json
import os
import re
import shutil
from pathlib import Path
from typing import Any


class SkillsLoader:
    """
    에이전트 스킬을 로드하는 로더입니다.

    스킬은 특정 도구를 사용하는 방법이나 특정 작업을 수행하는 방법을 에이전트에게 가르치는 마크다운 파일(SKILL.md)입니다.
    """
    # 기본 내장 스킬 디렉터리 (이 파일을 기준으로 한 상대 경로)#
    BUILTIN_SKILLS_DIR: Path = Path(__file__).parent.parent / "skills"

    def __init__(
            self,
            workspace: Path,
            builtin_skills_dir: Path | None = None,
    ):
        self._workspace = workspace
        self._workspace_skills = self._workspace / "skills"
        self._builtin_skills = builtin_skills_dir or self.BUILTIN_SKILLS_DIR

    def list_skills(self, filter_unavailable: bool = True) -> list[dict[str, str]]:
        """
        사용가능한 전체 스킬들의 리스트

        Args:
             filter_unavailable: True인 경우, 충족되지 않은 요구사항이 있는 스킬을 제외합니다.
        Returns:
            'name', 'path', 'source'를 포함하는 스킬 정보 딕셔너리들의 목록.
        """
        skills: list[dict[str, Any]] = []

        # Workspace skills (최상위 우선순위)
        if self._workspace_skills.exists():
            for skill_dir in self._workspace_skills.iterdir():
                if skill_dir.is_dir():
                    skill_file: Path = skill_dir / "SKILL.md"
                    if skill_file.exists():
                        skills.append({
                            "name": skill_dir.name,
                            "path": str(skill_file),
                            "source": "workspace"
                        })

        # Built-in skills
        if self._builtin_skills and self._builtin_skills.exists():
            for skill_dir in self._builtin_skills.iterdir():
                if skill_dir.is_dir():
                    skill_file: Path = skill_dir / "SKILL.md"
                    if skill_file.exists() and not any(s["name"] == skill_dir.name for s in skills):
                        skills.append({
                            "name": skill_dir.name,
                            "path": str(skill_file),
                            "source": "builtin"
                        })

        # 요구 사항에 따라 필터링
        if filter_unavailable:
            return [s for s in skills if self._check_requirements(self._get_skill_meta(s["name"]))]

        return skills

    def _check_requirements(self, skill_meta: dict[str, Any]) -> bool:
        """스킬의 요구 사항(실행 파일, 환경 변수)이 충족되었는지 확인합니다."""
        requires: dict[str, list[str]] = skill_meta.get("requires", {})
        for b in requires.get("bins", []):
            if not shutil.which(b):
                return False

        for env in requires.get("env", []):
            if not os.environ.get(env):
                return False

        return True

    def _get_skill_meta(self, name: str) -> dict:
        """스킬의 shacs-bot 메타데이터를 가져옵니다 (프론트매터에 캐시됨)."""
        meta: dict[str, str] = self.get_skill_metadata(name) or {}
        return self._parse_shacs_bot_metadata(meta.get("metadata", ""))

    def get_skill_metadata(self, name: str) -> dict[str, str] | None:
        """
        스킬의 프론트매터(frontmatter)에서 메타데이터를 가져옵니다.

        Args:
            name: 스킬 이름

        Returns:
            Metadata dict or None
        """
        content: str = self.load_skill(name)
        if not content:
            return None

        if content.startswith("---"):
            match: re.Match[str] | None = re.match(r"^---\n(.*?)\n---", content, re.DOTALL)
            if match:
                # 간단 YAML 파싱
                metadata: dict[str, Any] = {}
                for line in match.group(1).split("\n"):
                    if ":" in line:
                        key, value = line.split(":", 1)
                        metadata[key.strip()] = value.strip().strip('"\'')

                return metadata

        return None

    def load_skill(self, name: str) -> str | None:
        """
        이름으로 스킬을 로드합니다.

        Args:
            name: 스킬 이름(디렉터리 이름).

        Returns:
            스킬 내용. 찾을 수 없는 경우 None.
        """
        # 먼저 workspace 확인
        workspace_skill: Path = self._workspace_skills / name / "SKILL.md"
        if workspace_skill.exists():
            return workspace_skill.read_text(encoding="utf-8")

        # built-in 확인
        if self._builtin_skills:
            builtin_skill: Path = self._builtin_skills / name / "SKILL.md"
            if builtin_skill.exists():
                return builtin_skill.read_text(encoding="utf-8")

        return None

    def _parse_shacs_bot_metadata(self, raw: str) -> dict:
        """프론트매터에서 스킬 메타데이터 JSON을 파싱합니다 (shacs-bot 및 openclaw 키를 지원)."""
        try:
            data: dict[str, Any] = json.loads(raw)
            return data.get("shacs-bot", data.get("openclaw", {})) if isinstance(data, dict) else {}
        except (json.JSONDecodeError, TypeError):
            return {}

    def load_skills_for_context(self, skill_names: list[str]) -> str:
        """
        에이전트 컨텍스트에 포함하기 위해 특정 스킬들을 로드합니다.

        Args:
            skill_names: 로드할 스킬 이름 목록.

        Returns:
            포맷된 스킬 내용.
        """
        parts: list[str] = []
        for name in skill_names:
            content: str | None = self.load_skill(name)
            if content:
                content = self._strip_frontmatter(content)
                parts.append(f"### Skill: {name}\n\n{content}")

        return "\n\n---\n\n".join(parts) if parts else ""

    def _strip_frontmatter(self, content: str) -> str:
        """Markdown 콘텐츠에서 YAML 포멧터 제거"""
        if content.startswith("---"):
            match: re.Match = re.match(r"^---\n.*?\n---\n", content, re.DOTALL)
            if match:
                return content[match.end():].strip()

        return content

    def build_skills_summary(self) -> str:
        """
        모든 스킬의 요약 정보를 생성합니다 (이름, 설명, 경로, 사용 가능 여부).

        이 요약은 점진적 로딩을 위해 사용됩니다. 에이전트는 필요할 때
        read_file을 사용하여 전체 스킬 내용을 읽을 수 있습니다.

        Returns:
            XML 형식의 스킬 요약 문자열.
        """
        all_skills: list[dict[str, str]] = self.list_skills(filter_unavailable=False)
        if not all_skills:
            return ""

        def escape_xml(s: str) -> str:
            return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

        lines = ["<skills>"]

        for s in all_skills:
            name: str = escape_xml(s["name"])
            path: str = s["path"]
            desc: str = escape_xml(self._get_skill_description(s["name"]))

            skill_meta: dict[str, Any] = self._get_skill_meta(s["name"])
            available: bool = self._check_requirements(skill_meta)

            source: str = s.get("source", "builtin")
            lines.append(f"  <skill available=\"{str(available).lower()}\" source=\"{source}\">")
            lines.append(f"    <name>{name}</name>")
            lines.append(f"    <description>{desc}</description>")
            lines.append(f"    <location>{path}</location>")

            # 사용할 수 없는 스킬에 대해 누락된 요구 사항을 표시합니다.
            if not available:
                missing: str = self._get_missing_requirements(skill_meta)
                if missing:
                    lines.append(f"    <requires>{escape_xml(missing)}</requires>")

            lines.append(f"  </skill>")
        lines.append("</skills>")

        return "\n".join(lines)

    def _get_skill_description(self, name: str) -> str:
        """스킬의 프론트매터에서 설명(description)을 가져옵니다."""
        meta: dict[str, str] = self.get_skill_metadata(name)
        if meta and meta.get("description"):
            return meta["description"]

        return name     # 스킬 이름으로 대체

    def _get_missing_requirements(self, skill_meta: dict[str, str]) -> str:
        """누락된 요구 사항에 대한 설명을 가져옵니다."""
        missing: list[str] = []

        requires: dict[str, list[str]] = skill_meta.get("requires", {})
        for b in requires.get("bins", []):
            if not shutil.which(b):
                missing.append(f"CLI: {b}")

        for env in requires.get("env", []):
            if not os.environ.get(env):
                missing.append(f"ENV: {env}")

        return ", ".join(missing)

    def get_skill_source(self, name: str) -> str | None:
        """스킬의 출처를 반환. 'builtin' 또는 'workspace'. 없으면 None."""
        for s in self.list_skills(filter_unavailable=False):
            if s["name"] == name:
                return s.get("source", "builtin")
        return None

    def get_always_skills(self) -> list[str]:
        """요구 사항을 충족하는, always=true로 표시된 스킬들을 가져옵니다."""
        result: list[str] = []

        for s in self.list_skills(filter_unavailable=True):
            meta: dict[str, str] = self.get_skill_metadata(s["name"]) or {}
            skill_meta: dict[str, Any] = self._parse_shacs_bot_metadata(meta.get("metadata", ""))
            if skill_meta.get("always") or meta.get("always"):
                result.append(s["name"])

        return result