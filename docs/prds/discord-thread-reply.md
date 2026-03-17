# PRD: Discord 쓰레드 응답

---

## 문제

현재 Discord에서 봇이 응답하면 같은 채널에 일반 메시지(또는 reply)로 전송된다:

```
#general 채널
유저A: "이 코드 설명해줘"
봇: "이 코드는..." ← 채널에 그대로 노출
유저B: "오늘 날씨 어때?"
봇: "서울 날씨는..." ← 어떤 질문에 대한 답인지 혼란
```

**문제점**:
1. 여러 사용자가 동시에 대화하면 응답이 섞여 맥락을 잃는다
2. 긴 응답이 채널을 도배한다
3. 채널 단위 세션이라 다른 유저의 대화가 서로 간섭한다

## 해결책

Slack 채널에 이미 구현된 `reply_in_thread` 패턴을 Discord에 적용한다:

1. 길드 채널에서 메시지 수신 시 해당 메시지에 쓰레드를 생성
2. 봇 응답을 쓰레드 안에서 전송
3. 쓰레드 단위로 세션을 분리하여 대화 맥락 유지

```
#general 채널
유저A: "이 코드 설명해줘"
  └─ [쓰레드] 봇: "이 코드는..."  ← 쓰레드 안에서 응답
유저B: "오늘 날씨 어때?"
  └─ [쓰레드] 봇: "서울 날씨는..."  ← 별도 쓰레드
```

## 사용자 영향

| Before | After |
|---|---|
| 응답이 채널에 직접 노출 | 쓰레드 안에서 응답 |
| 여러 유저 대화가 섞임 | 쓰레드별 독립 대화 |
| 채널 단위 세션 (간섭) | 쓰레드 단위 세션 (격리) |
| DM에서는 변화 없음 | DM에서는 변화 없음 |

## 기술적 범위

- **변경 파일**: `config/schema.py`, `channels/discord.py` (2개)
- **의존성**: 없음 (기존 httpx + Discord REST API 사용)
- **하위 호환성**: `reply_in_thread` 기본값 `False` → 기존 동작 유지. 설정으로 활성화.

### 참고 패턴: Slack 채널

Slack은 이미 동일한 패턴을 구현하고 있다 (`channels/slack.py`):
- 설정: `reply_in_thread: bool = True`
- 인바운드: `thread_ts` 추출 → metadata에 저장 → `session_key`로 쓰레드 스코핑
- 아웃바운드: metadata에서 `thread_ts` 꺼내 Slack API에 전달

### Discord API 스펙

**쓰레드 생성**: `POST /channels/{channel_id}/messages/{message_id}/threads`
```json
{
  "name": "대화",
  "auto_archive_duration": 1440
}
```
→ Thread 채널 객체 반환 (thread_id = 일종의 channel_id)

**쓰레드 내 메시지 전송**: `POST /channels/{thread_id}/messages`
→ thread_id를 channel_id처럼 사용하면 됨

**필요 권한**: `CREATE_PUBLIC_THREADS` (봇 권한)
**필요 인텐트**: 기존 `GUILDS + GUILD_MESSAGES` (현재 intents=37377에 이미 포함)
**auto_archive_duration**: 60 (1시간) / 1440 (24시간) / 4320 (3일) / 10080 (7일)

**쓰레드 내 메시지 수신**: MESSAGE_CREATE의 `channel_id`가 thread_id로 옴. 별도 처리 필요.

### 변경할 코드 요약

**schema.py** — `DiscordConfig`:
```python
reply_in_thread: bool = False
thread_auto_archive_minutes: int = 1440  # 24시간
```

**discord.py** — `_handle_message_create`:
```python
# 길드 채널 + reply_in_thread 활성화 시:
# 1. 기존 쓰레드 안의 메시지인지 확인 (channel_type == 11 또는 12)
#    → 맞으면 기존 쓰레드에서 응답 (thread_id = channel_id)
# 2. 새 메시지면 쓰레드 생성
#    → POST /channels/{channel_id}/messages/{message_id}/threads
#    → thread_id를 metadata에 저장
# 3. session_key = f"discord:{channel_id}:{thread_id}"
# 4. typing indicator를 thread_id에서 실행
```

