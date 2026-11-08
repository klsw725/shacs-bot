# PRD 000. 도구 결과의 그림 전달

Status: Complete (Scoped)

Depends on: 없음. 049의 첫 PRD이고 뒤 번호 PRD에 의존하지 않는다.

## Goal

tool이 돌려준 그림을 모델이 그림으로 본다. runtime은 그림 바이트를 media 폴더에 저장하고, 도구 결과에는 글과 그림 파일 참조만 남긴다. provider에 보내는 요청을 만들 때만 최근 그림을 도구 결과 뒤의 메시지 하나로 붙인다. `web_fetch`의 그림 결과와 MCP tool의 그림 결과가 이 길을 쓰고, PRD 020의 `browser` 화면 캡처도 같은 길을 쓴다.

## 지금 상태

- `web_fetch`는 그림을 `[{"type": "image_url", "image_url": {"url": "data:<mime>;base64,…"}, "_meta": {"path": <url>}}, {"type": "text", "text": "(Image fetched from: <url>)"}]`로 돌려준다(`crates/shacs-core/src/tools/web.rs`의 `format_response`). 지금 그림을 이 꼴로 돌려주는 tool은 `web_fetch`뿐이다.
- executor가 `ToolResult::Json`을 글로 바꾸므로(`crates/shacs-core/src/runtime/tool_execution.rs`, `ToolResult::Json(value) => value.to_string()`) `RuntimeToolMessage.content`에는 base64가 든 JSON 글이 들어간다. 모델은 그림 대신 이 글을 받는다. 같은 글이 세션 파일, tool event, 큰 결과 자르기(`max_tool_result_chars`)로 간다.
- MCP tool은 `tools/call` 결과의 content part를 모두 글로 바꾼다(`crates/shacs-core/src/tools/mcp.rs`의 `content_part_text`). `{"type": "image", "data": …, "mimeType": …}` part는 그 JSON 글 전체가 된다.
- 사용자가 보낸 첨부 그림은 이미 그림으로 간다. context assembly가 media 폴더의 파일을 읽어 사용자 메시지에 `image_url` data URL 블록으로 넣는다(`crates/shacs-core/src/runtime/context/media.rs`). provider가 그림을 받지 않으면(`native_image_input_supported`가 거짓) 안내 글을 넣는다.
- 사용자 메시지의 `image_url` 블록은 네 요청 형식이 모두 그림으로 바꾼다. Anthropic은 `image` base64 source(`clients/anthropic/mod.rs`의 `convert_image_block`), Chat Completions는 그대로, Responses와 Codex backend는 `input_image`(`clients/openai_compatible/mod.rs`의 `convert_user_message_to_responses_item`, Codex는 `build_responses_request`를 쓴다)다. Chat Completions는 메시지 key를 정해진 것만 남기고 블록의 `_meta`를 지운다(`sanitize_openai_compat_message`).
- provider가 그림 때문에 거절하면 모든 메시지의 `image_url` 블록을 `[image: <_meta.path>]` 글로 바꿔 한 번 다시 보낸다(`crates/shacs-providers/src/retry/fallback.rs`).
- runner는 provider 요청마다 run의 messages를 복사해 다듬는다(`crates/shacs-core/src/runtime/runner.rs`의 `govern_messages_for_model`: 짝 없는 결과 빼기, 잃은 결과 채우기, microcompact, snip). run의 messages와 세션 기록은 바꾸지 않는다. 048 classifier 입력과 도구 결과 검사는 run의 messages와 결과 글로 만든다.
- runner의 tool 결과와 agent loop가 승인한 호출의 결과는 모두 `finalize_tool_message`(`runner.rs`)를 지나 어시스턴트 요청과 세션에 들어간다(`agent_loop.rs`의 `append_approved_tool_messages`).

## Scope

