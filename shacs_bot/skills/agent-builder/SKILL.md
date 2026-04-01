---
name: agent-builder
description: "커스텀 에이전트와 스킬을 생성, 구조화, 패키징합니다. 에이전트 TOML 정의 작성, 스킬 번들링, Git 배포 패키지 생성에 사용합니다. 에이전트 또는 스킬을 만들거나, 기존 것을 업데이트하거나, 배포 준비할 때 이 스킬을 사용하세요."
---

# Agent Builder

커스텀 에이전트와 스킬을 생성하고 패키징하는 가이드.

## 무엇을 만들 수 있나

### 에이전트
TOML 파일로 정의되는 서브에이전트. 모델, 도구 권한, MCP 서버를 선언적으로 구성.
`/agent install <git-url>` 로 설치 가능한 패키지로 배포.

### 스킬
SKILL.md 파일로 정의되는 작업 가이드. 스크립트, 레퍼런스, 에셋을 번들링.
에이전트와 함께 또는 독립적으로 배포 가능.

---

## 에이전트 생성

### 빠른 시작: init_agent.py

```bash
# 기본 에이전트
scripts/init_agent.py reviewer --path ./my-agents

# 스킬 번들 포함
scripts/init_agent.py data-analyst --path ./my-agents --with-skills data-query,chart-gen

# 설명 포함
scripts/init_agent.py reviewer --path ./my-agents --description "PR 리뷰 전문 에이전트"
```

생성 구조:
```
reviewer/
├── agent.toml        # 에이전트 정의
├── skills/           # 번들 스킬 (선택)
│   └── code-review/
│       └── SKILL.md
└── README.md         # 설치 가이드
```

### agent.toml 스키마

필수 필드:
```toml
name = "reviewer"
description = "PR 리뷰에 집중하는 읽기 전용 에이전트"
developer_instructions = """
에이전트의 핵심 행동 지시사항.
임무, 규칙, 제약을 명확히.
"""
```

선택 필드:
```toml
model = "claude-haiku-4-5-20251001"     # 동일 프로바이더 내 모델만
sandbox_mode = "read-only"               # "read-only" | "workspace-write" | "full"
max_iterations = 10                      # 최대 LLM 반복
allowed_tools = ["read_file", "list_dir", "exec"]  # 명시적 도구 허용

# 에이전트 전용 MCP 서버
[mcp_servers.docs]
url = "https://docs.example.com/mcp"
tool_timeout = 30
enabled_tools = ["search_docs", "get_page"]
```

### sandbox_mode 가이드

| 모드 | 용도 | 도구 |
|---|---|---|
| `read-only` | 조사, 분석, 리뷰 | read_file, list_dir, exec, web_search, web_fetch, search_history |
| `workspace-write` | 파일 생성/수정 (workspace 내) | 위 + write_file, edit_file |
| `full` | 전체 작업 | 모든 도구 |

`allowed_tools`가 지정되면 `sandbox_mode`보다 우선.

### developer_instructions 작성 가이드

효과적인 지시사항의 구조:

```
## 임무
에이전트가 무엇을 하는지 1-2문장.

## 행동 규칙
- 구체적 행동 지침 (예: "파일 수정 전 반드시 읽기")
- 품질 기준 (예: "출처 명시", "사실과 의견 구분")

## 결과 보고
- 출력 형식 (예: "요약 → 상세 → 결론")
- 포함해야 할 것 (예: "출처 URL", "신뢰도")

## 제약
- 하지 말아야 할 것 (예: "읽기 전용", "범위 밖 작업 금지")
```

### 보안 고려사항

`/agent install`로 설치된 에이전트는 workspace 레벨 → **자동으로 2중 보안 적용**:
- **Layer 1**: `sandbox_mode`로 도구 등록 자체를 제한
- **Layer 2**: `ApprovalGate`로 도구 호출 시점에 승인 검사

**권장**: 최소 권한 원칙. 읽기만 필요하면 `sandbox_mode = "read-only"`.

---

## 스킬 생성

### 빠른 시작: init_skill.py

```bash
scripts/init_skill.py my-skill --path ./workspace/skills
scripts/init_skill.py my-skill --path ./workspace/skills --resources scripts,references
```

### SKILL.md 구조

```markdown
---
name: skill-name
description: "스킬 설명 — 언제 사용하는지 명확히"
---

# Skill Name

## 지시사항
(에이전트가 따를 작업 가이드)
```

### 스킬 핵심 원칙

1. **간결**: 컨텍스트 윈도우는 공공재. 에이전트가 이미 아는 것은 생략.
2. **점진적 공개**: 메타데이터(항상) → SKILL.md(트리거 시) → references(필요 시)
3. **자유도 매칭**: 취약한 작업 = 구체적 스크립트, 유연한 작업 = 텍스트 가이드

### 리소스 디렉토리

| 디렉토리 | 용도 | 예시 |
|---|---|---|
| `scripts/` | 실행 가능 코드 | `rotate_pdf.py`, `extract_data.py` |
| `references/` | 컨텍스트에 로드할 문서 | `api_docs.md`, `schema.md` |
| `assets/` | 출력에 사용할 파일 | `template.pptx`, `logo.png` |

---

## Git 배포

### 에이전트 + 스킬 패키지

```bash
cd my-agent/
git init && git add -A && git commit -m "init agent"
git remote add origin <url> && git push
```

설치: `/agent install <git-url>`

### 저장소 구조 규칙

**단일 에이전트**:
```
my-agent/
├── agent.toml          # 루트에 agent.toml
├── skills/             # 번들 스킬
└── README.md
```

**에이전트 컬렉션**:
```
my-agents/
├── agents/             # agents/ 디렉토리에 여러 TOML
│   ├── reviewer.toml
│   └── researcher.toml
├── skills/             # 공유 스킬
└── README.md
```

설치 시스템이 루트 `agent.toml` 또는 `agents/*.toml`을 자동 감지.

---

## 관리 명령어

```
/agent install <git-url>   # Git에서 설치
/agent list                # 에이전트 목록
/agent remove <name>       # 삭제 (연관 스킬 포함)
/agent update [name]       # 최신 버전으로 업데이트
```

---

## 스크립트 목록

| 스크립트 | 용도 |
|---|---|
| `scripts/init_agent.py` | 에이전트 패키지 생성 (TOML + 스킬 + README) |
| `scripts/init_skill.py` | 스킬 디렉토리 생성 |
| `scripts/package_skill.py` | 스킬을 .skill 파일로 패키징 |
| `scripts/quick_validate.py` | 스킬 구조 검증 |
