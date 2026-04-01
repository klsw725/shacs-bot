#!/usr/bin/env python3
"""
Agent Initializer - Git 배포 가능한 에이전트 패키지를 생성한다.

Usage:
    init_agent.py <agent-name> --path <path> [--with-skills <skill1,skill2>]

Examples:
    init_agent.py reviewer --path ./my-agents
    init_agent.py data-analyst --path ./my-agents --with-skills data-query,chart-gen

생성 구조 (단일 에이전트):
    <agent-name>/
    ├── agent.toml              # 에이전트 정의
    ├── skills/                 # 번들 스킬 (--with-skills 시)
    │   └── <skill-name>/
    │       └── SKILL.md
    └── README.md               # 설치 가이드

생성 구조 (컬렉션 — 향후):
    <collection-name>/
    ├── agents/
    │   ├── agent1.toml
    │   └── agent2.toml
    ├── skills/
    └── README.md
"""

import argparse
import re
import sys
from pathlib import Path

MAX_NAME_LENGTH = 64

AGENT_TOML_TEMPLATE = '''# {agent_title}
# /agent install <git-url> 로 설치 가능

name = "{agent_name}"
description = "{agent_description}"

developer_instructions = """
[TODO: 에이전트의 핵심 지시사항을 작성하세요]

## 임무
[TODO: 에이전트가 수행하는 작업]

## 행동 규칙
[TODO: 에이전트가 따라야 할 규칙]

## 결과 보고
[TODO: 결과 형식]

## 제약
[TODO: 하지 말아야 할 것]
"""

# 선택 필드 — 필요한 것만 주석 해제
# model = "claude-haiku-4-5-20251001"       # 동일 프로바이더 내 모델만
# sandbox_mode = "read-only"                 # "read-only" | "workspace-write" | "full"
# max_iterations = 15
# allowed_tools = ["read_file", "list_dir", "exec", "web_search"]

# 에이전트 전용 MCP 서버 (선택)
# [mcp_servers.example]
# url = "https://example.com/mcp"
# tool_timeout = 30
# enabled_tools = ["search", "get"]
'''

SKILL_TEMPLATE = '''---
name: {skill_name}
description: "[TODO: 스킬 설명 — 언제 이 스킬을 사용하는지 명확히]"
---

# {skill_title}

[TODO: 스킬 지시사항 작성]
'''

README_TEMPLATE = '''# {agent_title}

> {agent_description}

## 설치

```bash
/agent install <이 저장소의 git URL>
```

## 에이전트

| 이름 | 설명 |
|---|---|
| `{agent_name}` | {agent_description} |

{skills_section}

## 구조

```
{structure}
```

## 보안

workspace 레벨로 설치되므로 ApprovalGate가 자동 적용됩니다.
- `sandbox_mode`로 도구 접근 제한 (Layer 1)
- 도구 호출 시점에 승인 검사 (Layer 2)
'''


def normalize_name(name: str) -> str:
    normalized = name.strip().lower()
    normalized = re.sub(r"[^a-z0-9]+", "-", normalized)
    normalized = normalized.strip("-")
    normalized = re.sub(r"-{2,}", "-", normalized)
    return normalized


def title_case(name: str) -> str:
    return " ".join(word.capitalize() for word in name.split("-"))


