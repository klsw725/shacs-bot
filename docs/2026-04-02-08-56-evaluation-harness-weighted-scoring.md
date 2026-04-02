# 작업 기록: evaluation harness weighted scoring 추가

## 사용자 프롬프트

> [SYSTEM DIRECTIVE: TODO CONTINUATION]

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 수정
  - `VariantHealth`에 `weighted_score` 필드 추가
- `shacs_bot/evals/state.py` 수정
  - `calculate_weighted_score()` 추가
  - `apply_weighted_scores()` 추가
  - weighted score를 trigger/runtime/provider 추천 게이트에 반영하도록 수정
    - low score variant는 `disabled=True` 처리 가능
    - `strict-completion` 추천은 충분한 weighted score가 있을 때만 허용
- `shacs_bot/evals/autoloop.py` 수정
  - baseline 비교 후 `apply_weighted_scores()` 적용
- `shacs_bot/cli/commands.py` 수정
  - `eval auto-run`, `eval status` 출력에 `Score` 컬럼 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
  - `shacs_bot/evals/models.py` clean
- `uv run python -c ...` 스모크 테스트
  - recent regression/warning history가 weighted score를 낮추는지 확인
  - low score variant가 trigger/runtime 추천에서 제외되는지 확인
  - auto-run 후 stored `weightedScore`가 반영되는지 확인

## 비고

- 이번 단계는 lightweight weighted scoring만 포함한다.
- 점수는 current success rate + baseline delta + 최근 status penalty를 합성하는 단순 모델이다.
- 아직 token cost, tool-call cost, channel importance 같은 추가 가중치는 반영하지 않았다.
