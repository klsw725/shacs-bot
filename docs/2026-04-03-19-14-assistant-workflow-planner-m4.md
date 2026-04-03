# Assistant Workflow Planner M4 — 시나리오 검증 eval 케이스

**날짜**: 2026-04-03  
**브랜치**: `feature/assistant-workflow-planner-m1`  
**스코프**: M4 전용

---

## 사용자 프롬프트

> H: 1. TASK: Implement Assistant Workflow Planner PRD M4 only on the current branch in the smallest codebase-consistent way, so the repository has explicit, reusable verification scenarios covering direct-answer, clarification, and planned-workflow behavior.
> 2. EXPECTED OUTCOME: Add the minimal code and data needed so M4 scenarios are represented as runnable verification assets, not just ad-hoc notes. Prefer extending the existing eval harness/case format minimally over inventing a new framework. Include a dated docs work-log file for this M4 work.
> 3. REQUIRED TOOLS: read, glob, grep, lsp_symbols, lsp_diagnostics, apply_patch.
> 4. MUST DO: Read the assistant-workflow-planner PRD, `shacs_bot/evals/models.py`, `shacs_bot/evals/runner.py`, the existing default eval cases JSON, `shacs_bot/agent/loop.py`, and the current M1-M3 outputs first. Keep changes surgical. The resulting M4 assets must be able to distinguish the three branches meaningfully; merely checking that a response exists is not enough. Reuse the current eval harness if that is the cleanest minimal fit. Follow AGENTS.md rules. Add a docs/YYYY-MM-DD-HH-mm-*.md work log including the user prompt text.
> 5. MUST NOT DO: Do not refactor unrelated eval infrastructure, do not change planner behavior just to make tests easier, do not add heavyweight testing frameworks, do not commit, and do not touch unrelated specs.
> 6. CONTEXT: Branch `feature/assistant-workflow-planner-m1` already has M1-M3 committed. Existing eval harness currently supports `expected_mode`, but M4 requires scenario verification that meaningfully differentiates direct answer vs clarification vs planned workflow. Existing project norms favor smoke/eval assets over pytest.

---

## 변경 파일

### `shacs_bot/evals/models.py`

`EvaluationCase`에 `expected_response_pattern: str = ""` 필드 추가.

- 설정 시 `final_response`에서 `re.search`로 매칭 여부로 성공/실패 판정
- 기존 `expected_mode` 판정보다 **우선** 적용됨
- 빈 문자열(기본값)이면 기존 `expected_mode` 로직으로 폴백 → 하위 호환 완전 유지

### `shacs_bot/evals/runner.py`

- `import re` 추가
- `_classify_status`에서 `expected_response_pattern` 분기 삽입 (infra_error 체크 직후):

  ```python
  if case.expected_response_pattern:
      matched = bool(re.search(case.expected_response_pattern, final_response, re.DOTALL))
      return "success" if matched else "task_failure"
  ```

### `shacs_bot/templates/evals/cases/planner-scenarios.json` (신규)

M4 검증 시나리오 7개:

| case_id | 입력 패턴 | 예상 분기 | 판정 기준 |
|---------|----------|----------|----------|
| `planner-direct-001` | `"안녕!"` | `direct_answer` | 비어 있지 않은 응답 (`expected_mode: response`) |
| `planner-direct-002` | `"오늘 날씨 어때?"` | `direct_answer` | 비어 있지 않은 응답 |
| `planner-clarification-001` | `"그거 해줘"` | `clarification` | 응답에 `구체적\|무엇인지\|알려주` 포함 |
| `planner-clarification-002` | `"이거 처리해줘"` | `clarification` | 응답에 `구체적\|무엇인지\|알려주` 포함 |
| `planner-workflow-001` | `"먼저 … 그 다음에 …"` | `planned_workflow` | 응답에 `📋\|처리 계획\|[research]\|[summarize]` 포함 |
| `planner-workflow-002` | `"매일 아침 … 보내줘"` | `planned_workflow` | 응답에 계획 마커 포함 |
| `planner-workflow-003` | 번호 목록 3줄 | `planned_workflow` | 응답에 계획 마커 포함 |

---

## 설계 결정

| 결정 | 이유 |
|------|------|
| `expected_response_pattern` 필드 추가 | 기존 `expected_mode` Literal 확장 없이 하위 호환 유지하면서 내용 기반 판정 가능 |
| clarification 패턴 `"구체적\|무엇인지\|알려주"` | `_classify_request`가 `clarification_question="요청이 무엇인지 좀 더 구체적으로 알려주시겠어요?"` 하드코딩 → 패턴이 해당 문자열에 안정적으로 매칭 |
| planned_workflow 패턴 `"📋\|처리 계획\|..."` | `_format_plan`이 `"📋 **처리 계획**"` 헤더와 step kind 마커를 고정 출력 → 플래너 출력임을 신뢰성 있게 검증 |
| direct_answer는 `expected_mode: response` 유지 | LLM 직답의 내용은 비결정적이므로 패턴 지정 불가. `planned_workflow/clarification` 케이스가 잘못된 분기(direct_answer로 폴백)에서 실패하므로 전체적으로 세 분기 모두 구분 가능 |
| 기존 케이스 파일(`default.json`) 미수정 | M4는 별도 파일(`planner-scenarios.json`)로 격리. 기존 smoke 케이스 영향 없음 |
| 플래너 로직 미수정 | MUST NOT DO 준수 — eval harness 측에서 판정 기준만 추가 |

---

## 검증

```
$ uv run python -c "from shacs_bot.evals.runner import load_cases_file; ..."
→ 7개 케이스 로드 성공

$ uv run python -c "(분기 판정 로직 직접 검증)"
→ direct_answer / clarification / planned_workflow 모든 분기 판정 통과
```

---

## 실행 방법

기존 eval CLI로 M4 시나리오 파일 지정 실행:

```bash
uv run python -m shacs_bot.cli eval run \
  --cases shacs_bot/templates/evals/cases/planner-scenarios.json
```

---

## M4 완료 기준 (PRD) 대응

| PRD Acceptance Criteria | 대응 케이스 |
|------------------------|-----------|
| direct answer 경로가 유지된다 | `planner-direct-001`, `planner-direct-002` |
| 복합 요청은 step 기반 계획을 남긴다 | `planner-workflow-001~003` |
| clarification은 필요한 경우에만 발생한다 | `planner-clarification-001~002` |
