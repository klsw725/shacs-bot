# Channel Workflow Commands

## 사용자 프롬프트

- "진행시켜"
- "관리자라는게 존재하지 않아 그걸 고려해서 진행해"

## 변경 요약

- `shacs_bot/agent/loop.py` 수정
  - 채널 슬래시 명령에 `/workflows`, `/workflows all`, `/workflow <id>` 추가
  - `notify_target.session_key` 또는 현재 채널/채팅 대상과 일치하는 workflow만 보이도록 session-scoped visibility 적용
  - `/help` 출력에 workflow 명령 추가

## 설계 포인트

- 관리자 개념이 없으므로 read-only만 채널에 노출
- `recover` 같은 상태 변경 명령은 여전히 CLI 전용으로 유지
- 다른 채널/세션의 workflow가 보이지 않도록 현재 세션에 연결된 항목만 조회 가능하게 제한
- 기본 `/workflows`는 incomplete만 표시하고, `/workflows all`에서 완료 항목까지 표시

## 검증

- `uv run python - <<'PY' ... PY` 스모크 테스트 수행
  - 현재 세션의 incomplete workflow만 `/workflows`에 표시
  - `/workflows all`에서 현재 세션의 완료 항목 표시
  - `/workflow <id>` 상세 조회 동작
  - 다른 세션 workflow는 숨김 처리 확인
- `agent/loop.py`의 basedpyright 오류는 기존 항목이 남아 있음
  - import cycle, generic type argument 등
  - 이번 workflow 채널 명령 추가로 새 런타임 오류는 확인되지 않음
