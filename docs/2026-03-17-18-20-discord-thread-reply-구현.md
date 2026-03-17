# Discord 쓰레드 응답 기능 구현

## 프롬프트

> 디스코드 응답할때 쓰레드로 응답하게 할 수 있어?

## 변경 사항

### config/schema.py
- `DiscordConfig`에 `reply_in_thread: bool = False` 추가
- `thread_auto_archive_minutes: int = 1440` 추가 (24시간 기본값)

### channels/discord.py

**`send()` 수정:**
- `metadata["thread_id"]`가 있으면 해당 thread_id로 메시지 전송
- 쓰레드 응답 시 `message_reference` (reply) 생략
- typing indicator도 thread_id 대상으로 정리

**`_create_thread()` 추가:**
- `POST /channels/{channel_id}/messages/{message_id}/threads` 호출
- 메시지 내용 앞 100자를 쓰레드 이름으로 사용
- rate limit (429) 처리 포함
- 실패 시 None 반환 → 폴백으로 기존 채널에 응답

**`_handle_message_create()` 수정:**
- `channel_type` 확인으로 기존 쓰레드 내 메시지 감지 (11=PUBLIC_THREAD, 12=PRIVATE_THREAD)
- 쓰레드 내 메시지는 `_should_respond_in_group` 체크 스킵
- 길드 채널 새 메시지 → `_create_thread` 호출 → `thread_id` metadata에 저장
- `session_key` 쓰레드 단위 스코핑: `discord:{thread_id}`
- typing indicator를 thread에서 실행

## 세션 동작

| 상황 | session_key |
|---|---|
| 길드에서 새 메시지 → 쓰레드 생성 | `discord:{thread_id}` |
| 기존 쓰레드 안에서 메시지 | `discord:{channel_id}` (= thread_id) |
| DM | `discord:{channel_id}` (기존대로) |

## 설정

```json
{
  "channels": {
    "discord": {
      "replyInThread": true,
      "threadAutoArchiveMinutes": 1440
    }
  }
}
```
