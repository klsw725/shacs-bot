# Subagent Redispatch

## 사용자 프롬프트

- "진행시켜"

## 변경 요약

- `shacs_bot/agent/subagent.py` 수정
  - `spawn()` / `spawn_skill()`에 `workflow_id` 재사용 경로 추가
  - subagent workflow metadata에 replay용 정보 추가
    - `originChannel`, `originChatId`, `originMetadata`, `sessionKey`
    - skill workflow의 경우 `skillName`, `skillPath`
  - `execute_existing_workflow()` 추가
    - persisted workflow record를 읽어 일반 subagent 또는 skill subagent로 재실행
- `shacs_bot/workflow/redispatcher.py` 수정
  - queued workflow 소비 시 `source_kind == "subagent"` 지원 추가
- `shacs_bot/agent/loop.py` 수정
  - `subagent_manager` property 추가
- `shacs_bot/cli/commands.py` 수정
  - gateway에서 redispatcher 생성 시 `subagent_manager` 주입

## 설계 포인트

- 기존 workflow id를 재사용해 replay 시 새 workflow record를 만들지 않음
- subagent는 queued record를 다시 `spawn()`/`spawn_skill()` 하는 방식으로 재디스패치
- skill replay를 위해 skill 이름/경로를 metadata에 남기지만, 이는 새로 생성되는 workflow부터 적용됨
- cron-only redispatch 구조를 확장하되, heartbeat/manual은 여전히 보류

## 검증

- `workflow/redispatcher.py` 진단 clean
- `subagent.py`는 기존 warning이 남아 있으나 새 런타임 오류는 확인되지 않음
- `uv run python - <<'PY' ... PY` 스모크 테스트 수행
  - queued subagent workflow record 준비
  - redispatcher tick 실행
  - 기존 workflow record가 `completed` 로 전이되는지 확인
  - `resultPreview` 갱신 확인
