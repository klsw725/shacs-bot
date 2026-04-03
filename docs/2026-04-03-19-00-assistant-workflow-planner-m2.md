# Assistant Workflow Planner M2: AgentLoop Planning 분기 추가

**날짜**: 2026-04-03  
**브랜치**: `feature/assistant-workflow-planner-m1`  
**관련 PRD**: `docs/specs/assistant-workflow-planner/prds/assistant-workflow-planner.md`

---

## 사용자 프롬프트

> H: 1. TASK: Implement Assistant Workflow Planner PRD M2 only on the current branch by adding AgentLoop planning entry rules for direct answer vs clarification vs planned workflow.
> 2. EXPECTED OUTCOME: Make the minimal surgical changes needed so `shacs_bot/agent/loop.py` classifies the current user message before entering the existing agent loop, preserves the direct-answer path, returns a clarification question when the request is obviously underspecified, and returns a structured step-based plan for clearly multi-step/compound requests. Add the required dated docs work-log file for this M2 work.
> 3. REQUIRED TOOLS: read, grep, lsp_symbols, lsp_diagnostics, apply_patch.
> 4. MUST DO: Read the assistant-workflow-planner PRD, `shacs_bot/agent/loop.py`, and the existing `shacs_bot/agent/planner.py` models first. Keep the implementation M2-only and rule-based/minimal if that is the smallest fit. Follow repo conventions from AGENTS.md. Ensure direct-answer requests still go through the existing `_run_agent_loop` path unchanged. For planning branches, use the existing planner models to produce either a clarification response or a structured step list response. Keep changes surgical. Add a docs/YYYY-MM-DD-HH-mm-*.md work log including the user prompt text.
> 5. MUST NOT DO: Do not implement M3 session metadata persistence, do not touch workflow runtime/store, do not add config/schema fields, do not add type suppression, do not refactor unrelated logic, and do not commit.
> 6. CONTEXT: M1 was already committed on this branch (`a3e8edd`). Current relevant files: `docs/specs/assistant-workflow-planner/prds/assistant-workflow-planner.md`, `shacs_bot/agent/planner.py`, and `shacs_bot/agent/loop.py`. The smallest acceptable M2 is a local planning classifier that detects clearly simple requests vs clarification-needed requests vs compound/planning-needed requests and branches before `_run_agent_loop`.

---

## 변경 내용

### `shacs_bot/agent/loop.py`

**변경 1: planning classifier 주입** (`_process_message` 내부)

`_run_agent_loop` 호출 직전에 분류기를 삽입. 미디어 첨부가 있는 메시지는 분류 없이 기존 경로로 바이패스.

- `clarification` → 분류기가 생성한 질문을 `OutboundMessage`로 즉시 반환
- `planned_workflow` → 구조화된 step 계획을 `OutboundMessage`로 즉시 반환  
- `direct_answer` → 기존 `_run_agent_loop` 경로 그대로 통과

**변경 2: `_classify_request` 정적 메서드 추가**

규칙 기반 분류기. 우선순위 순서:

1. 짧은 메시지 (< 15자) 또는 슬래시 명령 → `direct_answer`
2. 완전히 모호한 순수 지시어 (`그거 해줘`, `do it` 등) → `clarification`  
3. 복합/다단계 패턴 감지 → `planned_workflow`
   - 영문 순차 패턴: `first … then`, `step N`, `after that`, `followed by`
   - 한국어 순차 패턴: `먼저 … 그 다음`, `그 다음에`, `이후에`, `단계별`
   - 스케줄링 패턴: `매일`, `매주`, `every day`, `schedule`, `remind`
   - 번호 목록 2줄 이상
4. 나머지 → `direct_answer` (기본값, 과도한 분류 방지)

**변경 3: `_format_plan` 정적 메서드 추가**

`AssistantPlan`을 마크다운 텍스트로 변환. step별 kind, 설명, 선행 의존성, notify 여부 표시.

---

## 설계 결정

- **한국어 15자 임계값**: 영문 40자 기준은 한국어에서 과도하게 넓음. 한국어 문자는 의미 밀도가 높아 15자로 조정.
- **미디어 메시지 바이패스**: 이미지/파일 첨부 메시지는 내용 분석이 불가하므로 분류 없이 LLM으로 직접 전달.
- **clarification 매우 보수적**: 순수 대명사+동사 형태만 감지. 과도한 clarification 질문 방지.
- **planned_workflow 기본 step 3개**: M2는 계획 생성만, 실행은 M3(workflow runtime handoff)에서 담당.
- **top-level import**: `planner.py`는 `loop.py`를 역참조하지 않으므로 순환 임포트 없이 파일 상단 import로 단순하게 유지.

---

## M2 범위 (준수)

- ✅ `_run_agent_loop` 직전 분기 추가
- ✅ direct_answer 경로 완전 보존
- ✅ M1 모델(`AssistantPlan`, `PlanStep`) 재사용
- ✅ workflow runtime/store 미접촉
- ✅ session metadata 저장 미구현 (M3 범위)
- ✅ config/schema 변경 없음
- ✅ 커밋 없음
