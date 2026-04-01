# 작업 기록: evaluation harness default policy 튜닝

## 사용자 프롬프트

> 커밋 후 default policy 다듬어

## 브랜치

- `feature/evaluation-harness-m1`

## 커밋

- `97be403` — `feat(evals): add autonomous self-eval control loop`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - autonomous self-eval 기본 정책을 더 보수적으로 조정
    - `trigger_turn_threshold`: `5 → 12`
    - `trigger_min_interval_minutes`: `30 → 90`
    - `trigger_session_limit`: `10 → 6`
    - `trigger_case_limit`: `20 → 12`
    - `trigger_variants`: `['default'] → ['default', 'strict-completion']`

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
- `uv run python -c ...` 스모크 테스트
  - `AutoEvalState()` 기본값이 조정한 값으로 반영되는지 확인

## 비고

- autonomous self-eval은 기본 활성화 상태를 유지하되, 빈도와 샘플 크기를 줄여 더 보수적으로 동작하게 조정했다.
- `strict-completion`은 상대적으로 안전한 비교 축이라 기본 trigger variant에 포함했다.
