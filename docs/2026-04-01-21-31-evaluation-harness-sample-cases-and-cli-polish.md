# 작업 기록: evaluation harness 샘플 케이스 추가 및 CLI 보정

## 사용자 프롬프트

> 1후 2

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/cases.json` 신규 추가
  - response 1개, tool_use 2개의 샘플 케이스 추가
  - camelCase JSON 형식으로 `EvaluationCase` 스키마에 맞춤
- `shacs_bot/evals/runner.py` 수정
  - `last_run_dir` property 추가
  - `run_cases()` 실행 시 실제 생성된 run directory를 보관하도록 변경
- `shacs_bot/cli/commands.py` 수정
  - `eval run`의 `cases` 인자 help 추가
  - summary path 출력 시 미리 추정한 경로 대신 runner가 실제로 생성한 run directory를 사용

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/runner.py` clean
- `uv run python -c ...` 스모크 테스트
  - `shacs_bot/evals/cases.json`을 `load_cases_file()`로 정상 로드 확인
  - 샘플 케이스 개수/expectedMode 확인
  - fake `typer` 주입 기반 `eval_run()` 직접 호출 성공 확인
  - 실제 run directory 아래 `summary.json` 생성 확인
  - `case_id` 미스매치 시 `Exit(1)` 확인

## 비고

- sample file 위치는 repo에 별도 examples/fixtures 관례가 없어 `shacs_bot/evals/`와 함께 두는 방식으로 맞췄다.
- CLI 보정은 Step 5 범위를 넘지 않도록 실제 run path 출력 정확도만 개선했다.
