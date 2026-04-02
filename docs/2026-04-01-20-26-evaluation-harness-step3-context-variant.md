# 작업 기록: Evaluation Harness Step 3 context variant 추가

## 사용자 프롬프트

> 고고고

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/agent/context.py` 수정
  - `ContextVariant` dataclass 추가
  - `build_system_prompt(..., variant=None)` 추가
  - `build_messages(..., variant=None)` 추가
  - `build_runtime_context(..., variant=None)` 추가
  - `environment_bootstrap=False`일 때 identity의 워크스페이스 상세 안내 축소
  - `environment_bootstrap=False`일 때 runtime context에서 channel/chat id 생략
  - `context_profile="minimal"`일 때 memory/skills/agents 요약 생략
  - `completion_policy="strict"`일 때 완료 전 검증/불확실성 명시 지침 추가
- `shacs_bot/agent/loop.py` 수정
  - `_process_message(..., variant=None)` 추가
  - `process_direct(..., variant=None)` 추가
  - context 메시지 생성 시 variant 전달

## 검증

- `uv run python -c ...` 스모크 테스트:
  - 기본 prompt에 memory가 포함되는지 확인
  - `context_profile="minimal"`에서 memory/skills 요약이 빠지는지 확인
  - `completion_policy="strict"`에서 strict 안내가 붙는지 확인
  - `environment_bootstrap=False`에서 workspace 상세 안내가 줄고 bootstrap 파일은 유지되는지 확인
  - `build_runtime_context()`에서 bootstrap-off 시 channel/chat id가 빠지는지 확인
  - `build_messages()`가 strict variant를 system prompt에 반영하는지 확인
- `lsp_diagnostics`:
  - `shacs_bot/agent/context.py` 에러 없음, 경고만 존재
  - `shacs_bot/agent/loop.py`는 기존 기반 경고/오류 다수 유지

## 비고

- 이번 단계는 PRD Step 3 범위만 반영했다.
- runner preset 해석이나 CLI 연결은 아직 구현하지 않았다.
