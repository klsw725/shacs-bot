# 작업 기록: evaluation harness recovery/decay 정책 추가

## 사용자 프롬프트

> gogogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - `get_variant_status_history()` 추가
  - `has_recent_regression()` 추가
  - `is_stably_healthy()` 추가
  - `compute_trigger_variants()`가 최근 regression 이력이 있는 variant를 즉시 복귀시키지 않도록 수정
  - `compute_recommended_runtime_variant()`가 안정적인 healthy 이력이 있을 때만 `strict-completion`을 추천하도록 수정
- `shacs_bot/evals/autoloop.py` 수정
  - trigger/runtime 추천 계산 시 `previous_state.variant_history`를 반영하도록 수정
- `shacs_bot/evals/__init__.py` 수정
  - recovery/decay helper export 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
- `uv run python -c ...` 스모크 테스트
  - recent regression 이력이 있으면 한 번 healthy로는 `strict-completion`이 바로 복귀하지 않는지 확인
  - 연속 healthy 이력이 쌓이면 다시 trigger/runtime 추천에 들어오는지 확인
  - baseline summary + prior regression history가 있을 때 `triggerVariants == ['default']`로 유지되는지 확인
  - `recommendedRuntimeVariant == 'default'`로 유지되는지 확인

## 비고

- 이번 단계는 conservative recovery/decay만 포함한다.
- recovery는 연속 healthy 기반의 단순 규칙이고, 아직 score decay나 장기 통계 기반 weighting은 적용하지 않았다.
