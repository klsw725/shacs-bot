# 작업 기록: evaluation harness auto-run 및 state 추가

## 사용자 프롬프트

> ㄱㅡ래 진행

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 신규 추가
  - `AutoEvalState` 모델 추가
  - `get_eval_state_path(workspace)` 추가
  - `write_auto_eval_state(workspace, state)` 추가
- `shacs_bot/evals/__init__.py` 수정
  - state 관련 export 추가
- `shacs_bot/cli/commands.py` 수정
  - `_create_eval_runtime(config)` helper 추가
  - `eval run`이 공용 runtime helper를 재사용하도록 정리
  - `eval extract` 기본 출력이 timestamped auto file이 되도록 변경
  - `eval auto-run` 서브커맨드 추가
    - default cases 로드
    - 최근 세션에서 case 자동 추출
    - auto-run용 case bundle 저장
    - `EvaluationRunner` 실행
    - `workspace/evals/state.json` 갱신

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
  - `shacs_bot/evals/__init__.py` clean
  - `shacs_bot/evals/extractor.py` clean
- `uv run python -c ...` 스모크 테스트
  - timestamped auto cases path 생성 확인
  - `AutoEvalState` 저장/재로드 확인
  - fake `typer` + fake runtime 기반 `eval_extract()` 실행 확인
  - `eval_auto_run()`이 default + extracted cases를 실행하는지 확인
  - `workspace/evals/runs/<run_id>/summary.json` 생성 확인
  - `workspace/evals/state.json`에 `extractedCaseCount`, `totalCaseCount`, `lastCasesPath` 저장 확인

## 비고

- 이번 단계는 self-eval loop의 실행 엔진 MVP까지만 포함한다.
- 아직 자동 스케줄링, baseline 비교, regression health 계산, 정책 자동 반영은 구현하지 않았다.
