# operator-console 문서 동기화

**날짜**: 2026-04-04 16:10  
**브랜치**: `main`

---

## 사용자 프롬프트

> 그래 문서 정리해줘

---

## 작업 내용

- `docs/specs/operator-console/prds/operator-console.md`의 milestone/acceptance 체크박스를 실제 구현 상태에 맞게 완료 처리
- 같은 PRD에 진행 로그를 추가해 구현/검증/polish 이력을 문서에 반영
- `docs/specs/operator-console/spec.md`의 현재 상태 분석과 검증 기준을 최신 구현 상태에 맞게 동기화

## 근거로 반영한 구현 상태

- `shacs_bot/cli/commands.py`
  - `inspect sessions`
  - `inspect workflows`
  - `inspect usage`
  - `inspect approvals`
  - `status` personal inspect summary
- `shacs_bot/agent/session/manager.py`
  - `list_sessions()`에서 message count/metadata 포함 요약 제공
- `shacs_bot/agent/approval.py`
  - `list_pending_approvals()`로 process-local approval 조회 제공
- `shacs_bot/workflow/store.py`
  - `list_all()`, `list_incomplete()`로 workflow 조회 가능
- `shacs_bot/agent/usage.py`
  - `get_daily_summary()`, `get_session_summary()`, `get_recent_session()` 제공
- `tests/test_inspect_cli.py`
  - sessions/workflows/usage/approvals/status의 필터/empty-state 시나리오 검증

## 검증

- 문서 변경만 수행했고 런타임 코드는 수정하지 않음
- 변경 후 문서 내용이 실제 구현 파일/테스트와 충돌하지 않도록 교차 확인

## 정리

- `operator-console`은 구현상 완료되었는데 PRD/SPEC 체크박스가 남아 있어 다음 작업 추천이 왜곡될 수 있는 상태였다.
- 이번 동기화로 `D Next`가 stale 문서가 아니라 실제 미구현 작업을 기준으로 다음 우선순위를 잡을 수 있게 됐다.
