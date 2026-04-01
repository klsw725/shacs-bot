# 작업 기록: evaluation harness self-eval policy CLI 추가

## 사용자 프롬프트

> 그래

## 브랜치

- `feature/evaluation-harness-m1`

## 변경 사항

- `shacs_bot/evals/state.py` 수정
  - `update_auto_eval_state(workspace, **updates)` helper 추가
- `shacs_bot/evals/__init__.py` 수정
  - `update_auto_eval_state` export 추가
- `shacs_bot/cli/commands.py` 수정
  - `eval status` 추가
    - 현재 self-eval 상태/trigger/schedule/recommended runtime/variant health 출력
  - `eval policy` 추가
    - trigger enable/disable
    - turn threshold
    - min interval
    - session/case limit
    - trigger variants
    - schedule kind/every/cron/tz
    를 state 파일에 반영

## 검증

- `lsp_diagnostics`
  - `shacs_bot/evals/state.py` clean
- `uv run python -c ...` 스모크 테스트
  - `eval policy`로 trigger/schedule/variant 설정 저장 확인
  - `every` → `cron` schedule 업데이트 확인
  - `eval status`가 stored state를 읽어 출력하는지 확인
  - 잘못된 `--schedule-kind` 입력 시 `Exit(1)` 되는지 확인

## 비고

- 이번 단계로 self-eval 정책을 state 파일 수동 수정 없이 CLI에서 관리할 수 있게 됐다.
- 아직 schedule sync를 직접 재실행하는 별도 apply/reload 명령은 추가하지 않았다.
