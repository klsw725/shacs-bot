# MCP enabledTools 필터링 구현

> **Prompt**: `D Next` → 기존 3개 PRD 완료 후 다음 작업 추천 → MCP enabledTools 필터링 → PRD 작성 → 구현

## 변경 파일

| 파일 | 변경 |
|---|---|
| `shacs_bot/config/schema.py` | `MCPServerConfig`에 `enabled_tools: list[str]` 필드 추가 (JSON: `enabledTools`, 기본값: 빈 리스트) |
| `shacs_bot/agent/tools/mcp.py` | `connect_mcp_servers()`에 enabledTools 필터링 로직 추가: enabled set 기반 필터링, 필터링 결과 로그 (등록/전체), 존재하지 않는 도구명 WARNING |
| `docs/prds/mcp-enabled-tools.md` | PRD 작성 + M1/M2 완료 표시 |

## 핵심 변경 내용

### Before
- MCP 서버 연결 시 서버가 제공하는 모든 도구가 무조건 등록됨
- 도구 필터링 방법 없음

### After
- `enabledTools: ["tool_a", "tool_b"]` 지정 시 해당 도구만 등록
- 미지정 시 기존과 동일하게 전체 등록 (하위 호환)
- 존재하지 않는 도구명 지정 시 WARNING 로그 (오타 방지)
- 로그에 등록 비율 표시 (`3/40 도구`)

## 검증 결과 (코드 리뷰 정적 분석)

| 검증 포인트 | 결과 |
|---|---|
| enabledTools 지정 → 해당 도구만 등록 | ✅ |
| enabledTools 미지정 → 전체 등록 (하위 호환) | ✅ |
| 제외 도구 LLM 미노출 | ✅ |
| 필터링 결과 로그 출력 | ✅ |
| 존재하지 않는 도구명 WARNING | ✅ |
| JSON camelCase 매핑 (enabledTools ↔ enabled_tools) | ✅ |