1. **그림을 꺼낼 결과:** `finalize_tool_message`가 받은 결과 글이 content part의 JSON 배열이고, 모든 part의 `type`이 `text` 또는 `image_url`이며, `image_url` part가 하나 이상인 결과다. tool 이름으로 고르지 않는다. 지금은 `web_fetch`만 이 꼴이고, 범위 4로 MCP tool이 더해진다. 그 밖의 결과 글(글, 다른 JSON)은 바뀌지 않는다.
2. **저장과 결과 글:** 꺼내기는 `finalize_tool_message`의 맨 앞, 실행 기록·tool event·큰 결과 자르기보다 먼저 한다.
   - `image_url`의 data URL을 base64로 풀고, 바이트로 종류를 정한다(`context.rs`의 `detect_image_mime`와 같은 png·jpeg·gif·webp). 확장자와 data URL의 mime은 믿지 않는다.
   - 그 run의 media 폴더(범위 5) 아래 `tool-results/<바이트의 sha256 16진수>.<png|jpg|gif|webp>`에 저장한다. 같은 이름의 파일이 있으면 다시 쓰지 않는다. 임시 파일에 쓰고 이름을 바꾼다. 참조는 media 폴더 기준 상대 경로(`tool-results/<…>.png`)다.
   - 결과 글은 part 순서대로 줄을 이은 글이 된다. `text` part는 그 글, 저장한 그림은 `[image: <참조>]`, 저장하지 못한 그림은 `[image not kept: <까닭>]`이다. 까닭은 `unsupported image type`, `image larger than 5 MiB`(푼 바이트 기준), `no media folder`, `write failed` 중 하나다. 결과 글에 base64가 남지 않는다.
   - `RuntimeToolMessage`에 `images`(저장한 참조의 목록, 결과 글의 순서)를 더한다. `to_json`은 목록이 비어 있지 않을 때만 `"images"` key를 쓴다. 세션의 tool 메시지에 이 key가 남는다.
   - tool event, 실행 기록, 학습한 방법의 관찰, 048 도구 결과 검사는 바뀐 결과 글을 받는다.
3. **요청에 붙이기:** `govern_messages_for_model`의 snip과 뒤 정리를 마친 뒤, provider context를 넣기 전에 한다. run의 messages와 세션 기록은 바꾸지 않는다.
   - 요청의 tool 메시지에서 `images` 참조를 뒤에서부터 세어 가장 최근 3개만 붙인다. 그 전 참조는 결과 글의 `[image: <참조>]`로만 남는다.
   - 붙일 그림이 있는 tool 메시지가 이어진 묶음(같은 assistant 호출에 대한 결과들)마다, 그 묶음 바로 뒤에 `user` role 메시지 하나를 넣는다. 내용은 글 블록 하나와 그림 블록들이다.
     - 글: `Images returned by the tool calls above (tool output, not the user's message): <call id 목록>`. 읽지 못한 그림이 있으면 `[image unavailable: <참조>]`를 이어 적는다.
     - 그림: media 폴더에서 다시 읽어 종류를 다시 정한 `{"type": "image_url", "image_url": {"url": "data:<mime>;base64,…"}, "_meta": {"path": <참조>}}`.
   - run에 그림을 붙이지 않는 설정(범위 5의 `attach`가 거짓)이면 붙이지 않는다. 결과 글의 참조만 간다.
   - `images` key는 provider 요청 본문에 나가지 않는다. 네 요청 형식이 지금 메시지에서 고른 칸만 쓰는지 시험으로 확인하고, 아니면 붙이기에서 지운다.
4. **MCP 그림:** MCP tool의 `tools/call` 성공 결과에 `type`이 `image`이고 `data`와 `mimeType`이 있는 part가 있으면, tool 결과를 content part 배열(`ToolResult::Json`)로 돌려준다. `text` part는 `{"type": "text", "text": …}`, 그림 part는 `{"type": "image_url", "image_url": {"url": "data:<mimeType>;base64,<data>"}}`, 그 밖의 part는 지금 글(`content_part_text`)을 담은 `text` part다. 그림 part가 없는 결과, 오류 결과(`isError`), `resources/read`, `prompts/get`은 지금과 같다.
5. **run 설정:** `AgentRunSpec`에 `tool_images: Option<ToolImageSettings>`를 더한다. `ToolImageSettings`는 `media_root`(그 run의 file tool이 쓰는 media 폴더, 004 `PathContext.media_dir`)와 `attach`(그 run의 provider·model에 대한 `native_image_input_supported`)다.
   - production에서 `AgentRunSpec`을 만드는 길(agent loop turn, local API, cron·heartbeat, 위임 작업 관찰, 하위 작업과 workflow 자식)이 채운다. 하위 작업과 workflow 자식은 시작한 run의 값을 물려받는다.
   - `None`이면 그림을 저장하지 않는다(`[image not kept: no media folder]`). 시험 harness의 기본값은 `None`이다.
