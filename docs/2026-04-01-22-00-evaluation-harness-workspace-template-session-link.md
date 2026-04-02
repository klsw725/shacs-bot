# 작업 기록: evaluation harness workspace 템플릿 정렬 및 session 연계

## 사용자 프롬프트

> 그럼 마저 구현해

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/templates/evals/cases/default.json` 신규 추가
  - eval 샘플 케이스를 패키지 템플릿 자산으로 이동
- `shacs_bot/evals/cases.json` 삭제
  - repo 루트 예제 파일 역할 제거
- `shacs_bot/utils/helpers.py` 수정
  - `sync_workspace_template()`가 `workspace/evals/cases/default.json`을 생성하도록 추가
- `shacs_bot/evals/models.py` 수정
  - `EvaluationResult`에 `session_key` 필드 추가
- `shacs_bot/evals/runner.py` 수정
  - `get_default_cases_path(workspace)` 추가
  - optional `session_manager` 주입 추가
  - eval 세션 키 생성 로직 분리
  - eval session metadata (`type`, `run_id`, `variant`, `case_id`, `expected_mode`, `tags`) 저장 추가
- `shacs_bot/evals/__init__.py` 수정
  - `get_default_cases_path` export 추가
- `shacs_bot/cli/commands.py` 수정
  - `eval run`의 `cases` 인자를 optional로 변경
  - cases 미지정 시 `workspace/evals/cases/default.json` 기본 사용
  - eval CLI도 `SessionManager`를 생성해 `AgentLoop` 및 `EvaluationRunner`에 연결
  - 출력에 실제 cases 경로 표시 추가
- `pyproject.toml` 수정
  - templates 자산이 sdist/wheel에 포함되도록 hatch build 설정 보정

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/runner.py` clean
  - `shacs_bot/evals/models.py` clean
  - `shacs_bot/evals/__init__.py` clean
  - `shacs_bot/utils/helpers.py`는 기존 기반 타입 이슈 유지
- `uv run python -c ...` 스모크 테스트
  - `sync_workspace_template()`로 `workspace/evals/cases/default.json` 생성 확인
  - 기본 cases loader 로드 확인
  - runner가 `sessionKey`를 result에 기록하는지 확인
  - session metadata 저장 확인
  - fake `typer` 기반 `eval_run(cases=None, ...)` 호출 시 workspace 기본 케이스로 실행되는지 확인
  - 실제 run directory 및 `summary.json` 생성 확인
- `uv build`
  - sdist/wheel 빌드 성공 확인
  - built artifact 내부에 `shacs_bot/templates/evals/cases/default.json` 포함 확인
  - wheel에서 template 엔트리가 중복 없이 1회만 포함되는 것 확인

## 비고

- workspace reorganization 원칙에 맞춰 LLM/사용자가 다루는 eval cases는 workspace 아래로, 세션 저장은 기존 `data/sessions` 체계를 유지하도록 정렬했다.
- raw session을 eval 입력으로 직접 쓰지는 않고, eval 실행이 기존 session 체계 안에서 `eval:` namespace로 기록되도록 연결했다.
