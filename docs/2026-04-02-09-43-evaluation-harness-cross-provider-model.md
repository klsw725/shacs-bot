# 작업 기록: evaluation harness cross-provider/model 비교 추가

## 사용자 프롬프트

> gogogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 수정
  - `EvalCandidate` 모델 추가
  - `VariantHealth`에 `weighted_score` 필드 추가
- `shacs_bot/evals/state.py` 수정
  - `candidate_scores`, `candidate_best` 필드 추가
  - `build_candidate_key()`, `calculate_candidate_score()`, `select_best_candidate()` 추가
  - weighted scoring helper 추가
- `shacs_bot/evals/autoloop.py` 수정
  - `persist_state=False` 지원
  - candidate score 저장 로직 추가
- `shacs_bot/cli/commands.py` 수정
  - `_resolve_runtime_model()` 추가
  - `_make_provider()` / `_create_eval_runtime()`가 explicit provider/model override를 받을 수 있도록 확장
  - `eval auto-run --candidate provider:model` 반복 옵션 추가
  - candidate별 runtime을 생성해 순차 평가 후 best candidate를 state에 반영
  - `eval status`에 `Best Candidate`, candidate score 출력 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
  - `shacs_bot/evals/models.py` clean
- `uv run python -c ...` 스모크 테스트
  - helper(`build_candidate_key`, `calculate_candidate_score`, `select_best_candidate`) 동작 확인
  - `eval auto-run --candidate anthropic:model-a --candidate anthropic:model-b` 시나리오에서 best candidate가 state에 저장되는지 확인
  - `recommendedProviderName`, `recommendedModel`이 최고 점수 후보로 갱신되는지 확인
  - CLI 출력에 `best candidate`가 표시되는지 확인

## 비고

- 이번 단계는 conservative cross-provider/model comparison만 포함한다.
- candidate runtime은 CLI auto-run 경로에서만 명시적으로 실행된다.
- autonomous trigger/scheduled self-eval은 여전히 현재 runtime 기준으로만 동작한다.
