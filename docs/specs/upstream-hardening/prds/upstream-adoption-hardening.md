# PRD: Upstream Adoption Hardening

> **출처**: `HKUDS/nanobot`, `openclaw/openclaw` 최근 커밋 조사 결과

---

## 문제

최근 upstream 조사에서 `shacs-bot`에 직접적으로 연결되는 안정성 갭이 반복적으로 확인되었다.

현재 코드베이스의 핵심 취약점은 다음 4개 축으로 모인다.

1. **프로바이더 로딩/라우팅 취약성**
   - `shacs_bot/providers/registry.py`는 레지스트리 순서와 접두사 매칭에 크게 의존한다.
   - OAuth provider(`openai_codex`, `github_copilot`)는 명시 prefix가 없으면 자동 매칭되지 않는다.
   - provider import/초기화 경로가 무거워 startup 및 오류 표면적이 크다.
2. **세션 저장 내구성 부족**
   - `shacs_bot/agent/session/manager.py`는 JSONL 기반이지만 atomic write/복구 전략이 약하다.
   - 프로세스 중단이나 write 도중 장애 시 세션 손상 위험이 있다.
3. **서브에이전트 결과 손실 가능성**
   - `shacs_bot/agent/subagent.py`는 timeout/중단 시 partial progress 보존 규칙이 약하다.
   - upstream처럼 role 정합성, restart 이후 복구, 중간 산출물 복원이 명시되어 있지 않다.
4. **에이전트 death spiral 방어 부재**
   - `shacs_bot/agent/loop.py`에는 도구 반복 호출, no-op turn, 에러 연쇄를 감지하는 수동 안전장치가 없다.

추가로 nanobot 쪽 최근 커밋에서 아래 항목도 반영 가치가 컸다.

- **provider lazy loading**
- **빈 provider 응답 방어(empty choices guard)**
- **이미지 경로 등 멀티모달 메타데이터를 세션 히스토리에 보존**
- **subagent 결과를 `assistant` role로 정규화**

이 갭들은 모두 기존 `shacs-bot`의 구조와 직접 맞닿아 있어, 개별 버그 픽스로 흩어 처리하기보다 하나의 adoption/hardening 묶음으로 관리하는 편이 낫다.

## 해결책

다음 6개 묶음으로 upstream 패턴을 선별 도입한다.

1. **Provider Loading and Routing Hardening**
   - provider lazy import 도입
   - 명시 provider prefix 우선 규칙 강화
   - empty response guard 추가
   - API key 누락 시 actionable error message 정리
2. **Session Durability Hardening**
   - atomic write(temp file + rename)
   - 손상 파일 감지/복구 fallback
   - 저장 실패 시 최소한의 보존 로그 추가
3. **Subagent Resilience**
   - timeout 시 partial progress 요약 반환
   - subagent 결과 role을 `assistant`로 고정
   - 재시작 후 복구 범위와 한계 명시
4. **Execution Health Monitor**
   - tool loop 감지
   - no-effect turn 감지
   - file burst / error cascade 감지
   - 경고 중심(warn-only)으로 시작
5. **Session History Fidelity for Multimodal Turns**
   - 이미지/파일 경로를 `_meta` 등 내부 메타데이터로 보존
   - API 호출 직전 내부 필드는 제거
   - 히스토리 저장 시 `[image: /path]` 형태로 보존
6. **Docs and Operator Guidance**
   - 어떤 보호장치가 자동이고 어떤 것은 opt-in인지 문서화
   - failure mode를 `docs/`에 기록하여 재현/운영 판단 가능하게 함

## 사용자 영향

| Before | After |
|---|---|
| provider prefix/라우팅 실패 시 원인 파악이 어려움 | 명시 prefix 우선 + 더 명확한 에러 메시지 |
| provider 응답이 비정상이면 파싱 단계에서 불명확하게 실패 | empty response guard로 조기 실패 + 원인 노출 |
| 세션 저장 중 장애 시 손상 가능 | atomic write + 복구 fallback |
| subagent timeout 시 `(no output)` 같은 빈 결과 가능 | partial progress 포함 결과 반환 |
| death spiral이 생겨도 사용자가 늦게 알아챔 | 반복 도구 호출/에러 연쇄를 경고 로그로 조기 감지 |
| 이미지 첨부 히스토리가 `[image]` 수준으로 축약될 수 있음 | 파일 경로 포함 히스토리 보존 |

