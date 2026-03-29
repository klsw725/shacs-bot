# PRD: 커스텀 에이전트 아키텍처

> **Spec**: [`docs/specs/custom-agents/spec.md`](../spec.md)
> **선행**: [스킬 격리 실행](../../skill-isolation/prds/skill-isolation.md) — ApprovalGate, 출처 기반 신뢰 모델
> **참고**: [Codex Subagents](https://developers.openai.com/codex/subagents/)

---

## 문제

1. **역할 하드코딩**: `SUBAGENT_ROLES` dict에 3개 역할만 존재. 사용자가 특화 에이전트를 추가할 수 없음.
2. **모델 단일화**: 모든 서브에이전트가 메인 모델을 사용. 읽기 전용 탐색에 고급 모델 낭비.
3. **MCP 격리 부재**: MCP 서버가 메인 에이전트에만 연결. 서브에이전트는 MCP 도구에 접근 불가.
4. **동시성 무제한**: LLM이 spawn을 무제한 호출 가능. API 비용 폭발 리스크.
5. **ApprovalGate 편중**: workspace 스킬만 승인 게이트 통과. workspace 에이전트는 무검사.

## 해결책

TOML 기반 선언적 에이전트 + 에이전트별 모델 라우팅 + 에이전트별 MCP + 동시성 제한 + 출처 기반 ApprovalGate 확장. 자세한 설계는 [spec.md](../spec.md) 참조.

---

## 사용자 영향

| Before | After |
|---|---|
| 3개 역할만 선택 가능 | TOML로 무제한 커스텀 에이전트 정의 |
| 모든 서브에이전트가 동일 모델 | 에이전트별 최적 모델 사용 (동일 프로바이더 내) |
| 서브에이전트는 MCP 도구 접근 불가 | 에이전트별 MCP 서버 연결 가능 |
| spawn 무제한 | maxThreads로 비용 안전장치 |
| workspace 에이전트 무검사 | 출처 기반 ApprovalGate 적용 |

---

## 기술적 범위

### 신규 파일

| 파일 | 역할 | 규모 |
|---|---|---|
| `shacs_bot/agent/agents.py` | AgentDefinition + BUILTIN_AGENTS + AgentRegistry | ~130줄 |

### 수정 파일

| 파일 | 변경 내용 | 규모 |
|---|---|---|
| `shacs_bot/config/schema.py` | 기존 `AgentsConfig`에 `max_threads` 추가 | ~3줄 |
| `shacs_bot/agent/subagent.py` | AgentRegistry 통합. 모델 라우팅. 동시성 제한. 에이전트별 MCP. workspace ApprovalGate. SUBAGENT_ROLES 제거. | ~90줄 |
| `shacs_bot/agent/approval.py` | `skill_name` → `entity_name` + `entity_type` 일반화 | ~10줄 |
| `shacs_bot/agent/tools/spawn.py` | role enum 제거, 동적 역할 | ~15줄 |
| `shacs_bot/agent/loop.py` | AgentRegistry 생성 + 전달 | ~10줄 |
| `shacs_bot/agent/context.py` | AgentRegistry 주입 + 에이전트 목록 프롬프트 | ~15줄 |

### 총 변경량: ~273줄

---

## 성공 기준

1. 기존 하드코딩 역할 (researcher, analyst, executor) — 동일하게 동작 (하위 호환)
2. `~/.shacs-bot/agents/reviewer.toml` 작성 → `spawn(role="reviewer")` 동작
3. TOML에 `model = "claude-haiku-4-5-20251001"` → 해당 모델로 LLM 호출 (동일 프로바이더)
4. 동일 이름 커스텀 에이전트가 built-in override
5. `maxThreads=3` → 4번째 spawn (일반 또는 스킬) 거부 메시지
6. TOML에 `[mcp_servers.x]` → 서브에이전트에서 MCP 도구 사용 가능
7. MCP 연결 실패 → 경고 로그 + MCP 없이 계속 진행
8. MCP 연결은 서브에이전트 종료 시 자동 정리
9. workspace TOML 에이전트 → skill_approval 모드에 따라 ApprovalGate 적용
10. builtin/user 에이전트 → ApprovalGate 미적용
11. 시스템 프롬프트에 `<agents>` 태그로 에이전트 목록 표시
12. 잘못된 TOML → 경고 로그 + 해당 에이전트만 스킵
13. 프로바이더 불일치 모델 → 서브에이전트 실패 보고, 메인 무영향
14. `spawn_skill()` 기존 동작 유지 (entity_name 리네임 외)
15. LSP 진단: 변경 파일 신규 에러 0건

---

## 마일스톤

- [ ] **M1: AgentDefinition + AgentRegistry + 설정**
  `agent/agents.py` 신규 — `AgentDefinition` dataclass, `BUILTIN_AGENTS` (기존 프롬프트 이관), `AgentRegistry` (TOML 로드 + built-in 통합 + 조회 + `build_agents_summary()`). `config/schema.py` — 기존 `AgentsConfig`에 `max_threads: int = 6` 추가. **검증**: TOML 파일 작성 → `AgentRegistry.get()` 반환, built-in override 동작, `max_threads` config 로드.

- [ ] **M2: SubagentManager 통합 — 모델 라우팅 + 동시성**
  `subagent.py` — `AgentRegistry` 주입. `spawn()`에서 `max_threads` 체크 (일반 + 스킬 합산). `_run_subagent()`에서 에이전트 정의의 model/allowed_tools/sandbox_mode 사용. 기존 `SUBAGENT_ROLES` + `SubagentRole` 제거 → `BUILTIN_AGENTS` 대체. `loop.py` — AgentRegistry 생성 + SubagentManager/ContextBuilder에 전달. **검증**: 커스텀 에이전트 모델로 실행, maxThreads 초과 시 거부, 기존 spawn(role="executor") 하위 호환.

- [ ] **M3: ApprovalGate 일반화 + workspace 에이전트 승인**
  `approval.py` — `skill_name` → `entity_name`, `entity_type` 파라미터 추가. 승인 메시지 동적 표시. `subagent.py` — `_run_subagent()`에서 `source == "workspace"` → ApprovalGate 적용. `_run_skill()` — `entity_name`/`entity_type` 사용하도록 호출부 수정. **검증**: workspace 에이전트 → auto/manual 모드에서 승인 동작, builtin/user → 무승인, 기존 스킬 승인 동작 유지.

- [ ] **M4: 에이전트별 MCP 연결**
  `subagent.py` — `_run_subagent()`에서 `agent_def.mcp_servers` 있으면 `AsyncExitStack` + `connect_mcp_servers()` 호출. 서브에이전트 종료 시 자동 정리. 연결 실패 시 경고 + 계속 진행. **검증**: MCP TOML 에이전트 → MCP 도구 사용 가능, 종료 후 프로세스 정리, 연결 실패 시 graceful.

- [ ] **M5: SpawnTool + 시스템 프롬프트 + 통합 검증**
  `spawn.py` — role enum 제거, 동적 역할 지원. description에서 `<agents>` 참조 안내. `context.py` — `AgentRegistry` 주입 + `build_system_prompt()`에 에이전트 목록 추가. 전체 통합 검증. **검증**: LLM이 에이전트 목록을 보고 적절한 에이전트를 spawn, 잘못된 role 지정 시 에러 메시지.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| stdio MCP 연결 오버헤드 (수초) | 확실 | 중간 | 서브에이전트 실행 시간 대비 수용 가능. 로그 모니터링. |
| TOML 파싱 실패 | 낮음 | 낮음 | 개별 실패만 스킵, built-in + 나머지 정상. |
| 프로바이더 불일치 모델 | 중간 | 중간 | 서브에이전트 실패로 처리. 메인 무영향. |
| workspace TOML 악의적 프롬프트 | 중간 | 높음 | sandbox_mode (Layer 1) + ApprovalGate (Layer 2) 2중 방어. |
| maxThreads 너무 낮음 | 중간 | 낮음 | 기본값 6. config에서 조정 가능. |
| 동시 MCP 연결 과다 | 낮음 | 중간 | maxThreads로 간접 제한. |

---

## 종속성

- **선행**: 스킬 격리 (M1~M3 완료) — ApprovalGate, spawn_skill, ALWAYS_ALLOW/ALWAYS_DENY 인프라
- **신규 의존성**: 없음 (`tomllib`은 Python 3.11+ stdlib)
- **영향**: `subagent.py` 대규모 리팩토링 (`SUBAGENT_ROLES` → `AgentRegistry`)

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-29 | Codex subagents 분석. 스펙 초안. |
| 2026-03-29 | 스펙 리뷰 — AgentsConfig 충돌, 프로바이더 제약, max_depth 제거, skill-harness 통합 등 11개 항목 반영. |
| 2026-03-29 | PRD 작성. |
