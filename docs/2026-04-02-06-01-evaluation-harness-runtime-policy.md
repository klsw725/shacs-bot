# 작업 기록: evaluation harness runtime policy 적용

## 사용자 프롬프트

> go

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - `AutoEvalState`에 `recommended_runtime_variant` 추가
  - `compute_recommended_runtime_variant()` 추가
- `shacs_bot/evals/__init__.py` 수정
  - runtime policy helper export 추가
- `shacs_bot/evals/autoloop.py` 수정
  - auto-run 종료 시 `recommended_runtime_variant` 계산 및 state 저장
  - 현재는 안전한 자동 적용 대상만 허용하도록 `strict-completion`만 추천 가능
- `shacs_bot/agent/loop.py` 수정
  - `_runtime_policy_variant(session_key)` 추가
  - non-eval session에서 explicit variant가 없으면 persisted runtime policy를 읽어 `ContextVariant(completion_policy="strict")` 적용
  - `eval:` session은 runtime policy 자동 적용 대상에서 제외

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
- `uv run python -c ...` 스모크 테스트
  - `compute_recommended_runtime_variant()`가 healthy한 `strict-completion`을 추천하는지 확인
  - auto-run 이후 state에 `recommendedRuntimeVariant == "strict-completion"` 저장 확인
  - `AgentLoop._runtime_policy_variant('cli:demo')`가 strict completion variant를 반환하는지 확인
  - `AgentLoop._runtime_policy_variant('eval:run')`는 `None`을 반환하는지 확인

## 비고

- 이번 단계는 conservative runtime policy만 포함한다.
- `minimal-context`, `bootstrap-off`처럼 컨텍스트를 줄이는 variant는 자동 적용하지 않는다.
- 현재 자동 적용 대상은 `strict-completion`만 허용한다.
