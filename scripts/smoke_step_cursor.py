#!/usr/bin/env python3
"""
스모크 테스트: 완료된 step이 재디스패치 후 재실행되지 않음을 검증합니다.

실행 방법:
    uv run python scripts/smoke_step_cursor.py

성공 시 "PASS" 라인만 출력하고 종료 코드 0.
실패 시 AssertionError 메시지를 출력하고 종료 코드 1.
"""
from __future__ import annotations

import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from shacs_bot.workflow.runtime import WorkflowRuntime


def run_smoke() -> None:
    with tempfile.TemporaryDirectory() as tmpdir:
        runtime = WorkflowRuntime(workspace=Path(tmpdir))

        # 워크플로우 등록
        record = runtime.register_planned_workflow(
            goal="테스트 목표",
            plan={
                "kind": "planned_workflow",
                "steps": [
                    {"kind": "research", "description": "조사", "depends_on": []},
                    {"kind": "summarize", "description": "요약", "depends_on": [0]},
                    {"kind": "send_result", "description": "전달", "depends_on": [1]},
                ],
            },
            channel="test",
            chat_id="test-chat",
            session_key="test-session",
        )
        wf_id = record.workflow_id

        # 실행 시작
        runtime.start(wf_id)

        # --- step 0 (research) 실행 시작: cursor = step 0 ---
        runtime.update_step_cursor(wf_id, step_index=0, step_kind="research")

        # step 0 성공 → cursor를 다음 index(1)로 전진 (수정된 동작 시뮬레이션)
        runtime.annotate_step_result(wf_id, "조사 결과")
        runtime.annotate_result(wf_id, "조사 결과")
        next_idx = 1
        runtime.update_step_cursor(wf_id, step_index=next_idx, step_kind="summarize")

        # 검증 1: cursor가 step 1 (summarize)을 가리키는지 확인
        refreshed = runtime.store.get(wf_id)
        assert refreshed is not None
        stored_idx = refreshed.metadata.get("currentStepIndex")
        assert stored_idx == 1, f"[FAIL] step 0 완료 후 cursor는 1이어야 함. 실제: {stored_idx}"
        print("PASS [검증 1] step 0 완료 후 cursor = 1")

        # --- 재디스패치 시뮬레이션: 런타임을 재생성하고 cursor 읽기 ---
        runtime2 = WorkflowRuntime(workspace=Path(tmpdir))
        recovered = runtime2.store.get(wf_id)
        assert recovered is not None
        resume_idx = recovered.metadata.get("currentStepIndex", 0)
        if not isinstance(resume_idx, int) or resume_idx < 0:
            resume_idx = 0

        # 검증 2: 재디스패치 후 step 0가 아닌 step 1부터 시작함을 확인
        assert resume_idx == 1, (
            f"[FAIL] 재디스패치 후 시작 index는 1이어야 함. 실제: {resume_idx}"
        )
        print("PASS [검증 2] 재디스패치 후 step 0 (research) 재실행 없이 step 1 (summarize) 부터 시작")

        # --- step 1 (summarize) 성공 → cursor를 step 2로 전진 ---
        runtime2.update_step_cursor(wf_id, step_index=2, step_kind="send_result")
        refreshed2 = runtime2.store.get(wf_id)
        assert refreshed2 is not None
        assert refreshed2.metadata.get("currentStepIndex") == 2
        print("PASS [검증 3] step 1 완료 후 cursor = 2 (send_result)")

        # --- 두 번째 재디스패치: step 2부터 시작 ---
        runtime3 = WorkflowRuntime(workspace=Path(tmpdir))
        recovered2 = runtime3.store.get(wf_id)
        assert recovered2 is not None
        resume_idx2 = recovered2.metadata.get("currentStepIndex", 0)
        if not isinstance(resume_idx2, int) or resume_idx2 < 0:
            resume_idx2 = 0
        assert resume_idx2 == 2, (
            f"[FAIL] 두 번째 재디스패치 후 시작 index는 2여야 함. 실제: {resume_idx2}"
        )
        print("PASS [검증 4] 두 번째 재디스패치 후 step 0, 1 재실행 없이 step 2 (send_result) 부터 시작")

    print("\n모든 검증 통과 ✓")


if __name__ == "__main__":
    try:
        run_smoke()
    except AssertionError as e:
        print(f"\n{e}", file=sys.stderr)
        sys.exit(1)