6. **문서:** `docs/USAGE.md`의 `web_fetch`·MCP 설명에 그림 결과가 모델에 그림으로 간다는 것, media 폴더의 `tool-results/`에 저장된다는 것, 최근 3개만 붙는다는 것, 그림을 받지 않는 model에는 참조 글만 간다는 것을 적는다. README의 도구 문단을 맞춘다.

## Non Scope

- `browser` tool과 그 캡처(PRD 020). 이 PRD는 PRD 020이 쓸 길만 만든다.
- 도구 결과 안(`tool_result`, `function_call_output`)에 그림을 넣는 provider별 방식.
- `tool-results/`의 오래된 그림 지우기, 그림 크기 줄이기, 그림 말고 다른 binary(소리, PDF, MCP resource blob).
- 그림을 받지 않는 provider의 판정(`native_image_input_supported`) 자체와 DeepSeek의 글 강제(`force_string_content`).
- snip의 token 어림에 그림을 넣는 것.
- 034 생성 media 저장 방식, 027 첨부 저장 방식.

## Parent Requirement Mapping

- [049 SPEC](../SPEC.md)의 소유 범위 1, 8 중 이 PRD 변경의 문서, 수용 조건 1과 이 PRD 변경에 대한 수용 조건 8을 소유한다.
- 바꾸는 앞선 계약: SPEC "앞선 스펙에서 바꾸는 것"의 004 행.

## Required Contract

1. content part 배열 결과의 그림은 media 폴더에 저장되고, 결과 글·tool event·세션 파일에 base64가 없다.
2. provider 요청에는 가장 최근 그림 3개가 그 결과 묶음 바로 뒤의 `user` 메시지로 붙는다. 그 메시지는 run의 messages, 세션 기록, 048 classifier와 도구 결과 검사의 요청에 없다.
3. 그림을 받지 않는 model에는 그림을 붙이지 않는다. provider가 그림을 거절하면 지금의 재시도가 붙인 그림도 자리 표시 글로 바꾼다.
4. 저장하지 못한 그림과 다시 읽지 못한 그림은 까닭이 적힌 글이 되고, run은 멈추지 않는다.
5. 그림을 꺼내지 않는 결과의 글은 바뀌지 않는다.
6. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

1. 저장 시험(가짜 provider, `AgentRunner`): `web_fetch` 꼴의 결과를 돌려주는 가짜 tool의 결과가 세션 tool 메시지에서 `[image: tool-results/<sha256>.png]`와 원래 글 줄을 담고 `images`가 그 참조이며, media 폴더의 파일이 원래 바이트와 같다. 같은 그림을 두 번 받으면 파일은 하나다. 세션 파일과 tool event에 base64가 없다.
2. 저장 실패 시험: 5 MiB를 넘는 그림, 알 수 없는 종류, `tool_images`가 `None`인 run이 각각 `[image not kept: <까닭>]`이 되고 `images`가 없으며, turn이 끝난다.
3. 요청 시험: 다음 provider 요청에서 결과 묶음 바로 뒤에 범위 3의 `user` 메시지가 있고 그림 블록이 저장한 바이트의 data URL이다. 그림 넷을 받으면 가장 최근 셋만 붙는다. `attach`가 거짓이면 붙인 메시지가 없다. 저장한 파일을 지우면 `[image unavailable: <참조>]`가 적힌다. run이 끝난 뒤의 messages와 세션 파일에 붙인 메시지가 없다.
4. provider 형식 시험: 범위 3의 요청으로 Anthropic, Chat Completions, Responses, Codex backend 요청 본문을 만들면 그림이 각각 `image` base64 source, `image_url`, `input_image`로 들어 있고, 본문에 `images` key와 `_meta`가 없다.
5. 거절 재시도 시험: 그림이 든 요청을 거절하는 가짜 provider에서 재시도 요청의 붙인 메시지가 `[image: tool-results/…]` 글이 되고 turn이 끝난다.
6. auto mode 시험: 그림 결과가 있는 auto mode run에서 classifier 요청과 도구 결과 검사 요청에 base64와 붙인 메시지가 없다. 도구 결과 검사는 범위 2의 결과 글을 받는다.
7. 승인한 호출 시험: agent loop가 승인 질문에서 승인한 호출의 그림 결과도 같은 결과 글과 `images`로 세션에 들어간다.
8. MCP 시험: 그림 part가 있는 `tools/call` 결과가 범위 4의 배열이 되어 저장 시험과 같은 결과 글이 된다. 그림 part가 없는 결과와 `resources/read` blob은 지금 글과 같다.
9. 기존 시험이 통과한다. 그림 결과의 지금 글(base64 JSON)을 확인하던 기존 시험(예: `tests/it/spec048_auto_mode/result_checks.rs`의 그림 결과 시험)은 범위 2의 결과 글에 맞춘다.
10. 문서 확인: `docs/USAGE.md`와 README가 범위 6과 같다. `Cargo.lock`이 바뀌지 않는다.
11. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다.

