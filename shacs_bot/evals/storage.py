from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path

from pydantic import BaseModel

from shacs_bot.config.paths import get_workspace_path
from shacs_bot.evals.models import EvaluationResult, RunManifest, RunSummary, TraceArtifact
from shacs_bot.utils.helpers import ensure_dir, safe_filename


class EvaluationStorage:
    def __init__(self, workspace: Path | None = None) -> None:
        root: Path = workspace or get_workspace_path()
        self._runs_dir: Path = ensure_dir(root / "evals" / "runs")

    def create_run_dir(self, base_dir: Path | None = None, run_id: str | None = None) -> Path:
        runs_dir: Path = ensure_dir(base_dir) if base_dir else self._runs_dir
        candidate: str = run_id or self._build_run_id()
        run_dir: Path = runs_dir / candidate
        suffix: int = 1

        while run_dir.exists():
            run_dir = runs_dir / f"{candidate}-{suffix}"
            suffix += 1

        return ensure_dir(run_dir)

    def write_manifest(self, run_dir: Path, manifest: RunManifest) -> Path:
        path: Path = run_dir / "manifest.json"
        self._write_json(path, manifest)
        return path

    def write_result(self, run_dir: Path, variant: str, result: EvaluationResult) -> Path:
        variant_dir: Path = ensure_dir(run_dir / safe_filename(variant))
        filename: str = f"{safe_filename(result.case_id)}.result.json"
        path: Path = variant_dir / filename
        self._write_json(path, result)
        return path

    def write_trace(self, run_dir: Path, variant: str, case_id: str, trace: TraceArtifact) -> Path:
        variant_dir: Path = ensure_dir(run_dir / safe_filename(variant))
        filename: str = f"{safe_filename(case_id)}.trace.json"
        path: Path = variant_dir / filename
        self._write_json(path, trace)
        return path

    def write_summary(self, run_dir: Path, summary: RunSummary) -> Path:
        path: Path = run_dir / "summary.json"
        self._write_json(path, summary)
        return path

    def _build_run_id(self) -> str:
        return datetime.now().strftime("%Y-%m-%d-%H-%M-%S")

    def _write_json(
        self,
        path: Path,
        payload: RunManifest | EvaluationResult | TraceArtifact | RunSummary | dict[str, object],
    ) -> None:
        serializable: dict[str, object]
        if isinstance(payload, BaseModel):
            serializable = payload.model_dump(mode="json")
        else:
            serializable = payload

        tmp_path: Path = path.with_name(f"{path.name}.tmp")
        with tmp_path.open("w", encoding="utf-8") as file:
            json.dump(serializable, file, ensure_ascii=False, indent=2)
            _ = file.write("\n")

        _ = tmp_path.replace(path)
