# Workflow ID Hardening

## 사용자 프롬프트

- "진행시켜"

## 변경 요약

- `shacs_bot/workflow/store.py` 수정
  - `WORKFLOW_ID_HEX_LENGTH = 16` 상수 추가
  - `build_workflow_id()`가 `wf_` + 16 hex를 생성하도록 변경

## 설계 포인트

- 채널/CLI recover가 열려 있는 상태에서 너무 짧은 workflow id는 추측 가능성이 높아짐
- 기존 record 읽기 방식은 그대로 유지하고, 새로 생성되는 workflow id만 더 길게 만들어 호환성을 유지
- 코드베이스 내 고정 길이 가정은 `build_workflow_id()` 외에는 확인되지 않음

## 검증

- `workflow/store.py` 진단 clean
- `uv run python - <<'PY' ... PY` 로 생성된 workflow id가 `wf_` prefix + 16 hex 길이인지 확인