**discord.py** — `send`:
```python
# metadata에 thread_id가 있으면 thread_id로 메시지 전송
# url = f"{DISCORD_API_BASE}/channels/{thread_id}/messages"
# DM이면 기존대로 channel_id 사용
```

### 세션 동작

| 상황 | session_key | 결과 |
|---|---|---|
| 길드에서 새 메시지 → 쓰레드 생성 | `discord:{channel_id}:{thread_id}` | 새 세션 |
| 기존 쓰레드 안에서 메시지 | `discord:{parent_channel_id}:{thread_id}` | 기존 세션 이어감 |
| DM | `discord:{channel_id}` | 기존대로 (쓰레드 없음) |

### 쓰레드 내 메시지 감지

Discord에서 쓰레드 내 메시지는 `channel_type`으로 구분:
- `11` = PUBLIC_THREAD
- `12` = PRIVATE_THREAD

Gateway MESSAGE_CREATE에서 `channel_id`가 쓰레드 ID이고, 페이로드에 쓰레드 메타데이터가 포함됨. 이를 활용해 "이미 쓰레드 안에 있는 메시지"를 감지하고, 새 쓰레드를 생성하지 않고 기존 쓰레드에서 응답.

## 성공 기준

1. `reply_in_thread=True` 시 길드 채널의 유저 메시지에 쓰레드가 생성된다
2. 봇 응답이 해당 쓰레드 안에서 전송된다
3. 동일 쓰레드 내 후속 메시지는 같은 세션에서 처리된다 (대화 맥락 유지)
4. DM에서는 쓰레드 없이 기존대로 동작한다
5. `reply_in_thread=False`(기본값)일 때 기존 동작과 동일하다
6. 쓰레드 생성 실패 시 폴백으로 기존 채널에 응답한다

---

## 마일스톤

- [x] **M1: 설정 추가 및 쓰레드 생성 + 응답 라우팅**
  `DiscordConfig`에 `reply_in_thread`, `thread_auto_archive_minutes` 추가.
  `_handle_message_create`에서 길드 메시지 수신 시 쓰레드 생성, metadata에 thread_id 저장, session_key 쓰레드 스코핑.
  `send`에서 thread_id로 메시지 전송. 쓰레드 생성 실패 시 폴백.

- [x] **M2: 기존 쓰레드 내 메시지 처리**
  쓰레드 안에서 온 메시지(channel_type=11/12) 감지. 새 쓰레드 생성 없이 기존 쓰레드에서 응답.
  쓰레드 내 메시지는 group_policy 체크 스킵 (쓰레드 진입 = 대화 의도).
  session_key = `discord:{thread_id}`로 쓰레드 단위 세션 유지.

- [ ] **M3: 실사용 검증 및 문서 기록**
  Discord 서버에서 실제 동작 확인: 쓰레드 생성, 쓰레드 내 대화, DM 동작, 폴백.
  docs/ 작업 기록.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 쓰레드 생성 rate limit (10/10min/guild) | 중간 | 중간 | 실패 시 기존 채널에 폴백 응답 |
| 봇에 CREATE_PUBLIC_THREADS 권한 없음 | 낮음 | 높음 | 에러 로그 + 채널 폴백 |
| 쓰레드 아카이브 후 재활성화 | 낮음 | 낮음 | 메시지 전송 시 자동 unarchive됨 |
| 쓰레드 내 메시지의 parent_channel_id 누락 | 낮음 | 중간 | Gateway READY 시 채널 캐시 또는 REST API 폴백 |

---

## 진행 로그

| 날짜 | 내용 |
|---|---|
| 2026-03-17 | PRD 초안 작성 |
| 2026-03-17 | M1, M2 구현 완료. `schema.py` DiscordConfig에 reply_in_thread/thread_auto_archive_minutes 추가. `discord.py`에 _create_thread 메서드, send() thread 라우팅, _handle_message_create 쓰레드 생성/감지/세션 스코핑 구현. LSP 진단: 기존 타입 이슈만 존재, 변경 관련 에러 없음. |
