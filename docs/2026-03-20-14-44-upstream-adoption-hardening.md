# Upstream Adoption Hardening (M1-M6)

> 프롬프트: `D Next - 다음 작업 진행` → PRD 19 (upstream-adoption-hardening) 선택 → M1-M6 전체 구현

## 변경 파일

| 파일 | 마일스톤 | 변경 |
|---|---|---|
| `shacs_bot/providers/litellm.py` | M1, M5 | `_parse_response()` empty choices guard 추가. `chat()` sanitize 체인에 `_strip_content_meta` 적용 |
| `shacs_bot/config/schema.py` | M1 | `_match_provider()` 키워드 매칭 시 API 키 없으면 warning 로그 + 설정 경로 안내 |
| `shacs_bot/cli/commands.py` | M1 | `_make_provider()` 에러 메시지에 provider명, apiKey 설정 경로, env 변수명 포함 |
| `shacs_bot/agent/session/manager.py` | M2 | `save()` temp file + `replace()` atomic write. `_load()` 개별 레코드 JSONDecodeError 복구 + `.corrupt` 백업 |
| `shacs_bot/agent/subagent.py` | M3 | `_extract_partial_progress()` 정적 메서드 (timeout 시 도구 목록 + 마지막 텍스트 추출). `shutdown()` 메서드 (종료 시 orphan task 경고) |
| `shacs_bot/agent/execution_health.py` | M4 | 신규. `ExecutionHealthMonitor` — tool repeat, error cascade, file burst 3개 detector (deque 윈도우, warn-only) |
| `shacs_bot/agent/loop.py` | M4, M5 | `_run_agent_loop`에 `ExecutionHealthMonitor` 통합. `_save_turn` image placeholder에 source_path 포함 |
| `shacs_bot/agent/context.py` | M5 | `_build_user_content()` 이미지 block에 `_meta.source_path` 추가 |
| `shacs_bot/providers/base.py` | M5 | `_strip_content_meta()` 정적 메서드 — content list 내 `_meta` 키 제거 |
| `shacs_bot/providers/custom.py` | M5 | `chat()` sanitize 체인에 `_strip_content_meta` 적용 |

## 마일스톤별 요약

### M1: Provider Hardening
- `_parse_response()`: `response.choices` 빈 배열 시 `IndexError` 대신 `LLMResponse(error)` 반환
- Lazy loading: 검증 결과 이미 함수 내부 import — 추가 작업 불필요
- Explicit prefix: `find_by_model()`, `_match_provider()` 모두 prefix 우선 매칭 이미 구현
- API key UX: 매칭 실패 시 provider명 + 설정 경로 + env 변수명 안내

### M2: Session Durability
- `save()`: temp file(`.jsonl.tmp`) → `f.flush()` → `replace()` atomic rename
- `_load()`: 개별 JSONL 레코드별 `JSONDecodeError` catch → skip + `.corrupt` 백업

### M3: Subagent Resilience
- `_extract_partial_progress()`: max_iterations 도달 시 사용된 도구 + 마지막 assistant text 추출
- `shutdown()`: 종료 시 실행 중인 서브에이전트 warning 로그 + cancel

### M4: Execution Health Monitor
- 3개 detector: tool repeat(3회), error cascade(3회), file burst(10회)
- `deque(maxlen=15)` 슬라이딩 윈도우, args MD5 해시 1024자 제한
- warn-only — 정상 흐름 차단 없음

### M5: Multimodal History Fidelity
- `_meta.source_path`: 이미지 content block에 원본 파일 경로 보존
- `_strip_content_meta()`: provider API 호출 전 `_meta` 제거
- `_save_turn()`: `[image]` → `[image: /path/to/file]` 경로 포함 placeholder

### M6: 문서화
- 본 파일
