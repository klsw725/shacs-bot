# 작업 기록: evaluation harness scheduled trigger 추가

## 사용자 프롬프트

> gogogo

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - scheduled trigger 정책 필드 추가
    - `trigger_schedule_kind`
    - `trigger_schedule_every_minutes`
    - `trigger_schedule_cron_expr`
    - `trigger_schedule_tz`
    - `last_scheduled_job_id`
- `shacs_bot/evals/autoloop.py` 수정
  - `prepare_trigger()`가 `scheduled:` session도 제외하도록 수정
  - `sync_schedule(cron_service)` 추가
    - 기존 self-eval cron job 정리
    - `every` / `cron` 정책에 맞춰 self-eval job 등록
    - state에 `lastScheduledJobId` 저장
    - invalid schedule config로 early-return 할 때도 `lastScheduledJobId`를 비워 stale state가 남지 않도록 수정
- `shacs_bot/agent/loop.py` 수정
  - runtime policy / turn-trigger 모두 `scheduled:` 세션을 제외하도록 수정
- `shacs_bot/cli/commands.py` 수정
  - gateway runtime startup 시 `AutoEvalService.sync_schedule(cron)` 호출
  - cron callback이 `eval_trigger` metadata를 보면 일반 agent_turn 대신 self-eval 실행하도록 확장
  - scheduled run은 `scheduled:<job_id>` session key 사용

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/autoloop.py` clean
  - `shacs_bot/evals/state.py` clean
- `uv run python -c ...` 스모크 테스트
  - schedule 정책이 `every`면 self-eval cron job 등록 확인
  - schedule 정책이 `off`면 기존 self-eval job 제거 확인
  - state에 `lastScheduledJobId` 기록 확인
  - scheduled self-eval은 `scheduled:<job_id>` session key를 사용하는 호출 경로 확인
  - invalid schedule config로 빠질 때도 `lastScheduledJobId == ''`로 정리되고 cron store가 비어 있는지 확인

## 비고

- 이번 단계는 conservative scheduled trigger만 포함한다.
- 실제 cron scheduling은 gateway runtime startup 경로에서만 연결했다.
- 아직 schedule 정책을 CLI에서 편집하는 명령은 추가하지 않았다.