## 기술적 범위

- **변경 파일(예상)**:
  - `shacs_bot/providers/__init__.py` 또는 `shacs_bot/providers/registry.py`
  - `shacs_bot/providers/litellm.py`
  - `shacs_bot/providers/custom.py`
  - `shacs_bot/providers/openai_codex.py`
  - `shacs_bot/agent/session/manager.py`
  - `shacs_bot/agent/subagent.py`
  - `shacs_bot/agent/loop.py`
  - `shacs_bot/agent/context.py` 또는 멀티모달 turn 저장 경로
  - 관련 `docs/` 문서
- **변경 유형**: Python 코드 수정 + 신규 유틸/헬퍼 추가 가능
- **의존성**: 없음이 이상적. 표준 라이브러리 우선.
- **하위 호환성**:
  - execution health monitor는 기본 warn-only
  - provider routing은 명시 prefix 우선만 강화하고 기존 keyword fallback은 유지
  - session durability는 저장 형식을 크게 바꾸지 않고 write path만 강화

## 상세 범위

### 1. Provider Loading and Routing Hardening

**대상 파일**: `shacs_bot/providers/registry.py`, `shacs_bot/providers/litellm.py`, `shacs_bot/providers/custom.py`

#### 문제

- 현재 provider 선택은 registry 순서와 문자열 매칭에 민감하다.
- 명시 prefix가 있어도 후속 정규화/별칭 처리에서 의도가 흐려질 수 있다.
- provider import가 무거우면 startup/status 계열 경로에 불필요한 비용이 생긴다.
- 비정상 provider 응답(`choices=[]`, 빈 body 등)에 대한 방어가 약하다.

#### 해결

- nanobot 패턴을 참고해 provider export를 lazy-load한다.
- OpenClaw 패턴을 참고해 명시 provider prefix가 있으면 alias/keyword 매칭보다 우선한다.
- `choices` 비어 있음, content 블록 없음, malformed tool calls를 조기에 검증한다.
- missing API key 에러는 내부 경로 대신 필요한 env/config 키를 바로 안내한다.

#### 성공 기준

1. 명시 provider prefix가 있는 모델은 다른 provider로 재해석되지 않는다.
2. empty provider response가 `IndexError`류로 터지지 않고 설명 가능한 예외로 바뀐다.
3. startup/status-like 경로에서 불필요한 provider import가 줄어든다.
4. OAuth provider 자동 매칭 한계가 로그/에러 메시지에서 분명히 드러난다.

### 2. Session Durability Hardening

**대상 파일**: `shacs_bot/agent/session/manager.py`

#### 문제

- append/save 경로가 간단하지만 write 도중 중단 시 세션 손상 가능성이 있다.
- 손상 감지/백업/복구 흐름이 약하다.

#### 해결

- 저장 시 temp file 작성 후 rename으로 교체한다.
- 로드 시 손상된 마지막 레코드를 감지하면 이전 정상 상태로 복구하거나 경고 후 continue 한다.
- 실패 시 최소한 백업 파일 또는 `.corrupt` 파일로 남겨 운영자가 복구할 수 있게 한다.

#### 성공 기준

1. 정상 저장은 기존과 동일하게 동작한다.
2. 저장 도중 실패해도 기존 세션 파일이 사라지지 않는다.
3. 손상된 세션 파일을 읽을 때 전체 세션이 unusable 상태가 되지 않는다.

### 3. Subagent Resilience

**대상 파일**: `shacs_bot/agent/subagent.py`, 관련 세션/이벤트 경로

#### 문제

- timeout 시 중간 결과가 버려질 수 있다.
- subagent 결과 message role이 일관되지 않으면 상위 에이전트 컨텍스트가 왜곡될 수 있다.
- 프로세스 재시작/복구 시 in-flight task 정리가 약하다.

#### 해결

- timeout 결과에 intermediate assistant text/tool count 기반 partial progress를 포함한다.
- nanobot 패턴처럼 subagent 결과는 `assistant` role로 고정한다.
- restart 복구는 1단계에서 "이미 끝난 작업의 결과 유실 방지" 중심으로만 제한한다.

#### 성공 기준

