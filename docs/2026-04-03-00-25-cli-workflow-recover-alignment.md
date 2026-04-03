# CLI Workflow Recover Alignment

## 사용자 프롬프트

- "남은 이슈는 뭐가 있어?"
- "진행시켜"

## 변경 요약

- `shacs_bot/cli/commands.py` 수정
  - `workflows recover`를 bulk recovery에서 단건 recovery 인터페이스로 변경
  - 사용법: `shacs-bot workflows recover <id>`
  - `all` 인자를 명시적으로 거부하도록 처리
  - 결과 상태에 따라 not found / terminal / already queued / cooldown / recovered 메시지 분기
- `shacs_bot/workflow/__init__.py` 수정
  - `ManualRecoverResult` export 추가

## 설계 포인트

- 채널 recover와 CLI recover 정책을 일치시켜 bulk recovery 표면을 제거
- recover는 즉시 실행이 아니라 queued 복원만 수행하며, 실제 실행은 큐 시스템에 맡김
- 기존 `WorkflowRuntime.manual_recover()`를 그대로 재사용해 cooldown/audit 정책을 CLI에도 동일 적용

## 검증

- `workflow/__init__.py` 진단 clean
- `uv run python - <<'PY' ... ast.parse(...)` 로 `commands.py` 문법 확인
- `uv run python - <<'PY' ... PY` 로 `ManualRecoverResult` export 및 manual recover 동작 확인
- `commands.py` 기반 LSP 오류는 기존 환경/타입 문제(`typer` 미설치 등)가 남아 있음
