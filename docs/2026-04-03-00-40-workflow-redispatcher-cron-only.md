# Workflow Redispatcher (Cron Only)

## 사용자 프롬프트

- "고고"

## 변경 요약

- `shacs_bot/workflow/redispatcher.py` 추가
  - `WorkflowRedispatcher` 구현
  - persisted workflow 중 `state == "queued"` 인 항목을 순차 소비
  - 현재는 `source_kind == "cron"` 만 재디스패치 대상으로 지원
- `shacs_bot/agent/tools/cron/service.py` 수정
  - `_execute_job(job, workflow_id=None)`로 확장
  - 기존 workflow id를 재사용해 실행할 수 있도록 `execute_existing_workflow()` 추가
  - redispatch 시 새 workflow record를 만들지 않고 기존 record를 이어서 사용
- `shacs_bot/cli/commands.py` 수정
  - gateway 조립 시 `WorkflowRedispatcher` 생성
  - `cron.start()` 후 redispatcher 시작
  - shutdown 시 redispatcher 정지
- `shacs_bot/workflow/__init__.py` 수정
  - redispatcher 최상위 export는 순환 import 방지를 위해 제거

## 설계 포인트

- Oracle 권고에 따라 현재 단계에서는 cron queued workflow만 재실행 대상으로 제한
- heartbeat/subagent/manual은 재실행 payload가 충분히 persisted 되지 않아 queued-only로 유지
- redispatch는 gateway-local polling task 하나로 처리하고, 별도 worker/framework는 추가하지 않음
- cron redispatch는 기존 `on_cron_job` 경로를 재사용해 notify/result 업데이트가 같은 workflow record에 붙도록 유지

## 검증

- `workflow/redispatcher.py` 진단 clean
- `workflow/__init__.py` 진단 clean
- `uv run python - <<'PY' ... PY` 스모크 테스트 수행
  - queued cron workflow 생성
  - redispatcher tick 실행
  - 기존 workflow record가 `completed` 로 전이되는지 확인
  - `resultPreview` 기록 및 `workflowId` 재사용 확인
- `cron/service.py`의 기반 타입 오류는 기존 항목이 남아 있음
