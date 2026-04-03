# PR 초안: Assistant Workflow Planner

## 사용자 프롬프트

> PR 내용 작성해서 Md로 만들어줘

---

## PR 제목

`feat: assistant workflow planner 추가 및 workflow handoff/eval 검증 연결`

## PR 본문

```md
## 요약

- assistant 전용 planner 모델을 추가해 요청을 direct answer, clarification, planned workflow 경로로 분류합니다.
- 다단계 요청은 lightweight planning 분기로 보내고, planning 상태를 session metadata에 저장한 뒤 기존 workflow runtime으로 handoff 되도록 연결합니다.
- 시나리오 기반 eval 검증을 추가하고, planner spec/PRD 문서를 실제 구현 상태와 맞게 동기화합니다.

## 변경 사항

### Planner 기반 추가
- `shacs_bot/agent/planner.py`에 `AssistantPlan`, `PlanStep`, `ClarificationResult`, planner step taxonomy 추가

### Agent loop planning 분기 추가
- `shacs_bot/agent/loop.py`에 요청 분류 로직 추가
- 단순 요청은 기존 direct-answer 경로 유지
- 명백히 모호한 요청은 clarification 질문 반환
- 복합 요청과 일정성 요청은 구조화된 plan 반환

### Session metadata 및 workflow handoff
- `Session.metadata`에 `last_planning_result`, `current_plan` 저장
- direct-answer 턴에서는 stale `current_plan` 제거
- `WorkflowRuntime`에 `notify_target`과 직렬화된 plan metadata를 포함한 planned workflow 등록

### Eval 검증 추가
- eval harness에 optional `expectedResponsePattern` 지원 추가
- direct-answer / clarification / planned-workflow 시나리오를 담은 `shacs_bot/templates/evals/cases/planner-scenarios.json` 추가

### 문서 동기화
- Assistant Workflow Planner PRD의 M1-M4 마일스톤 완료 처리
- 실제 구현과 일치하도록 spec/PRD 문서 갱신

## 배경

기존에는 assistant 요청이 명시적인 planning 산출물 없이 바로 응답/도구 실행으로 흘러갔습니다. 그 결과 단순 요청과 복합 요청의 처리 경계가 일관되지 않았고, clarification 발생 기준도 모호했으며, 후속 실행 상태를 추적하기 어려웠습니다.

이 PR은 별도의 무거운 실행 프레임워크를 도입하지 않고, 최소한의 assistant planner 계층을 추가합니다. 단순 요청은 그대로 즉답하되, clarification이나 단계적 처리가 필요한 요청은 구조화된 planning 상태를 남기고 기존 workflow runtime으로 handoff 할 수 있게 합니다.

## 검증

- `uv run python` 기반 planner model import / instantiation smoke check
- direct-answer / clarification / planned-workflow 분기 classifier 검증
- `notify_target` 및 plan metadata 저장에 대한 workflow handoff smoke test
- `shacs_bot/templates/evals/cases/planner-scenarios.json` 기반 재사용 가능한 eval 시나리오 검증

## 참고

- 이 PR은 planning과 handoff까지 포함하지만, dedicated planned-workflow executor까지는 포함하지 않습니다.
- planned workflow는 기록되고 노출되지만, 실제 executor 동작은 후속 PR에서 확장할 수 있습니다.
```

---

## 포함된 커밋

- `a3e8edd` feat(planner): add assistant workflow plan models
- `0bec81d` feat(planner): classify requests before agent execution
- `221f48c` feat(planner): persist planning state and workflow handoff
- `dc591df` feat(planner): add scenario-based eval coverage
- `a79af53` docs(planner): sync spec and PRD with delivered work
