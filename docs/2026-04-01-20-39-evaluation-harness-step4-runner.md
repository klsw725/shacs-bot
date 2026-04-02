# 작업 기록: Evaluation Harness Step 4 runner 추가

## 사용자 프롬프트

> 그래그래

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/runner.py` 신규 추가
  - `load_cases_file()` JSON case loader 추가
  - `resolve_variant()` preset factory 추가
  - `TraceCollector` 추가
  - `build_variant_summary()` 추가
  - `EvaluationRunner` 추가
    - `run_cases()`에서 manifest/result/trace/summary orchestration
    - `_run_case()`에서 `process_direct(..., observer=..., variant=...)` 실행
    - timeout / provider exception을 `infra_error`로 분류
    - expected_mode 기준으로 success / task_failure 분류
- `shacs_bot/evals/__init__.py` 수정
  - runner 관련 export 추가

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/runner.py` clean
  - `shacs_bot/evals/__init__.py` clean
- `uv run python -c ...` 스모크 테스트
  - JSON case 로드 확인
  - variant preset 해석 확인
  - result/trace/manifest/summary 파일 생성 확인
  - tool_use 성공 분류 확인
  - timeout → `infra_error` 분류 확인
  - provider exception → `infra_error` 분류 확인

## 비고

- 이번 단계는 Step 4 범위만 반영했다.
- CLI subcommand 연결은 아직 구현하지 않았다.
- `failure_expected`는 PRD에 성공 판정 규칙이 명확하지 않아, 현재는 non-infra 상태에서 빈 최종 응답일 때 success로 처리했다.
