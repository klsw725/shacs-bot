"""Git 저장소에서 에이전트 + 스킬 번들을 workspace에 설치한다."""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from loguru import logger


@dataclass
class InstallRecord:
    """설치된 에이전트 1건의 기록."""

    name: str
    git_url: str
    installed_at: str
    commit: str = ""
    skills: list[str] = field(default_factory=list)


@dataclass
class InstallResult:
    """설치 결과."""

    success: bool
    message: str
    agents: list[str] = field(default_factory=list)
    skills: list[str] = field(default_factory=list)


class AgentInstaller:
    """Git 저장소에서 에이전트를 workspace에 설치/관리한다."""

    MANIFEST_FILE = ".installed.json"

    def __init__(self, workspace: Path):
        self._workspace = workspace
        self._agents_dir = workspace / "agents"
        self._skills_dir = workspace / "skills"
        self._manifest_path = self._agents_dir / self.MANIFEST_FILE

    # ── install ─────────────────────────────────────────────────

    async def install(self, git_url: str) -> InstallResult:
        """Git URL에서 에이전트를 설치한다."""
        # git 확인
        if not shutil.which("git"):
            return InstallResult(success=False, message="git이 설치되어 있지 않습니다.")

        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = Path(tmp)
            clone_dir = tmp_path / "repo"

            # 1. shallow clone
            try:
                subprocess.run(
                    ["git", "clone", "--depth", "1", git_url, str(clone_dir)],
                    capture_output=True, text=True, check=True, timeout=60,
                )
            except subprocess.CalledProcessError as e:
                return InstallResult(success=False, message=f"git clone 실패: {e.stderr.strip()}")
            except subprocess.TimeoutExpired:
                return InstallResult(success=False, message="git clone 타임아웃 (60초)")

            # 커밋 해시
            commit = self._get_commit(clone_dir)

            # 2. 구조 감지
            agents_found, skills_found = self._detect_structure(clone_dir)

            if not agents_found:
                return InstallResult(
                    success=False,
                    message="설치 가능한 에이전트를 찾을 수 없습니다. agent.toml 또는 agents/*.toml이 필요합니다.",
                )

            # 3. TOML 검증
            import tomllib
            validated_agents: list[tuple[Path, str]] = []  # (src_path, name)
            for toml_path in agents_found:
                try:
                    with open(toml_path, "rb") as f:
                        data = tomllib.load(f)
                    name = data.get("name")
                    desc = data.get("description")
                    instructions = data.get("developer_instructions")
                    if not all([name, desc, instructions]):
                        return InstallResult(
                            success=False,
                            message=f"TOML 필수 필드 누락: {toml_path.name} (name, description, developer_instructions 필요)",
                        )
                    validated_agents.append((toml_path, name))
                except Exception as e:
                    return InstallResult(success=False, message=f"TOML 파싱 실패: {toml_path.name} — {e}")

            # 4. 이름 충돌 체크
            manifest = self._load_manifest()
            existing_names = {r.name for r in manifest}
            for _, name in validated_agents:
                if name in existing_names:
                    return InstallResult(
                        success=False,
                        message=f"에이전트 '{name}'이 이미 설치되어 있습니다. 먼저 `/agent remove {name}` 후 다시 시도하세요.",
                    )

            # 5. 복사
            self._agents_dir.mkdir(parents=True, exist_ok=True)
            installed_agents: list[str] = []
            for src_path, name in validated_agents:
                dst = self._agents_dir / src_path.name
                shutil.copy2(src_path, dst)
                installed_agents.append(name)

            installed_skills: list[str] = []
            for skill_dir in skills_found:
                skill_name = skill_dir.name
                dst = self._skills_dir / skill_name
                if dst.exists():
                    shutil.rmtree(dst)
                shutil.copytree(skill_dir, dst)
                installed_skills.append(skill_name)

            # 6. 매니페스트 기록
            now = datetime.now(timezone.utc).isoformat()
            for name in installed_agents:
                manifest.append(InstallRecord(
                    name=name,
                    git_url=git_url,
                    installed_at=now,
                    commit=commit,
                    skills=installed_skills,
                ))
            self._save_manifest(manifest)

            # 결과
            parts: list[str] = ["✅ 에이전트 설치 완료"]
            for name in installed_agents:
                parts.append(f"  에이전트: {name}")
            for skill in installed_skills:
                parts.append(f"  스킬: {skill}")
            parts.append(f"  출처: {git_url}")
            parts.append("  위치: workspace (ApprovalGate 적용)")

            return InstallResult(
                success=True,
                message="\n".join(parts),
                agents=installed_agents,
                skills=installed_skills,
            )

    # ── list ────────────────────────────────────────────────────

    def list_installed(self) -> list[InstallRecord]:
        """설치된 에이전트 목록을 반환한다."""
        return self._load_manifest()

    # ── remove ──────────────────────────────────────────────────

    def remove(self, name: str) -> str:
        """설치된 에이전트를 삭제한다."""
        manifest = self._load_manifest()
        record = next((r for r in manifest if r.name == name), None)
        if not record:
            return f"설치된 에이전트 '{name}'을 찾을 수 없습니다."

        # TOML 파일 삭제
        for toml_file in self._agents_dir.glob("*.toml"):
            try:
                import tomllib
                with open(toml_file, "rb") as f:
                    data = tomllib.load(f)
                if data.get("name") == name:
                    toml_file.unlink()
                    break
            except Exception:
                continue

        # 연관 스킬 삭제
        removed_skills: list[str] = []
        for skill_name in record.skills:
            skill_dir = self._skills_dir / skill_name
            if skill_dir.exists():
                shutil.rmtree(skill_dir)
                removed_skills.append(skill_name)

        # 매니페스트 업데이트
        manifest = [r for r in manifest if r.name != name]
        self._save_manifest(manifest)

        parts: list[str] = [f"🗑 에이전트 '{name}' 삭제됨"]
        if removed_skills:
            parts.append(f"  연관 스킬도 삭제: {', '.join(removed_skills)}")
        return "\n".join(parts)

    # ── update ──────────────────────────────────────────────────

    async def update(self, name: str | None = None) -> str:
        """설치된 에이전트를 최신 버전으로 업데이트한다."""
        manifest = self._load_manifest()
        if not manifest:
            return "설치된 에이전트가 없습니다."

        targets = [r for r in manifest if name is None or r.name == name]
        if not targets:
            return f"설치된 에이전트 '{name}'을 찾을 수 없습니다."

        results: list[str] = []
        # URL별로 그룹핑 (같은 repo에서 여러 에이전트가 설치된 경우)
        urls_processed: set[str] = set()
        for record in targets:
            if record.git_url in urls_processed:
                continue
            urls_processed.add(record.git_url)

            # 삭제 후 재설치
            same_url_agents = [r for r in manifest if r.git_url == record.git_url]
            for r in same_url_agents:
                self.remove(r.name)

            result = await self.install(record.git_url)
            results.append(result.message)

        return "\n\n".join(results)

    # ── 내부 헬퍼 ──────────────────────────────────────────────

    def _detect_structure(self, clone_dir: Path) -> tuple[list[Path], list[Path]]:
        """저장소 구조를 감지한다. (에이전트 TOML 목록, 스킬 디렉토리 목록)"""
        agents: list[Path] = []
        skills: list[Path] = []

        # 컬렉션 모드: agents/*.toml
        agents_subdir = clone_dir / "agents"
        if agents_subdir.is_dir():
            agents.extend(sorted(agents_subdir.glob("*.toml")))

        # 단일 모드: agent.toml (루트)
        single_toml = clone_dir / "agent.toml"
        if single_toml.exists() and not agents:
            agents.append(single_toml)

        # 스킬: skills/*/SKILL.md
        skills_subdir = clone_dir / "skills"
        if skills_subdir.is_dir():
            for skill_dir in sorted(skills_subdir.iterdir()):
                if skill_dir.is_dir() and (skill_dir / "SKILL.md").exists():
                    skills.append(skill_dir)

        return agents, skills

    @staticmethod
    def _get_commit(clone_dir: Path) -> str:
        """현재 커밋 해시를 가져온다."""
        try:
            result = subprocess.run(
                ["git", "rev-parse", "--short", "HEAD"],
                cwd=clone_dir, capture_output=True, text=True, check=True,
            )
            return result.stdout.strip()
        except Exception:
            return ""

    def _load_manifest(self) -> list[InstallRecord]:
        """매니페스트 파일을 로드한다."""
        if not self._manifest_path.exists():
            return []
        try:
            data = json.loads(self._manifest_path.read_text(encoding="utf-8"))
            return [InstallRecord(**r) for r in data.get("installed", [])]
        except Exception as e:
            logger.warning("매니페스트 로드 실패: {}", e)
            return []

    def _save_manifest(self, records: list[InstallRecord]) -> None:
        """매니페스트 파일을 저장한다."""
        self._agents_dir.mkdir(parents=True, exist_ok=True)
        data = {"installed": [
            {
                "name": r.name,
                "git_url": r.git_url,
                "installed_at": r.installed_at,
                "commit": r.commit,
                "skills": r.skills,
            }
            for r in records
        ]}
        self._manifest_path.write_text(
            json.dumps(data, ensure_ascii=False, indent=2),
            encoding="utf-8",
        )