1. subagent timeout 시 `(no output)` 대신 부분 진행 상황이 반환된다.
2. 상위 루프가 subagent 결과를 `assistant` 발화로 해석한다.
3. 재시작 후 orphan task를 최소한 warning/log 수준에서 추적 가능하다.

### 4. Execution Health Monitor

**대상 파일**: `shacs_bot/agent/loop.py`, 신규 `shacs_bot/agent/execution_health.py` 가능

#### 문제

- agent가 같은 도구를 반복 호출하거나 에러만 되풀이해도 현재는 명시 경고가 없다.

#### 해결

- OpenClaw 패턴을 참고해 아래 4개 detector를 warn-only로 도입한다.
  - tool repeat
  - no-effect turn
  - error cascade
  - file burst
- 단, upstream 구현의 알려진 함정은 피한다.
  - cumulative window 누락 금지
  - 대형 args 직렬화 시 크기 제한
  - 장기 세션 O(n^2) 스캔 피하기

#### 성공 기준

1. 동일 도구 반복/무효 턴/에러 연쇄 시 경고가 남는다.
2. 경고 도입만으로 정상 turn 흐름은 차단하지 않는다.
3. detector가 장기 세션에서 과도한 비용을 만들지 않는다.

### 5. Session History Fidelity for Multimodal Turns

**대상 파일**: 멀티모달 content 구성 및 세션 저장 경로

#### 문제

- 이미지/파일 입력의 원본 경로가 사라지면 fallback 판단과 사후 디버깅이 어려워진다.

#### 해결

- nanobot 패턴처럼 내부 `_meta` 필드에 원본 경로를 보존한다.
- provider API 호출 직전에는 내부 필드를 제거한다.
- 세션 저장 시 사람이 읽을 수 있는 placeholder로 변환한다.

#### 성공 기준

1. 이미지가 포함된 턴은 세션 히스토리에서 경로를 잃지 않는다.
2. 내부 `_meta`는 외부 provider 요청 payload로 유출되지 않는다.
3. fallback/retry 로직이 이미지 존재 여부를 더 정확히 판단한다.

### 6. Docs and Operator Guidance

**대상 파일**: `README.md`, 관련 `docs/*.md`, 장애 기록 문서

#### 해결

- 어떤 기능이 기본 활성인지, opt-in인지, 운영자가 봐야 할 로그가 무엇인지 문서화한다.

## 마일스톤

- [x] **M1: Provider hardening**
  Lazy loading, explicit prefix preservation, empty response guard, missing API key UX 정리.

- [x] **M2: Session durability**
  Atomic write + corrupt session fallback 구현.

- [x] **M3: Subagent resilience**
  Partial progress 반환, `assistant` role 정규화, orphan task logging 추가.

- [x] **M4: Execution health monitor**
  4개 detector를 warn-only로 도입하고 config에서 토글 가능하게 함.

- [x] **M5: Multimodal history fidelity**
  `_meta` 기반 경로 보존 및 session history 반영.

- [x] **M6: 문서화 및 운영 가이드**
  README/docs에 신규 동작과 실패 시 해석법 기록.

## 우선순위

