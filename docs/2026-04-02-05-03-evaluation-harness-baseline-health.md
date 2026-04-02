# 작업 기록: evaluation harness baseline 비교 및 variant health 추가

## 사용자 프롬프트

> gogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/models.py` 수정
  - `VariantHealth` 모델 추가
- `shacs_bot/evals/state.py` 수정
  - `AutoEvalState`에 `baseline_run_id`, `baseline_run_dir`, `variant_health`, `regressions` 추가
  - `read_auto_eval_state()` 추가
  - `read_run_summary()` 추가
  - `calculate_success_rate()` 추가
  - `compare_to_baseline()` 추가
- `shacs_bot/evals/__init__.py` 수정
  - baseline/health 관련 export 추가
- `shacs_bot/cli/commands.py` 수정
  - `eval auto-run`에 `--baseline`, `--compare/--no-compare` 추가
  - baseline summary와 현재 summary 비교 로직 추가
  - state에 baseline/health/regression 저장 추가
  - 출력 테이블에 `Δ Success`, `Health` 컬럼 추가
  - 같은 초 재실행 시에도 baseline이 유지되도록 실제 `run_dir.name` 기준으로 state 저장하도록 수정
  - custom `--output` 사용 시에도 `baseline_run_dir`를 따라 baseline summary를 읽도록 수정
  - `Δ Success`를 퍼센트 형식으로 표시하도록 수정

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/models.py` clean
  - `shacs_bot/evals/__init__.py` clean
- `uv run python -c ...` 스모크 테스트
  - `compare_to_baseline()`이 healthy / regression 판정하는지 확인
  - `AutoEvalState` + `VariantHealth` 저장/재로드 확인
  - baseline auto-run 1회 실행 후 baseline state 저장 확인
  - follow-up auto-run에서 baseline 대비 success 하락 시 regression 감지 확인
  - `workspace/evals/state.json`에 `regressions`와 `variantHealth` 반영 확인
  - 실제 run directory suffix(`-1`)가 붙는 경우에도 baseline 비교가 유지되는지 확인
  - custom output directory에 baseline run을 저장한 경우에도 follow-up run이 해당 baseline을 정상 참조하는지 확인

## 비고

- 이번 단계는 baseline 비교와 lightweight health 저장까지만 포함한다.
- 아직 scheduler, history 누적 health, 정책 자동 반영은 구현하지 않았다.