## Closure Evidence

1. 저장, 붙이기, MCP 변환, run 설정, 문서의 구현과 시험.
2. 그림 결과가 있는 turn의 세션 tool 메시지와, 그 다음 provider 요청에서 붙인 메시지의 예 하나씩(base64는 줄여서).
3. Cargo 검증 기록과 `Cargo.lock` 확인.

PRD 000은 PRD 010–040 없이 닫힌다.

## 남은 제한

- 그림은 도구 결과 안이 아니라 그 뒤의 `user` 메시지로 간다. 메시지 글이 도구 결과라고 밝히지만, model이 그것을 사용자 쪽 입력으로 받아들일 수 있다.
- `tool-results/`의 그림은 지우지 않아 쌓인다. 같은 그림은 한 파일이다.
- 참조는 그 run의 media 폴더 기준이다. 다른 media 폴더를 쓰는 run이 같은 세션을 이어 읽으면 `[image unavailable]`이 된다.
- 그림 크기를 줄이지 않는다. provider가 크기나 해상도로 거절하면 재시도가 그림을 빼고 보낸다.
- snip은 그림의 token을 세지 않는다. 그림 3개만 붙여 그 차이를 줄인다.
- 그림을 받는다고 판정된 model이라도 Chat Completions 호환 provider가 그림 블록을 거절하면 재시도가 처리한다(DeepSeek은 지금처럼 글로 바뀐다).
- 저장 쪽은 읽기 쪽과 달리 `tool-results` 폴더·파일의 symlink를 따로 막지 않는다. media 폴더 안에 누가 symlink를 두면 새 그림이 그 대상에 쓰이거나, 저장했다고 남긴 참조가 다음 요청에서 `[image unavailable]`이 될 수 있다. 독립 검토 2차가 nice-to-have로 남긴 점이다.

## 구현 메모 및 검증 근거 (2026-11-08)

### 구현

- `crates/shacs-core/src/runtime/tool_images.rs`(새 파일): 범위 2의 저장(`persist`)과 범위 3의 붙이기(`attach`). 붙이기는 요청 본문으로 가는 모든 메시지에서 `images` key를 지운다. 세션에서 읽은 참조는 `tool-results/<64자 소문자 16진수>.<png|jpg|gif|webp>` 꼴이고, symlink가 아닌 보통 파일이며, canonical 경로가 `<media 폴더>/tool-results` 바로 아래일 때만 읽는다. 아니면 `[image unavailable: <참조>]`다.
- `runner.rs`: `AgentRunSpec.tool_images`와 `ToolImageSettings`. `finalize_tool_message`가 맨 앞에서 저장하고, tool event는 저장 뒤의 결과 글로 만든다. `govern_messages_for_model`이 provider context를 넣기 전에 붙인다.
- `tool_execution.rs`: `RuntimeToolMessage.images`(비면 쓰지 않고, 없으면 빈 목록으로 읽는다). `tool_search.rs`는 deferred bridge 결과에 참조를 옮긴다.
- `crates/shacs-session/src/lib.rs`: 세션 기록을 모델용 메시지로 되돌릴 때 tool 메시지의 `images`를 남긴다(다른 role은 버린다). 화면용 기록은 바뀌지 않는다.
- `crates/shacs-core/src/tools/mcp.rs`: 범위 4.
- `crates/shacs-providers/src/clients/openai_compatible/mod.rs`: `build_chat_completions_request`도 메시지의 `images`와 블록의 `_meta`를 지운다(정리하는 client 길은 이미 지웠다).
- 실행 설정: `crates/shacs-cli/src/lib.rs`의 공용 agent loop 설정이 `media/api` 폴더와 기존 `native_image_input_supported`로 채운다. 일반 turn, 승인한 호출, local API·cron·heartbeat가 이 설정을 쓰고, 하위 작업과 workflow 자식은 물려받는다. 위임 작업 관찰은 `AgentRunSpec`을 따로 만들지 않는다. Dream(`memory.rs`)은 media 폴더와 그림을 내는 tool이 없어 `None`이다.
- 문서: `docs/USAGE.md`의 `web_fetch`·MCP 설명, README의 도구 문단.

