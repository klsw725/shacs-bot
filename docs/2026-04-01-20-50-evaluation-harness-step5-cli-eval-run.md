# 작업 기록: Evaluation Harness Step 5 CLI `eval run` 추가

## 사용자 프롬프트

> 그래그래그래

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/cli/commands.py` 수정
  - `eval_app = typer.Typer(...)` 추가
  - `app.add_typer(eval_app, name="eval")` 추가
  - `eval run` 서브커맨드 추가
    - `cases` 입력 파일 경로
    - `--variant` 반복 옵션
    - `--case` 필터 옵션
    - `--output` 출력 디렉터리 옵션
  - 기존 `agent` 커맨드와 동일한 방식으로 config/provider/AgentLoop 초기화
  - `load_cases_file()` / `resolve_variant()` / `EvaluationRunner` 연결
  - summary path 출력
  - variant별 결과를 rich `Table`로 출력

## 검증

- 정적 확인
  - `commands.py` 전체는 기존부터 basedpyright 이슈가 많아 파일 단위 clean 검증은 불가
  - 새 추가 구간은 읽기 검토로 옵션 처리, runner 연결, 출력 흐름 확인
- `uv run python -c ...` 스모크 테스트
  - fake `typer` 모듈을 주입해 `commands.py`를 import 가능하게 구성
  - `eval_run(..., variant=['default'])` 직접 호출 성공 확인
  - output 디렉터리 생성 확인
  - `--case missing` 경로에서 `Exit(1)` 확인

## 비고

- 현재 실행 환경에는 `typer`가 없어 실제 CLI import 기반 e2e 실행은 할 수 없었다.
- 따라서 Step 5 검증은 fake `typer` 주입 기반의 직접 함수 호출 smoke로 수행했다.
