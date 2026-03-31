# Assistant core 7 Speckit 정렬

## 사용자 프롬프트

> spec은 다음 기준으로 작성이 되야 하고 각 spec을 실제 구현 태스크로 더 쪼갠개 PRD 여야 해 PRD는 기존에 작성된 애들 참고하면 되고

## 요약

assistant core 7 스펙을 Speckit 스타일 요구에 맞춰 보강했다.

- 각 spec에 다음 섹션을 보강/추가:
  - User Scenarios & Testing
  - Functional Requirements
  - Key Entities
  - Success Criteria
  - Assumptions
- 각 feature 디렉토리에 `checklists/requirements.md` 추가
- 짧았던 PRD들을 기존 repo 스타일에 맞는 구현 태스크 문서로 확장
  - 문제
  - 해결책
  - 사용자 영향
  - 기술적 범위
  - 변경 단계
  - 성공 기준
  - 마일스톤
  - 위험 및 완화
  - Acceptance Criteria

## 변경 파일

- `docs/specs/lifecycle-hooks/spec.md`
- `docs/specs/lifecycle-hooks/prds/lifecycle-hooks.md`
- `docs/specs/lifecycle-hooks/checklists/requirements.md`
- `docs/specs/channel-rendering/spec.md`
- `docs/specs/channel-rendering/prds/channel-rendering.md`
- `docs/specs/channel-rendering/checklists/requirements.md`
- `docs/specs/policy-trust/spec.md`
- `docs/specs/policy-trust/prds/policy-trust.md`
- `docs/specs/policy-trust/checklists/requirements.md`
- `docs/specs/background-workflows/spec.md`
- `docs/specs/background-workflows/prds/background-workflows.md`
- `docs/specs/background-workflows/checklists/requirements.md`
- `docs/specs/assistant-workflow-planner/spec.md`
- `docs/specs/assistant-workflow-planner/prds/assistant-workflow-planner.md`
- `docs/specs/assistant-workflow-planner/checklists/requirements.md`
- `docs/specs/operator-console/spec.md`
- `docs/specs/operator-console/prds/operator-console.md`
- `docs/specs/operator-console/checklists/requirements.md`
- `docs/specs/vector-memory/spec.md`
- `docs/specs/vector-memory/checklists/requirements.md`

## 메모

- 저장소에는 `.specify/` 템플릿 파일이 없어서, 사용자 제공 Speckit 기준을 현재 `docs/specs/` 구조 위에 맞춰 반영했다.
- `vector-memory`는 기존 PRD가 이미 상세해서 spec만 Speckit 요구에 맞게 보강했다.
