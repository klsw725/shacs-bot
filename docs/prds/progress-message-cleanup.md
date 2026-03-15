# PRD: Progress 메시지 정리 및 Discord typing 수정

---

## 문제

현재 에이전트가 도구를 호출할 때 LLM이 생성한 중간 "생각(thought)" 텍스트가 사용자에게 그대로 전달된다:

```
사용자: "이 영상 요약해줘"
봇: "summarize 스킬로 자막을 먼저 추출하겠습니다. 잠시만요."  ← LLM 독백
(30초 침묵)
봇: "영상 요약입니다: ..."  ← 최종 응답
```

**문제점**:
1. "잠시만요" 메시지가 최종 응답과 시각적으로 구분이 안 되어 사용자가 봇이 끝났는지 아직 작업 중인지 알 수 없다
2. Discord에서는 progress 메시지 전송 시 typing indicator까지 중지되어 "끝난 것 같은" 인상을 더 강하게 준다
3. `send_progress` 기본값이 `True`라 모든 신규 사용자가 이 혼란을 겪는다

## 해결책

1. `send_progress` 기본값을 `False`로 변경하여 LLM 독백을 기본 비활성화
2. Discord의 `send()` 메서드에서 progress 메시지 전송 시 typing indicator를 유지하도록 수정

## 사용자 영향

| Before | After |
|---|---|
| LLM 독백("잠시만요")이 사용자에게 전달됨 | 최종 응답만 전달됨 |
| Discord에서 progress 전송 시 typing 중지 | typing 유지됨 |
| 사용자가 봇 상태를 판단할 수 없음 | typing indicator로 "작업 중" 인지 가능 |
| config에서 끄려면 `sendProgress: false` 설정 필요 | 기본 동작. 원하면 `sendProgress: true`로 활성화 |

## 기술적 범위

- **변경 파일**: `shacs_bot/config/schema.py`, `shacs_bot/channels/discord.py` (2개)
- **변경 유형**: 기본값 변경 1줄, 조건문 추가 1줄
- **의존성**: 없음
- **하위 호환성**: `sendProgress: true` 설정 시 기존과 동일 동작

### 변경할 코드 요약

**schema.py** (line 204):
- `send_progress` 기본값 `True` → `False`

**discord.py** (line 120-123):
- `send()` 메서드의 `finally` 블록에서 `_progress` 메타데이터 확인 추가
- Telegram(line 241), Matrix(line 451)와 동일한 패턴 적용

```python
# discord.py send() — Before
finally:
    await self._stop_typing(msg.chat_id)

# After
finally:
    if not msg.metadata.get("_progress", False):
        await self._stop_typing(msg.chat_id)
```

## 성공 기준

1. `send_progress=False`(기본값)일 때 LLM 독백이 채널에 전달되지 않는다
2. Discord에서 도구 실행 중 typing indicator가 유지된다 (progress 메시지 전송 후에도)
3. 최종 응답은 정상적으로 전달된다
4. config에서 `sendProgress: true` 설정 시 기존처럼 중간 텍스트가 전달된다
5. Telegram, Matrix의 기존 typing 동작에 영향 없다

---

## 마일스톤

- [x] **M1: send_progress 기본값 변경 + Discord typing 수정**
  `schema.py` 기본값 변경, `discord.py` progress 조건문 추가. CLI 및 채널에서 동작 확인.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 기존 사용자가 progress 메시지 의존 | 낮음 | 낮음 | `sendProgress: true`로 복원 가능 |
| Discord typing이 너무 오래 지속 | 낮음 | 낮음 | 최종 응답 전송 시 자동 중지됨 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-16 | PRD 초안 작성 |
| 2026-03-16 | M1 완료. `schema.py` send_progress 기본값 True→False 변경 (1줄). `discord.py` send() finally 블록에 `_progress` 메타데이터 조건 추가 — Telegram/Matrix와 동일 패턴. LSP 진단: 기존 타입 이슈만 존재, 변경 관련 에러 없음. |
