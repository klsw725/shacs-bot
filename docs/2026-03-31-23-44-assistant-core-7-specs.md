# Assistant core 7 specs 작성

## 사용자 프롬프트

> assistant workflow planner 를 별로의 축으로 빼서 핵심 7개로 만들고 각자 스펙을 만들어

## 요약

assistant-first 기준으로 핵심 축을 7개로 고정했다.

1. Vector Memory / Hybrid Memory (`기존 spec 활용, assistant core 7 위치 명시`)
2. Lifecycle Hooks
3. Channel-aware Rendering Layer
4. Policy / Approval / Trust Model
5. Lightweight Background Workflows
6. Assistant Workflow Planner
7. Operator Console (CLI/TUI)

## 변경 파일

- `docs/specs/vector-memory/spec.md`
- `docs/specs/lifecycle-hooks/spec.md`
- `docs/specs/lifecycle-hooks/prds/lifecycle-hooks.md`
- `docs/specs/channel-rendering/spec.md`
- `docs/specs/channel-rendering/prds/channel-rendering.md`
- `docs/specs/policy-trust/spec.md`
- `docs/specs/policy-trust/prds/policy-trust.md`
- `docs/specs/background-workflows/spec.md`
- `docs/specs/background-workflows/prds/background-workflows.md`
- `docs/specs/assistant-workflow-planner/spec.md`
- `docs/specs/assistant-workflow-planner/prds/assistant-workflow-planner.md`
- `docs/specs/operator-console/spec.md`
- `docs/specs/operator-console/prds/operator-console.md`

## 메모

- 별도 인프라(예: Redis/Celery/Postgres/웹 대시보드)를 전제로 하지 않는 방향으로 정리했다.
- planner는 background workflow runtime과 분리된 축으로 다뤘다.
- quota/budget/observability는 이번 핵심 7개에는 포함하지 않고 policy / operator 축에 간접 반영했다.
