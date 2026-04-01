# 작업 기록: evaluation harness self-eval trigger 추가

## 사용자 프롬프트

> gogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/autoloop.py` 신규 추가
  - `AutoEvalRunResult` 추가
  - `AutoEvalService` 추가
    - `prepare_trigger(session_key)`로 turn threshold / 최소 간격 / eval 세션 제외 판단
    - `run_auto_eval(...)`로 auto-run 실행 로직 캡슐화
    - `mark_trigger_failure(...)`로 trigger 실패 상태 기록
- `shacs_bot/evals/state.py` 수정
  - trigger 관련 상태 필드 추가
    - `trigger_enabled`
    - `trigger_turn_threshold`
    - `trigger_min_interval_minutes`
    - `trigger_session_limit`
    - `trigger_case_limit`
    - `trigger_variants`
    - `completed_turns_since_trigger`
    - `last_triggered_at`
    - `last_triggered_session_key`
    - `last_trigger_status`
    - `last_trigger_error`
- `shacs_bot/evals/__init__.py` 수정
  - `AutoEvalService`, `AutoEvalRunResult` export 추가
- `shacs_bot/agent/loop.py` 수정
  - `_auto_eval_task` background task slot 추가
  - `_maybe_schedule_auto_eval(session_key)` 추가
  - non-eval turn 저장 후 self-eval trigger 검사 및 background 실행
  - `eval:` session은 재귀 방지를 위해 자동 trigger 대상에서 제외

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/autoloop.py` clean
- `uv run python -c ...` 스모크 테스트
  - `prepare_trigger()`가 turn threshold 전에는 false, 충족 시 true 반환 확인
  - trigger state에 `completed_turns_since_trigger`, `last_trigger_status`, `last_triggered_session_key` 기록 확인
  - `AgentLoop._maybe_schedule_auto_eval()`가 background task를 생성하고 `AutoEvalService.run_auto_eval()`을 호출하는지 확인
  - `eval:` session key는 trigger 대상에서 제외되는지 확인

## 비고

- 이번 단계는 trigger MVP까지만 포함한다.
- 아직 cron 기반 주기 실행, health history 누적, 정책 자동 반영은 구현하지 않았다.
