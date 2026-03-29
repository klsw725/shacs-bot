# PRD: 에이전트 Git 설치

> **Spec**: [`docs/specs/custom-agents/spec.md`](../spec.md)
> **선행**: [커스텀 에이전트 아키텍처](custom-agents.md) — AgentRegistry, TOML 로드, ApprovalGate
> **참고**: [Codex Skills](https://developers.openai.com/codex/skills/) — 스킬 설치 메커니즘

---

## 문제

커스텀 에이전트와 스킬을 사용하려면 사용자가 TOML 파일과 SKILL.md를 직접 작성하거나, 수동으로 파일을 복사해야 한다. 커뮤니티에서 만든 에이전트/스킬을 재사용할 경로가 없다.

## 해결책

`/agent install <git-url>` 슬래시 명령어로 Git 저장소에서 에이전트 + 스킬 번들을 workspace에 설치한다. workspace 레벨 설치이므로 기존 ApprovalGate가 자동 적용된다.

---

## 사용자 영향

| Before | After |
|---|---|
| TOML/SKILL.md 수동 작성만 가능 | `/agent install <url>` 로 Git에서 설치 |
| 커뮤니티 에이전트 공유 불가 | Git 저장소로 배포/설치 가능 |
| 설치된 에이전트 관리 불가 | `/agent list`, `/agent remove` 로 관리 |

---

## 설치 가능 저장소 구조

### 단일 에이전트

```
my-agent/
├── agent.toml              # 필수: 에이전트 정의
├── skills/                 # 선택: 번들 스킬
│   └── my-skill/
│       ├── SKILL.md
│       └── scripts/
└── README.md               # 무시됨
```

### 에이전트 컬렉션

```
agent-collection/
├── agents/                 # 여러 에이전트
│   ├── reviewer.toml
│   └── researcher.toml
├── skills/                 # 공유 스킬
│   ├── code-review/
│   │   └── SKILL.md
│   └── web-research/
│       └── SKILL.md
└── README.md               # 무시됨
```

### 감지 로직

```
git clone → 임시 디렉토리
  → agents/*.toml 있으면 → 컬렉션 모드
  → agent.toml (루트) 있으면 → 단일 모드
  → 둘 다 없으면 → 에러
```

---

## 슬래시 명령어

### `/agent install <git-url>`

```
/agent install https://github.com/user/my-agent.git
```

동작:
1. `git clone --depth 1` → 임시 디렉토리
2. 저장소 구조 감지 (단일/컬렉션)
3. TOML 파일 검증 (필수 필드 확인)
4. `{workspace}/agents/` 에 TOML 복사
5. `{workspace}/skills/` 에 스킬 복사 (있는 경우)
6. 설치 매니페스트 기록 (`{workspace}/agents/.installed.json`)
7. `AgentRegistry.reload()` 호출
8. 결과 메시지 출력

출력 예시:
```
✅ 에이전트 설치 완료
  에이전트: reviewer (PR 리뷰에 집중하는 읽기 전용 에이전트)
  스킬: code-review
  출처: https://github.com/user/my-agent.git
  위치: workspace (ApprovalGate 적용)
```

### `/agent list`

설치된 에이전트 목록 표시 (built-in + user + workspace 구분).

```
🤖 에이전트 목록

Built-in:
  • researcher — 웹 검색/정보 수집
  • analyst — 분석/요약
  • executor — 파일 작업/명령 실행

Workspace (ApprovalGate 적용):
  • reviewer — PR 리뷰 (github.com/user/my-agent)
  • data-analyst — 데이터 분석 (github.com/org/agents)
```

### `/agent remove <name>`

workspace에서 에이전트 (+ 연관 스킬)를 삭제.

```
/agent remove reviewer
```

```
🗑 에이전트 'reviewer' 삭제됨
  연관 스킬도 삭제: code-review
```

### `/agent update [name]`

설치된 에이전트를 최신 버전으로 업데이트. name 생략 시 전체 업데이트.

```
/agent update reviewer
```

---

## 설치 매니페스트

`{workspace}/agents/.installed.json` — 설치된 에이전트의 출처 추적.

```json
{
  "installed": [
    {
      "name": "reviewer",
      "git_url": "https://github.com/user/my-agent.git",
      "installed_at": "2026-03-29T15:30:00",
      "commit": "abc1234",
      "skills": ["code-review"]
    }
  ]
}
```

이 파일로:
- `/agent list`에서 출처 URL 표시
- `/agent remove`에서 연관 스킬 함께 삭제
- `/agent update`에서 원본 URL로 재설치

---

## 보안 모델

설치 위치가 `{workspace}/agents/`이므로:

1. **ApprovalGate 자동 적용**: workspace 에이전트 → `skill_approval` 모드에 따라 도구 호출 검사
2. **sandbox_mode 적용**: TOML에 정의된 대로 도구 필터링
3. **스킬도 workspace에 설치**: workspace 스킬 → 동일한 ApprovalGate 적용
4. **git clone은 임시 디렉토리에서**: 검증 후 workspace로 복사, 임시 디렉토리 삭제

**위협 시나리오**: 악의적 TOML에 `sandbox_mode = "full"` + `developer_instructions`에 프롬프트 인젝션 → **ApprovalGate Layer 2**가 도구 호출 시점에 차단. 스킬의 악의적 SKILL.md → 동일.

---

## 기술적 범위

### 신규 파일

| 파일 | 역할 | 규모 |
|---|---|---|
| `shacs_bot/agent/agent_installer.py` | Git clone + 구조 감지 + 설치 + 매니페스트 관리 | ~150줄 |

### 수정 파일

| 파일 | 변경 내용 | 규모 |
|---|---|---|
| `shacs_bot/agent/loop.py` | `/agent install\|list\|remove\|update` 슬래시 명령어 핸들러 | ~50줄 |
| `shacs_bot/agent/agents.py` | `AgentRegistry.reload()` 구현 (이미 인터페이스 존재) | ~3줄 |

### 총 변경량: ~200줄

---

## 성공 기준

1. `/agent install https://github.com/...` → TOML + 스킬이 workspace에 설치
2. 설치 후 `spawn(role="<설치된 에이전트>")` 동작
3. 설치된 에이전트에 ApprovalGate 적용 (workspace source)
4. `/agent list` → built-in/workspace 구분 표시
5. `/agent remove <name>` → 에이전트 + 연관 스킬 삭제
6. `/agent update <name>` → 최신 커밋으로 재설치
7. 잘못된 URL → 에러 메시지 (크래시 없음)
8. 필수 필드 누락 TOML → 설치 거부 + 에러 메시지
9. 매니페스트(`.installed.json`)에 출처 기록
10. `git` 명령어 없으면 → 명확한 에러 메시지

---

## 마일스톤

- [ ] **M1: AgentInstaller 코어 — clone + 감지 + 설치**
  `agent_installer.py` — `git clone --depth 1`, 저장소 구조 감지 (단일/컬렉션), TOML 검증, workspace에 복사, 매니페스트 기록. **검증**: 단일 에이전트 repo clone → workspace에 TOML + 스킬 설치.

- [ ] **M2: 슬래시 명령어 — install + list + remove**
  `loop.py` — `/agent install`, `/agent list`, `/agent remove` 핸들러. 설치 후 `AgentRegistry.reload()`. **검증**: 슬래시 명령어로 설치/조회/삭제 동작.

- [ ] **M3: update + 에러 처리 + 통합 검증**
  `/agent update` 구현. git 미설치, 네트워크 오류, 잘못된 TOML 등 에러 처리. **검증**: 전체 흐름 (install → list → spawn → remove) 동작.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 악의적 TOML/스킬 설치 | 중간 | 높음 | workspace 레벨 설치 → ApprovalGate 자동 적용 |
| git clone 시간 (대형 repo) | 낮음 | 낮음 | `--depth 1` shallow clone |
| git 미설치 환경 | 중간 | 낮음 | 명확한 에러 메시지 |
| 이름 충돌 (기존 에이전트와 동일) | 중간 | 중간 | 설치 시 경고 + 덮어쓰기 확인 |
| 네트워크 없는 환경 | 낮음 | 낮음 | 에러 메시지 |

---

## 종속성

- **선행**: 커스텀 에이전트 아키텍처 (M1~M5 완료) — AgentRegistry, TOML 로드
- **신규 의존성**: 없음 (`git` 은 시스템 의존성, `subprocess`로 호출)
- **외부 의존성**: `git` CLI

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-29 | PRD 작성 |
