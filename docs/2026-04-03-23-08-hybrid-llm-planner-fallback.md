# 하이브리드 LLM 플래너 폴백 구현

**날짜**: 2026-04-03  
**브랜치**: feature/planned-workflow-executor

---

## 사용자 프롬프트

> 1. TASK: Implement the smallest safe hybrid planner: keep the current rule-based planner as the fast path, and add an LLM-based planner fallback only when the rule-based logic would otherwise return `direct_answer` for a non-trivial request that may need planning.
> 2. EXPECTED OUTCOME: A surgical multi-file change where the assistant can call an LLM planner fallback, parse the response into `AssistantPlan`, preserve backward compatibility, and keep existing fast-path behavior for clearly matched rule-based cases. Include deterministic smoke verification where possible and the required dated docs work-log file under `docs/` with the user's prompt text.
> 3. REQUIRED TOOLS: read, grep, apply_patch, lsp_diagnostics, bash. Use bash only for targeted compile/smoke verification.
> 4. MUST DO: Reuse existing provider invocation patterns already used in `AgentLoop`. Keep the current `_classify_request()` or equivalent rule-based path for confident matches. Add a minimal async fallback path that asks the model for a structured `AssistantPlan` response and validates/parses it with existing Pydantic models. If the fallback output is invalid, safely fall back to `direct_answer`. Do not remove description fallback or current step metadata conventions. Add deterministic unit/smoke coverage for fallback gating logic and at least one controlled fallback path by stubbing provider output. Update the smallest relevant planner eval/scenario asset if it can be done without adding flakiness. Add the dated docs work log.
> 5. MUST NOT DO: Do not rewrite planner architecture. Do not add external dependencies. Do not make all requests go through the model. Do not commit. Do not use type-suppression hacks. Do not break existing direct_answer / clarification / planned_workflow behavior for current covered rule-based cases.
> 6. CONTEXT: Current work already includes step-based execution, structured `step_meta`, heuristic expansion, and planner→workflow E2E smoke. The next step is to reduce regex-only limits by adding an LLM fallback for ambiguous/unhandled non-trivial requests while keeping deterministic fast-path behavior.

---

## 변경 개요

규칙 기반 플래너를 그대로 유지하면서, 규칙 기반이 `direct_answer`를 반환하는 비자명한 요청에 대해 LLM에 구조화된 `AssistantPlan` JSON을 요청하는 비동기 폴백 경로를 추가했다.

---

## 변경 파일

### `shacs_bot/agent/loop.py`

**추가된 메서드 (4개):**

1. `_LLM_PLANNER_SYSTEM` (클래스 상수)  
   LLM 플래너 폴백에 사용하는 시스템 프롬프트. `AssistantPlan` JSON 구조와 라우팅 규칙을 명시한다.

2. `_is_nontrivial_for_llm_fallback(text: str) -> bool` (staticmethod)  
   LLM 폴백 호출 게이팅 조건. 텍스트 길이 ≥ 30자일 때만 True. 규칙 기반이 이미 15자 미만을 필터링하므로 30자 임계값으로 추가 걸러낸다.

3. `_llm_classify_fallback(user_text: str) -> AssistantPlan | None` (async)  
   `self._provider.chat_with_retry()`를 호출하여 JSON 응답을 받고, Pydantic `AssistantPlan.model_validate()`로 파싱한다. 네트워크 오류, 비정상 JSON, 빈 응답, 빈 steps 등 모든 실패 경우에 `None`을 반환한다. 마크다운 코드 블록으로 감싼 JSON도 처리한다.

4. `_classify_request_with_llm_fallback(user_text: str) -> AssistantPlan` (async)  
   오케스트레이터. `_classify_request()` 규칙 기반을 먼저 호출하고, 결과가 `direct_answer` + 비자명 조건을 만족할 때만 LLM 폴백을 시도한다. 폴백이 `direct_answer`를 반환하거나 실패하면 원래 결과를 유지한다.

**변경된 호출 지점:**

- `_process_message` 내 `self._classify_request(msg.content)` → `await self._classify_request_with_llm_fallback(msg.content)`

---

## 새 파일

### `scripts/smoke_llm_planner_fallback.py`

LLM 폴백 게이팅 로직을 위한 결정론적 스모크 테스트 (12개 검증):

| # | 검증 내용 |
|---|-----------|
| 1 | 규칙 기반 `wait_until` → LLM 미호출 |
| 2 | 규칙 기반 `clarification` → LLM 미호출 |
| 3 | 짧은 텍스트(<30자) → LLM 미호출 |
| 4 | 비자명 요청 → LLM 폴백 호출 → `planned_workflow` 파싱 |
| 5 | LLM 무효 JSON → `direct_answer` 안전 폴백 |
| 6 | LLM 빈 응답 → `direct_answer` 안전 폴백 |
| 7 | LLM `error` finish_reason → `direct_answer` 안전 폴백 |
| 8 | LLM `planned_workflow` + 빈 steps → `direct_answer` 안전 폴백 |
| 9 | LLM 마크다운 코드블록 JSON → 파싱 성공 |
| 10 | LLM `direct_answer` 반환 → 원래 결과 유지 |
| 11 | `_is_nontrivial_for_llm_fallback` 경계: 30자=True, 29자=False |
| 12 | 규칙 기반 sequential → LLM 미호출 |

스텁 프로바이더 `_StubProvider`로 실제 LLM 없이 실행 가능.

---

## 설계 결정

- **외부 의존성 없음**: 기존 `LLMProvider.chat_with_retry()` 패턴을 그대로 재사용.
- **아키텍처 불변**: `_classify_request()`는 수정하지 않았다. 빠른 경로는 그대로 유지.
- **안전 우선**: 폴백이 어떤 형태로든 실패하면 항상 `direct_answer`로 복귀.
- **게이팅 조건**: 30자 임계값으로 단순 인사/단문 질의에 LLM을 낭비하지 않는다.
- **온도 0.0 + max_tokens 512**: 플래닝 결정은 결정론적이어야 하므로 temperature를 낮게, 응답이 짧아도 충분하므로 토큰을 제한한다.