def init_agent(
    agent_name: str,
    path: str,
    skills: list[str] | None = None,
    description: str = "[TODO: 에이전트 설명]",
) -> Path | None:
    agent_dir = Path(path).resolve() / agent_name

    if agent_dir.exists():
        print(f"[ERROR] 디렉토리가 이미 존재합니다: {agent_dir}")
        return None

    try:
        agent_dir.mkdir(parents=True)
        print(f"[OK] 에이전트 디렉토리 생성: {agent_dir}")
    except Exception as e:
        print(f"[ERROR] 디렉토리 생성 실패: {e}")
        return None

    agent_title = title_case(agent_name)

    # agent.toml
    toml_content = AGENT_TOML_TEMPLATE.format(
        agent_name=agent_name,
        agent_title=agent_title,
        agent_description=description,
    )
    (agent_dir / "agent.toml").write_text(toml_content)
    print("[OK] agent.toml 생성")

    # skills/
    skill_names: list[str] = []
    if skills:
        skills_dir = agent_dir / "skills"
        skills_dir.mkdir()
        for skill_name in skills:
            skill_name = normalize_name(skill_name)
            skill_dir = skills_dir / skill_name
            skill_dir.mkdir()
            skill_title = title_case(skill_name)
            (skill_dir / "SKILL.md").write_text(
                SKILL_TEMPLATE.format(skill_name=skill_name, skill_title=skill_title)
            )
            skill_names.append(skill_name)
            print(f"[OK] 스킬 생성: skills/{skill_name}/SKILL.md")

    # README.md
    skills_section = ""
    if skill_names:
        skills_section = "## 스킬\n\n| 이름 | 설명 |\n|---|---|\n"
        for s in skill_names:
            skills_section += f"| `{s}` | [TODO: 설명] |\n"

    structure_lines = [f"{agent_name}/", "├── agent.toml"]
    if skill_names:
        structure_lines.append("├── skills/")
        for i, s in enumerate(skill_names):
            prefix = "│   └──" if i == len(skill_names) - 1 else "│   ├──"
            structure_lines.append(f"{prefix} {s}/")
            structure_lines.append(f"│       └── SKILL.md")
    structure_lines.append("└── README.md")

    readme = README_TEMPLATE.format(
        agent_name=agent_name,
        agent_title=agent_title,
        agent_description=description,
        skills_section=skills_section,
        structure="\n".join(structure_lines),
    )
    (agent_dir / "README.md").write_text(readme)
    print("[OK] README.md 생성")

    print(f"\n[OK] 에이전트 '{agent_name}' 초기화 완료: {agent_dir}")
    print("\n다음 단계:")
    print("1. agent.toml의 TODO 항목 작성 (developer_instructions)")
    if skill_names:
        print("2. skills/*/SKILL.md의 TODO 항목 작성")
        print("3. 필요 시 스킬에 scripts/, references/, assets/ 추가")
    print(f"{'3' if skill_names else '2'}. Git 저장소로 배포:")
    print(f"   cd {agent_dir}")
    print("   git init && git add -A && git commit -m 'init agent'")
    print("   git remote add origin <your-repo-url> && git push")
    print(f"\n설치: /agent install <git-url>")

    return agent_dir


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Git 배포 가능한 에이전트 패키지를 생성합니다.",
    )
    parser.add_argument("agent_name", help="에이전트 이름 (하이픈 케이스로 정규화)")
    parser.add_argument("--path", required=True, help="생성 경로")
    parser.add_argument(
        "--with-skills",
        default="",
        help="번들 스킬 이름 (콤마 구분)",
    )
    parser.add_argument(
        "--description",
        default="[TODO: 에이전트 설명]",
        help="에이전트 설명",
    )
    args = parser.parse_args()

    agent_name = normalize_name(args.agent_name)
    if not agent_name:
        print("[ERROR] 에이전트 이름에 문자 또는 숫자가 포함되어야 합니다.")
        sys.exit(1)
    if len(agent_name) > MAX_NAME_LENGTH:
        print(f"[ERROR] 이름이 너무 깁니다 ({len(agent_name)}자). 최대 {MAX_NAME_LENGTH}자.")
        sys.exit(1)
    if agent_name != args.agent_name:
        print(f"참고: 이름 정규화 '{args.agent_name}' → '{agent_name}'")

    skills = [s.strip() for s in args.with_skills.split(",") if s.strip()] if args.with_skills else None

    print(f"에이전트 초기화: {agent_name}")
    print(f"  경로: {args.path}")
    if skills:
        print(f"  스킬: {', '.join(skills)}")
    print()

    result = init_agent(agent_name, args.path, skills, args.description)
    sys.exit(0 if result else 1)


if __name__ == "__main__":
    main()
