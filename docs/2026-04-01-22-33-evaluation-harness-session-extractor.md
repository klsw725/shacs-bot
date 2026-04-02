# 작업 기록: evaluation harness session extractor 추가

## 사용자 프롬프트

> 구현해

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/extractor.py` 신규 추가
  - `get_auto_cases_dir(workspace)` 추가
  - `build_auto_cases_path(workspace, name)` 추가
  - `SessionCaseExtractor` 추가
    - 최근 세션 목록 조회
    - 기본적으로 `eval:` 세션 제외
    - user 메시지 턴만 추출
    - `workspace/evals/cases/auto/*.json` 형식으로 저장
- `shacs_bot/evals/models.py` 수정
  - `EvaluationCase`에 source metadata 필드 추가
    - `source_session_key`
    - `source_message_index`
    - `source_timestamp`
    - `source_channel`
- `shacs_bot/evals/__init__.py` 수정
  - extractor 관련 export 추가
- `shacs_bot/cli/commands.py` 수정
  - `eval extract` 서브커맨드 추가
    - `--session`
    - `--session-limit`
    - `--case-limit`
    - `--output`
    - `--include-eval-sessions`

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/extractor.py` clean
  - `shacs_bot/evals/models.py` clean
  - `shacs_bot/evals/__init__.py` clean
- `uv run python -c ...` 스모크 테스트
  - fake session manager로 user 턴 2개 추출 확인
  - `sourceSessionKey`, `sourceMessageIndex`, `sourceChannel` 저장 확인
  - 저장된 auto case file을 `load_cases_file()`로 재로드 확인
  - fake `typer` 기반 `eval_extract(...)` 호출 시 `workspace/evals/cases/auto/auto-extract.json` 생성 확인

## 비고

- 이번 단계는 extractor MVP만 구현했다.
- 아직 자동 스케줄링, baseline 비교, self-eval 정책 반영은 구현하지 않았다.
