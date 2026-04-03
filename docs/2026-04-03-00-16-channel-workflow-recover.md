# Channel Workflow Recover

## 사용자 프롬프트

- "2번인데 큐잉 시스템으로 할 수 있을 때 돌린다"
- "진행시켜"

## 변경 요약

- `shacs_bot/workflow/runtime.py` 수정
  - `ManualRecoverResult` 추가
  - `manual_recover()` 추가
    - `running`, `waiting_input`, `retry_wait`만 `queued`로 복원
    - `queued`는 idempotent no-op 처리
    - `completed`, `failed`는 거부
    - 60초 cooldown 적용
    - `lastManualRecoverAt`, `lastManualRecoverBy*`, `recoverCount`, `recoverSource` audit metadata 기록
- `shacs_bot/agent/loop.py` 수정
  - `/workflow recover <id>` 채널 명령 추가
  - `/workflow recover all` 명시적 차단
  - `/help`에 recover 명령 반영
  - recover 성공 시 실제 실행이 아니라 queued 복원만 수행한다는 안내 메시지 추가

## 설계 포인트

- recover는 즉시 실행이 아니라 큐잉 시스템이 나중에 처리할 수 있도록 상태만 `queued`로 되돌림
- 권한 모델 없이 열어두는 대신 bulk recover를 막고, cooldown과 audit trail로 오남용 위험을 낮춤
- read-only workflow 조회는 기존 session-scoped visibility를 유지하지만, recover는 workflow id를 아는 경우 직접 요청 가능하게 열어둠

## 검증

- `workflow/runtime.py` 진단 clean
- `uv run python - <<'PY' ... PY` 스모크 테스트 수행
  - `running -> queued` recover 성공
  - `waiting_input -> queued` recover 성공
  - `completed` recover 거부
  - `queued`는 이미 대기열 응답
  - `/workflow recover all` 거부
  - audit metadata 기록 확인
