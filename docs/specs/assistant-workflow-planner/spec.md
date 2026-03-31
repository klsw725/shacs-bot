# SPEC: Assistant Workflow Planner

> **Prompt**: shacs-bot이 장기 assistant 작업을 계획·질문·후속조치까지 이어갈 수 있도록, 코딩 에이전트용 플래너가 아닌 assistant workflow planner를 추가한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`assistant-workflow-planner.md`](./prds/assistant-workflow-planner.md) | intent→plan 변환, clarification, follow-up step planning |

## TL;DR

> **목적**: 사용자의 요청을 "지금 바로 답할 것"과 "계획을 세워 이어서 실행할 것"으로 구분하고, 후속 작업 계획을 구조화한다.
>
> **Deliverables**:
> - `agent/planner.py` — assistant-specific Plan, Step, Clarification 모델
> - `agent/loop.py` — planner 진입점 및 direct answer bypass
> - `workflow/runtime.py` — planner 산출물 소비(배경 workflow와 연결)
> - `agent/session/manager.py` — current plan metadata 저장
>
> **Estimated Effort**: Medium (4-6시간)

## 현재 상태 분석

- 현재는 AgentLoop가 메시지를 받아 바로 응답/도구/서브에이전트 호출로 흘러간다.
- 복합 요청도 session metadata에 명시적 plan 구조를 남기지 않는다.
- cron/heartbeat가 있어도, "이 요청을 어떤 단계로 풀어야 하는가"를 구조화하는 planner는 없다.

assistant 제품에서 planner가 필요한 경우는 이런 요청이다.

1. "내일 오전 회의 전에 요약 보내줘"
2. "자료 찾아보고 정리해서 나중에 보내줘"
3. "먼저 초안 만들고, 내가 승인하면 발송해"

이건 코딩 에이전트식 task decomposition이 아니라, **후속조치 중심 assistant planning**이다.

## 설계

### Planner의 역할

1. direct answer 가능 여부 판정
2. clarification 필요 여부 판정
3. step list 생성
4. background workflow 필요 여부 판정
5. notify 시점/채널 결정

### Plan 모델

```python
@dataclass
class AssistantPlan:
    goal: str
    steps: list[PlanStep]
    needs_clarification: bool = False
    requires_background: bool = False
    notify_target: dict[str, str] | None = None
```

`PlanStep.type` 예시:

- `ask_user`
- `research`
- `summarize`
- `wait_until`
- `request_approval`
- `send_result`

### Planner 경계

- planner는 **무엇을 할지** 정한다.
- background workflow runtime은 **언제/어떻게 이어서 실행할지** 담당한다.

### 비목표

- 코드 리팩토링/파일 작업 플랜 생성
- 고중량 multi-agent planner
- 사용자가 요청하지 않은 speculative task creation

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/planner.py` | 신규 | AssistantPlan, PlanStep, planner entry |
| `shacs_bot/agent/loop.py` | 수정 | direct answer vs planning 경로 분기 |
| `shacs_bot/agent/session/manager.py` | 수정 | current_plan/session metadata 저장 |
| `shacs_bot/workflow/runtime.py` | 수정 | planner output 소비 |
| `shacs_bot/config/schema.py` | 수정 | planner on/off 및 heuristics 설정 |

## 검증 기준

- [ ] 단순 질의는 planner를 우회하고 즉답한다
- [ ] 복합 assistant 요청은 구조화된 step list를 만든다
- [ ] clarification이 필요한 경우에만 추가 질문을 한다
- [ ] planner 산출물이 background workflow runtime으로 연결된다

## Must NOT

- planner가 코딩 에이전트식 todo orchestration으로 변질되지 않는다.
- 단순 대화까지 과도하게 구조화하지 않는다.