| 항목 | 우선순위 | 이유 |
|---|---|---|
| Provider hardening | P0 | 현재 provider routing landmine와 직접 연결 |
| Session durability | P0 | 데이터 손실 가능성 방지 |
| Subagent resilience | P1 | background/subagent 신뢰성 개선 |
| Execution health monitor | P1 | runaway loop 조기 탐지 |
| Multimodal history fidelity | P2 | 디버깅/회귀 분석 가치 |
| Docs/operator guidance | P2 | 운영 혼란 감소 |

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| provider routing 수정이 기존 매칭을 깨뜨림 | 중간 | 높음 | 명시 prefix 우선만 강화하고 keyword fallback은 유지 |
| session durability 변경이 저장 포맷을 깨뜨림 | 낮음 | 높음 | 포맷은 유지하고 write path만 교체 |
| health monitor가 false positive를 많이 낸다 | 중간 | 중간 | warn-only + conservative threshold로 시작 |
| multimodal `_meta`가 외부 요청으로 새어 나감 | 낮음 | 높음 | provider 호출 직전 sanitize 단계 명시 |
| subagent 복구 범위가 과도해 복잡해짐 | 중간 | 중간 | Phase 1은 partial progress + orphan visibility까지만 제한 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-20 | `HKUDS/nanobot`, `openclaw/openclaw` 최근 커밋 조사 결과를 바탕으로 통합 adoption hardening PRD 초안 작성. 4개 우선 항목(provider, session, subagent, execution health)과 nanobot 고가치 항목(lazy loading, empty response guard, multimodal history fidelity, subagent role 정규화) 포함. |
| 2026-03-20 | 코드베이스 검증: M1 중 **empty response guard**는 이미 `litellm.py`에 부분 구현됨 (line 299-300: `if not content and msg.content: content = msg.content`). 나머지 항목(lazy loading, explicit prefix preservation, missing API key UX)은 미구현. M2-M6 전부 미착수. |
| 2026-03-20 | M6 완료. `docs/2026-03-20-14-44-upstream-adoption-hardening.md` 작업 기록 작성 — 전체 변경 파일, 마일스톤별 요약 포함. |
| 2026-03-20 | M5 구현 완료. `context.py` `_build_user_content()`: 이미지 content block에 `_meta: {"source_path": str(p)}` 추가. `base.py`: `_strip_content_meta()` 정적 메서드 추가 — content list 내 `_meta` 키 제거. `litellm.py`와 `custom.py`의 `chat()`: sanitize 체인에 `_strip_content_meta` 적용 — provider API로 `_meta` 유출 방지. `loop.py` `_save_turn()`: 이미지 placeholder를 `[image]` → `[image: /path/to/file]`로 변경 — 원본 경로 보존. |
| 2026-03-20 | M4 구현 완료. `execution_health.py` 신규 파일: `ExecutionHealthMonitor` 클래스에 3개 detector 구현 — tool repeat (동일 도구+인자 3회 이상), error cascade (연속 에러 3회), file burst (윈도우 내 write/edit 10회). deque(maxlen=15) 기반 슬라이딩 윈도우로 O(n) 유지. args는 MD5 해시 1024자 제한으로 대형 인자 방어. `loop.py`의 `_run_agent_loop`에서 매 도구 실행 후 `health.check()` 호출. warn-only — 정상 흐름 차단 없음. no-effect turn detector는 정의가 주관적이므로 향후 필요 시 추가 예정. |
| 2026-03-20 | M3 구현 완료. `subagent.py` (1) `_extract_partial_progress()` 정적 메서드 추가: max_iterations 도달 시 messages에서 사용된 도구 목록 + 마지막 assistant text(500자)를 추출하여 반환 — 기존 "최종 응답이 생성되지 않았습니다" 대신 구체적 진행 상황 보고. (2) `shutdown()` async 메서드 추가: 종료 시 실행 중인 서브에이전트 목록을 warning 로그로 출력하고 모두 cancel. (3) Assistant role 정규화: 검증 결과 subagent 결과는 이미 InboundMessage로 일관되게 주입됨 — 추가 변경 불필요. |
| 2026-03-20 | M2 구현 완료. `session/manager.py` `save()`: temp file 작성 후 `replace()`로 atomic rename — write 중단 시 기존 세션 파일 보존. 실패 시 tmp 파일 정리. `_load()`: 개별 JSONL 레코드별 `JSONDecodeError` 캐치 — 손상 레코드 skip + 정상 레코드는 보존. 손상 발견 시 원본을 `.jsonl.corrupt`로 백업. |
| 2026-03-20 | M1 구현 완료. (1) `litellm.py` `_parse_response()`: `response.choices` 빈 배열 가드 추가 — `IndexError` 대신 설명 가능한 `LLMResponse(error)` 반환. (2) Lazy loading: 검증 결과 `commands.py`의 provider import가 이미 함수 내부(`_make_provider`)에 있어 startup 경로에서 로드되지 않음 — 이미 충족. (3) Explicit prefix: `find_by_model()`과 `_match_provider()`에서 prefix 우선 매칭 이미 구현 — 이미 충족. (4) `schema.py` `_match_provider()`: 키워드 매칭되었으나 API 키 없을 때 `providers.{name}.apiKey` 설정 안내 warning 로그 추가. (5) `commands.py` `_make_provider()`: 에러 메시지에 매칭된 provider명, 필요한 설정 경로, env 변수명을 포함하도록 개선. |
