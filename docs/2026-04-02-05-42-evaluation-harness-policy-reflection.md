# 작업 기록: evaluation harness policy reflection 추가

## 사용자 프롬프트

> go

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 수정
  - `VariantHealth`에 `disabled`, `recommended` 필드 추가
- `shacs_bot/evals/state.py` 수정
  - `AutoEvalState`에 `variant_history` 추가
  - `compute_trigger_variants()` 추가
  - `append_variant_history()` 추가
- `shacs_bot/evals/__init__.py` 수정
  - policy reflection 관련 export 추가
- `shacs_bot/evals/autoloop.py` 수정
  - auto-run 종료 후 `variant_health`를 기반으로 다음 `trigger_variants` 계산
  - regression variant는 future trigger variants에서 제외
  - `default`는 항상 fallback으로 유지
  - `variant_history`에 run별 snapshot 누적
- `shacs_bot/cli/commands.py` 수정
  - `eval auto-run`이 inline 로직 대신 `AutoEvalService.run_auto_eval()`을 직접 사용하도록 통일
  - auto-run 출력에 `next trigger variants`, `disabled`, `recommended` 표시 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/autoloop.py` clean
  - `shacs_bot/evals/models.py` clean
- `uv run python -c ...` 스모크 테스트
  - `compute_trigger_variants()`가 regression variant를 제외하는지 확인
  - 모든 variant가 disable되어도 `default`가 fallback으로 유지되는지 확인
  - `append_variant_history()`가 run별 snapshot을 누적하는지 확인
  - baseline summary가 있는 상태에서 regression variant가 발생하면 state의 `triggerVariants`가 `['default']`로 바뀌는지 확인
  - `variant_history`가 누적되는지 확인
  - CLI `eval auto-run` 경로가 `AutoEvalService.run_auto_eval()`로 위임되는지 확인
  - CLI 출력에서 `next trigger variants`, `disabled`, `recommended`가 표시되는지 확인

## 비고

- 이번 단계는 lightweight policy reflection까지만 포함한다.
- 아직 provider/model 선택 정책, health history 기반 decay, 다단계 variant prioritization은 구현하지 않았다.
