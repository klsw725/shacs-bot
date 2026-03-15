# PRD: DuckDuckGo 폴백 웹 검색

---

## 문제

현재 `WebSearchTool`은 **Brave Search API만** 지원한다. `BRAVE_API_KEY`가 설정되지 않으면 웹 검색이 완전히 불가능하며, 에이전트가 에러 메시지를 반환한다:

```
에러: BRAVE_API_KEY가 설정되어 있지 않습니다.
```

shacs-bot의 원본인 nanobot은 이미 DuckDuckGo 폴백을 구현하여 API 키 없이도 웹 검색이 가능하다. shacs-bot은 clone 시점(0.1.4) 이후 추가된 이 기능이 누락된 상태다.

## 해결책

nanobot의 DuckDuckGo 폴백 패턴을 포팅한다:

1. `ddgs` 패키지를 의존성에 추가
2. `WebSearchTool`에 `_search_duckduckgo()` 메서드 추가
3. `BRAVE_API_KEY` 미설정 시 DuckDuckGo로 자동 폴백

## 사용자 영향

| Before | After |
|---|---|
| BRAVE_API_KEY 없으면 웹 검색 불가 | API 키 없어도 DuckDuckGo로 검색 가능 |
| 새 사용자가 반드시 Brave API 가입 필요 | 설치 직후 바로 웹 검색 사용 가능 |
| 서브에이전트 researcher 역할이 웹 검색 불가 | researcher가 즉시 동작 |

## 기술적 범위

- **변경 파일**: `shacs_bot/agent/tools/web.py` (1개)
- **변경 유형**: Python 코드 추가
- **의존성 추가**: `ddgs>=9.5.5,<10.0.0` (`pyproject.toml`)
- **하위 호환성**: BRAVE_API_KEY 설정된 환경은 기존과 동일하게 Brave API 사용. 변경 없음.

### 변경할 코드 요약

**web.py**:
- `_search_duckduckgo(query, n)` 비동기 메서드 추가 — `asyncio.to_thread`로 동기 `ddgs.text()` 래핑
- `execute()`에서 `api_key` 없을 때 에러 반환 대신 `_search_duckduckgo()` 호출
- 기존 `_format` 로직을 공유하여 Brave/DuckDuckGo 결과 형식 통일

## 성공 기준

1. `BRAVE_API_KEY` 미설정 상태에서 `web_search` 도구 호출 → DuckDuckGo 결과 반환
2. `BRAVE_API_KEY` 설정된 상태에서 → 기존과 동일하게 Brave API 사용 (하위 호환)
3. DuckDuckGo 검색 실패 시 → 에러 메시지 반환 (조용히 실패하지 않음)
4. 로그에 폴백 사용 여부 명시 (`"BRAVE_API_KEY not set, falling back to DuckDuckGo"`)

---

## 마일스톤

- [x] **M1: DuckDuckGo 폴백 구현**
  `ddgs` 의존성 추가, `_search_duckduckgo()` 메서드 구현, `execute()`에서 폴백 로직 적용. BRAVE_API_KEY 설정 시 기존 동작 유지.

- [x] **M2: 실행 검증**
  BRAVE_API_KEY 미설정 상태에서 웹 검색 요청 → DuckDuckGo 결과 정상 반환 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| DuckDuckGo rate limiting | 중간 | 중간 | 과도한 호출 시 에러 반환, 사용자에게 Brave API 설정 안내 |
| ddgs 패키지 API 변경 | 낮음 | 중간 | 버전 고정 (`<10.0.0`), try/except로 방어 |
| DuckDuckGo 검색 품질이 Brave보다 낮음 | 중간 | 낮음 | 폴백 전략이므로 허용 가능. Brave 설정 시 더 나은 결과 안내 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-15 | PRD 초안 작성. nanobot 최신 버전에서 DuckDuckGo 폴백 패턴 확인. |
| 2026-03-15 | M1+M2 완료. `ddgs>=9.5.5` 의존성 추가, `_search_duckduckgo()` 구현, `execute()`에서 api_key 없을 시 폴백. `_format_results()` 공통 포맷 메서드 추출. CLI 테스트: `web_search` → WARNING 로그 + DuckDuckGo 5개 결과 정상 반환. |
