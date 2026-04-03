# Planned Workflow Executor PRD 초안 작성

**날짜**: 2026-04-03  
**브랜치**: `feature/planned-workflow-executor`

---

## 사용자 프롬프트

> 이제 더 할께 있어?
>
> 다음 작업 추천
>
> PRD가 있는 작업이야?
>
> 그래
>
> 진행 시켜

---

## 작업 내용

- `docs/specs/planned-workflow-executor/spec.md` 추가
- `docs/specs/planned-workflow-executor/prds/planned-workflow-executor.md` 추가
- `docs/specs/planned-workflow-executor/prds/planned-workflow-executor-m1.md` 추가
- `docs/specs/planned-workflow-executor/checklists/requirements.md` 추가

## 정리한 방향

- 기존 `assistant-workflow-planner` PRD는 planning / handoff까지로 닫혀 있어, step executor는 별도 workstream으로 분리
- 전체 PRD와 M1 최소 구현 범위를 분리해 바로 다음 단일 작업을 선택할 수 있게 구성
- 기존 `docs/specs/<feature>/spec.md`, `prds/*.md`, `checklists/requirements.md` 패턴을 그대로 따름

## M1 초안 핵심

- `research -> summarize -> send_result` 3-step path 우선
- current step metadata 저장
- manual redispatch가 goal 재실행이 아니라 executor 진입으로 연결되도록 정의

## 후속 기대 효과

- 다음 구현 작업을 새 PRD 기준으로 시작 가능
- planner step taxonomy와 runtime execution 사이의 공백을 문서적으로 메움
