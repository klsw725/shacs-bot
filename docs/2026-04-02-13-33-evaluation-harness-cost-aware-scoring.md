# 작업 기록: evaluation harness cost-aware scoring 추가

## 사용자 프롬프트

> 1 하고 3해줘

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 수정
  - `VariantHealth`에 `avg_tool_calls`, `avg_total_tokens` 추가
- `shacs_bot/evals/state.py` 수정
  - `compare_to_baseline()`가 `VariantSummary`의 tool/tokens 정보를 `VariantHealth`에 반영하도록 수정
  - `calculate_weighted_score()`가 tool call 수와 total token 수에 따른 비용 penalty를 반영하도록 수정
  - `apply_weighted_scores()`가 cost-aware score를 계산하도록 수정
- `shacs_bot/cli/commands.py` 수정
  - `eval auto-run`, `eval status` 출력에 `Avg Tools`, `Avg Tokens` 컬럼 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/models.py` clean
- `uv run python -c ...` 스모크 테스트
  - 같은 success rate라도 tool/token 비용이 큰 variant가 더 낮은 weighted score를 받는지 확인
  - cost penalty가 `strict-completion` 같은 variant의 추천/비활성 판단에 영향을 주는지 확인

## 비고

- 이번 단계는 단순한 비용 penalty만 포함한다.
- tool call은 최대 0.15, total tokens는 최대 0.15까지 감점한다.
- 아직 provider별 실제 비용 단가나 채널별 중요도는 반영하지 않았다.
