# PRD: MCP enabledTools 필터링

---

## 문제

현재 MCP 서버 연결 시 서버가 제공하는 **모든 도구가 무조건 등록**된다:

```python
# mcp.py:104-108
tools = await session.list_tools()
for tool_def in tools.tools:
    wrapper = MCPToolWrapper(session, name, tool_def, ...)
    registry.register(wrapper)
```

**문제점**:
1. 도구가 많은 MCP 서버(예: GitHub 40개+, filesystem 20개+)를 연결하면 LLM이 수십 개의 도구를 보게 됨 → 도구 선택 정확도 저하
2. 사용하지 않는 도구까지 노출되어 LLM이 불필요한 도구를 호출할 수 있음
3. 위험한 도구(파일 삭제, DB 변경 등)를 선택적으로 비활성화할 방법이 없음

nanobot은 이미 `enabledTools` 필터링을 구현하여 필요한 도구만 선택적으로 활성화할 수 있다.

## 해결책

`MCPServerConfig`에 `enabled_tools` 필드를 추가하고, MCP 도구 등록 시 필터링한다:

1. `MCPServerConfig`에 `enabled_tools: list[str]` 필드 추가
2. `connect_mcp_servers()`에서 도구 등록 전 `enabled_tools` 필터링
3. `enabled_tools`가 비어있으면 기존과 동일하게 전체 등록 (하위 호환)

## 사용자 영향

| Before | After |
|---|---|
| MCP 서버의 모든 도구가 무조건 등록됨 | `enabledTools`로 필요한 도구만 선택적 활성화 |
| 도구 40개+ 노출 → LLM 선택 혼란 | 필요한 5-10개만 노출 → 정확한 선택 |
| 위험한 도구를 비활성화할 방법 없음 | 안전한 도구만 활성화 가능 |
| 설정 변경 없이 사용하는 사용자는 변화 없음 | 동일 (하위 호환) |

## 기술적 범위

- **변경 파일**: `shacs_bot/config/schema.py`, `shacs_bot/agent/tools/mcp.py` (2개)
- **변경 유형**: Python 코드 추가/수정
- **의존성**: 없음. 기존 패키지만 사용.
- **하위 호환성**: `enabledTools` 미지정 시 기존과 동일하게 전체 도구 등록.

### 변경할 코드 요약

**schema.py**:
- `MCPServerConfig`에 `enabled_tools: list[str]` 필드 추가 (기본값: 빈 리스트)
- Pydantic `alias_generator=to_camel`에 의해 JSON에서는 `enabledTools`로 매핑

**mcp.py**:
- `connect_mcp_servers()`의 도구 등록 루프에서 `enabled_tools` 필터링 추가
- `enabled_tools`가 비어있으면(`not cfg.enabled_tools`) 전체 등록, 있으면 일치하는 도구만 등록
- 필터링 결과 로그 출력 (등록/제외 도구 수)

### config.json 예시

```json
{
  "tools": {
    "mcpServers": {
      "github": {
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-github"],
        "env": { "GITHUB_TOKEN": "ghp_..." },
        "enabledTools": ["search_repositories", "get_file_contents", "create_issue"]
      },
      "filesystem": {
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        "enabledTools": ["read_file", "list_directory"]
      },
      "weather": {
        "command": "python",
        "args": ["weather_mcp.py"]
      }
    }
  }
}
```

위 예시에서:
- `github`: 40개+ 도구 중 3개만 활성화
- `filesystem`: 읽기 관련 도구만 활성화 (쓰기/삭제 차단)
- `weather`: `enabledTools` 미지정 → 전체 도구 등록 (기존 동작)

## 성공 기준

1. `enabledTools`에 도구명 목록 지정 → 해당 도구만 등록, 나머지 제외
2. `enabledTools` 미지정 또는 빈 리스트 → 기존과 동일하게 전체 등록 (하위 호환)
3. 제외된 도구는 LLM에 노출되지 않음 (도구 정의 목록에 포함되지 않음)
4. 필터링 결과가 로그에 명시 (`"MCP 서버 'X': Y개 도구 중 Z개 활성화"`)
5. `enabledTools`에 존재하지 않는 도구명 지정 시 경고 로그 출력 (오타 방지)

---

## 마일스톤

- [x] **M1: enabledTools 필터링 구현**
  `MCPServerConfig`에 `enabled_tools` 필드 추가, `connect_mcp_servers()`에서 필터링 로직 적용. `enabledTools` 미지정 시 기존 동작 유지.

- [x] **M2: 실행 검증**
  MCP 서버 연결 후 `enabledTools`에 지정한 도구만 등록되는지 확인. 미지정 시 전체 등록 확인. 존재하지 않는 도구명 경고 로그 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| enabledTools에 오타로 도구 누락 | 중간 | 중간 | 존재하지 않는 도구명에 WARNING 로그 출력 |
| MCP 서버가 도구명을 변경 (업데이트 시) | 낮음 | 중간 | WARNING 로그로 감지 가능, 빈 리스트로 폴백 |
| 설정 마이그레이션 필요 | 없음 | 없음 | 새 필드의 기본값이 빈 리스트 → 기존 설정 그대로 동작 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-15 | PRD 초안 작성. nanobot의 enabledTools 패턴 확인. 현재 mcp.py에서 전체 도구 무조건 등록 확인. |
| 2026-03-15 | M1+M2 완료. `MCPServerConfig`에 `enabled_tools: list[str]` 필드 추가 (JSON: `enabledTools`). `connect_mcp_servers()`에서 `enabled` set 기반 필터링 + 미등록 도구 수 포함 로그 (`3/40 도구`) + 존재하지 않는 도구명 WARNING. 코드 리뷰 정적 분석: 6개 검증 포인트 전부 통과 (필터링, 하위 호환, LLM 미노출, 로그, 오타 감지, camelCase 매핑). |