### 계획 밖에서 바꾼 것

- 하위 작업의 file tool에 물려받은 media 폴더를 준다(`subagent.rs`의 `build_subagent_tool_registry`, 지금까지는 없었다). 범위 5의 `media_root`가 "그 run의 file tool이 쓰는 media 폴더"이므로, 부모의 폴더를 물려받는 하위 작업의 file tool도 같은 폴더를 보게 맞춘 것이다. 그래서 하위 작업도 media 폴더의 첨부·그림 파일을 file tool로 읽을 수 있다.

### 독립 검토

- 1차(deep-high, FAIL): 세션 기록을 되돌릴 때 `images`가 빠져 다음 turn에 그림이 붙지 않음(blocker), 세션의 참조로 저장 폴더 밖 파일을 읽어 provider로 보낼 수 있음(blocker), auto mode 시험이 그림 결과 뒤의 classifier 요청을 보지 않고 provider 형식 시험이 이미 정리된 입력을 줌(should-fix). 세 가지를 고치고 시험을 더했다.
- 2차(deep-high, PASS): 세 지적이 고쳐진 것을 확인하고 전체를 다시 검토했다. 저장 쪽 symlink 막기만 nice-to-have로 남았다(남은 제한).

### 시험

`crates/shacs-core/tests/it/spec049_tool_images.rs`(통합 시험 실행 파일 `it`에 등록):

| 수용 기준 | 시험 |
|---|---|
| 1 | `runner_saves_deduplicated_images_without_base64_in_history_or_events` |
| 2 | `runner_rejects_unkeepable_images_and_preserves_other_results` |
| 3 | `requests_attach_only_latest_three_and_report_missing_files`, `agent_loop_reloads_tool_images_on_the_following_turn` |
| 4 | `runner_saves_deduplicated_images_without_base64_in_history_or_events`(`images`·`_meta`가 남은 입력으로 Anthropic, Responses, Codex, Chat Completions 두 길) |
| 5 | `rejected_tool_images_retry_as_reference_text` |
| 6 | `auto_mode_checks_receive_saved_text_without_image_attachments`(그림 결과 뒤 `exec`의 classifier 요청까지) |
| 7 | `approved_image_call_is_saved_in_the_session`(세션과 승인 뒤 provider 요청) |
| 8 | `mcp_image_content_uses_the_same_runner_storage`, `tools::mcp::tests::tool_images_preserve_content_parts_and_other_mcp_results_keep_their_text` |
| 저장 폴더 경계 | `history_image_references_cannot_use_absolute_paths_or_parent_traversal`, `history_image_references_cannot_follow_file_or_store_directory_symlinks` |

- 수용 기준 9: 바뀐 다섯 package의 시험이 모두 통과했다. 그림 결과의 옛 글을 확인하던 `spec048_auto_mode::result_checks::refusals_empty_results_and_image_data_are_not_sent`는 범위 2의 글에 맞췄다. `shacs-session`의 `session_manager_saves_loads_metadata_and_history`는 tool 메시지의 참조만 남는지 확인하도록 넓혔다.
- 수용 기준 10: README와 `docs/USAGE.md`가 범위 6과 같고, `Cargo.lock`은 바뀌지 않았다.

### Cargo 검증

| 명령 | 결과 |
|---|---|
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` | 통과 |
| `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings` | 통과(기존 `imap-proto v0.10.2` future-compat 안내만) |
| `cargo test ... -p shacs-session -q` | 148 passed / 0 failed |
| `cargo test ... -p shacs-core -q` | 1,971 passed / 0 failed / 5 ignored |
| `cargo test ... -p shacs-providers -q` | 202 passed / 0 failed |
| `cargo test ... -p shacs-cli -q` | 509 passed / 0 failed / 3 ignored |
| `cargo test ... -p shacs-api -q` | 118 passed / 0 failed |
| `git diff --stat crates/Cargo.lock` | 빈 출력 |

개수는 시험이 안에서 띄운 하위 process의 결과를 뺀 package 합계다. 전체 workspace 시험은 돌리지 않았다.

## 종료 확인 (2026-11-08)

수용 기준 1–11을 위 시험과 검증으로 확인했고, 독립 검토 2차가 PASS다. PRD 000은 `Complete (Scoped)`로 닫는다. 전체 출시 seal이 아니다.
