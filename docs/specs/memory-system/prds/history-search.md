# PRD: HISTORY.md 검색 도구

---

## 문제

현재 메모리 시스템은 이중 계층 구조다:
- **MEMORY.md** — 장기 사실 저장. 시스템 프롬프트에 항상 주입됨.
- **HISTORY.md** — 대화 이벤트 로그. 통합 시 append되지만 **읽히지 않음**.

HISTORY.md는 `context.py`에서 "grep으로 검색 가능"이라고 안내하지만, 실제 검색 메커니즘이 없다:

```python
# context.py:90 — 안내만 있고 도구 없음
"히스토리 로그: {workspace_path}/memory/HISTORY.md (grep으로 검색 가능)"
```

**문제점**:
1. 봇이 과거 대화를 회상할 수 없음 — MEMORY.md에 축약된 사실만 있고 상세 맥락은 HISTORY.md에만 존재
2. "지난주에 뭐 얘기했지?", "그때 추천해준 영화 뭐였어?" 같은 요청에 답변 불가
3. HISTORY.md가 계속 축적되지만 활용되지 않아 사실상 데드 데이터
4. 봇이 `exec grep`으로 검색할 수는 있지만, 이를 유도하는 도구나 가이드가 없음

## 해결책

`search_history` 도구를 추가하여 LLM이 HISTORY.md를 검색할 수 있게 한다:

1. `search_history` 도구 구현 — 키워드 기반 HISTORY.md grep 검색
2. 관련 엔트리만 추출하여 반환 (전체 파일이 아닌 매칭 단락)
3. SOUL.md에 기억 회상 가이드 추가

## 사용자 영향

| Before | After |
|---|---|
| "지난번에 뭐 추천해줬어?" → 답변 불가 | HISTORY.md 검색 → 과거 추천 내용 회상 |
| MEMORY.md 축약 사실만 사용 | HISTORY.md 상세 맥락도 활용 가능 |
| HISTORY.md가 쌓이기만 하는 데드 데이터 | 실제 검색되어 가치 창출 |
| 봇이 과거 대화 맥락을 잃음 | 장기 사용 시 대화 품질 향상 |

## 기술적 범위

- **신규 파일**: `shacs_bot/agent/tools/history.py` (1개)
- **변경 파일**: `shacs_bot/agent/loop.py` (도구 등록), `shacs_bot/templates/SOUL.md` (가이드 추가)
- **변경 유형**: Python 코드 추가
- **의존성**: 없음. 표준 라이브러리만 사용.

### search_history 도구 설계

**파라미터**:
- `query` (필수): 검색 키워드 또는 문구
- `max_results` (선택): 최대 반환 엔트리 수 (기본: 10)

**동작**:
1. HISTORY.md 파일을 읽음
2. 빈 줄(`\n\n`)로 엔트리를 분리
3. `query`가 포함된 엔트리를 필터링 (대소문자 무시)
4. 최신순으로 정렬하여 `max_results`개 반환
5. 파일이 없거나 매칭 없으면 적절한 메시지 반환

**반환 형식**:
```
[검색 결과: "영화" — 3개 매칭]

[2026-03-10 14:30] 사용자가 SF 영화 추천을 요청함. "인터스텔라", "듄", "블레이드 러너 2049"를 추천.

[2026-03-05 09:15] 주말 영화 계획 논의. 사용자가 "듄 파트2"를 보겠다고 결정.

[2026-02-28 20:00] 사용자가 최근 본 영화 "오펜하이머" 감상 공유. 평점 9/10.
```

### SOUL.md 가이드 추가

의도 파악 테이블에 기억 회상 유형 추가:
```
| 기억 회상 | "그때 뭐 얘기했지?", "지난번에 추천해준 거" | search_history로 과거 대화 검색. |
```

### 도구 등록

`AgentLoop._register_default_tools()`에 `SearchHistoryTool` 추가.

## 성공 기준

1. `search_history(query="영화")` 호출 → HISTORY.md에서 "영화" 포함 엔트리 반환
2. 매칭 없으면 "검색 결과가 없습니다" 반환 (에러 아님)
3. HISTORY.md 파일이 없으면 "히스토리가 아직 없습니다" 반환
4. 대소문자 무시 검색
5. 최신순 정렬
6. "그때 추천해준 영화 뭐였어?" → 봇이 search_history 호출 → 과거 추천 내용 답변

---

## 마일스톤

- [x] **M1: search_history 도구 구현 및 등록**
  `SearchHistoryTool` 클래스 구현 (history.py), 키워드 기반 HISTORY.md 검색, `_register_default_tools()`에 등록. 서브에이전트 researcher/analyst에도 허용.

- [x] **M2: SOUL.md 가이드 추가 및 검증**
  의도 파악 테이블에 "기억 회상" 유형 추가. CLI 테스트: "지난번에 뭐 얘기했지?" → search_history 호출 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| HISTORY.md가 매우 클 때 성능 저하 | 중간 | 중간 | 파일 크기 제한 또는 최신 N줄만 검색. 실측 후 필요시 최적화 |
| LLM이 search_history를 적절히 호출하지 않음 | 중간 | 중간 | SOUL.md에 기억 회상 가이드 추가 |
| 검색 결과가 너무 많아 컨텍스트 오염 | 낮음 | 중간 | max_results 기본값 10, 결과 길이 제한 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-15 | PRD 초안 작성. 현재 HISTORY.md가 write-only 상태임을 확인. context.py에서 "grep 검색 가능" 안내만 있고 실제 메커니즘 없음. |
| 2026-03-15 | M1+M2 완료. SearchHistoryTool 구현 (history.py 신규), loop.py에 등록, subagent.py에 인스턴스 생성 + allowed_tools 추가. SOUL.md 의도 파악 테이블에 "기억 회상" 유형 추가. 코드 리뷰 정적 검증 9개 포인트 전부 통과. |
