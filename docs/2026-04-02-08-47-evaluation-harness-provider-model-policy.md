# 작업 기록: evaluation harness provider/model policy 반영

## 사용자 프롬프트

> gogogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - `recommended_provider_name`, `recommended_model` 추가
  - `compute_recommended_provider_model()` 추가
- `shacs_bot/evals/__init__.py` 수정
  - provider/model policy helper export 추가
- `shacs_bot/evals/autoloop.py` 수정
  - auto-run 결과가 healthy하고 회귀가 없을 때 현재 provider/model 조합을 recommendation으로 state에 저장
- `shacs_bot/cli/commands.py` 수정
  - `_make_provider()`가 `provider == auto`일 때 `recommendedModel`을 우선 적용하도록 수정
  - provider 매칭 실패 시 `find_by_name(None)`로 죽지 않도록 안전 처리
  - `eval status`에 `Recommended Provider`, `Recommended Model` 표시 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
- `uv run python -c ...` 스모크 테스트
  - `compute_recommended_provider_model()` helper 결과 확인
  - auto-run 이후 state에 `recommendedProviderName`, `recommendedModel` 저장 확인
  - fake `typer` 주입 환경에서 `_make_provider()`가 auto mode일 때 추천 model을 사용하는지 확인

## 비고

- 이번 단계는 conservative provider/model recommendation만 포함한다.
- 실제로는 healthy한 현재 provider/model 조합을 "고정 추천"하는 수준이며, 아직 여러 provider/model 후보를 비교해 선택하는 cross-provider evaluation은 구현하지 않았다.
