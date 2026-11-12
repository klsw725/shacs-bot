# PRD 000. 대화로 고르기

Status: Complete (Scoped)

Depends on: [049](../../049-container-browser-computer-use/SPEC.md) PRD 000–040의 닫힌 결과(`browser` tool, 검증된 읽기, process 전체 브라우저 자리, 쓰지 않으면 닫히는 시간, tool 결과 그림 저장). 013의 command router, 048의 auto mode 안내(소유 범위 13)와 classifier의 사용자 메시지 입력(소유 범위 9)을 소비한다. 이 스펙의 PRD 010·020과 051의 구현에 의존하지 않는다.

## Goal

어시스턴트가 브라우저로 일하다 사용자가 정할 단계에 오면 진행과 화면을 글로 알리고 요소 이름으로 선택지를 낸 뒤 turn을 끝낸다. 사용자의 다음 메시지가 답이다. 답을 기다리는 동안 브라우저는 정한 시간만큼 열려 있고, 사용자가 청하면 지금 화면을 새로 찍어 대화방에 보낸다. 사용자는 `/browser close`로 바로 닫고 `/browser status`로 상태를 본다.

## 지금 상태

- `browser` tool 설명은 한 문단이고 동작은 `open`, `look`, `click`, `type`, `press`, `scroll`, `back`, `wait`, `close` 아홉 개다(`crates/shacs-core/src/tools/browser_tool.rs`의 `Request`와 schema). 사용자가 정할 단계나 비밀에 대한 안내는 없다.
- 캡처는 `read`가 `tools.browser.screenshots`와 model의 그림 입력 판정으로 정한 때에만 찍고, 결과의 `image_url` part를 `tool_images::persist`가 media 폴더 `tool-results/<sha256>.<확장자>`로 옮긴 뒤 `attach`가 최근 그림을 model 요청에 다시 붙인다(`crates/shacs-core/src/runtime/tool_images.rs`). 사용자가 청할 때 그림만 찍는 길은 없다.
- `BrowserManager::maintenance`는 마지막 사용 뒤 `idleTimeoutSeconds`가 지나면 generation을 닫는다(`crates/shacs-cli/src/browser/browser_manager.rs`). manager는 turn이 언제 시작하고 끝나는지 모른다. `BrowserHost`는 `prepare`, `close_prepared`, `acquire`만 있고(`crates/shacs-core/src/runtime/browser_host.rs`), `AgentLoop`는 host를 들고 있지 않다. host는 CLI의 `with_browser_clients`가 `BrowserTool::new(browser)`에만 넘긴다(`crates/shacs-cli/src/lib.rs`).
- command router의 priority 명령은 글 전체가 `/stop`, `/restart`, `/status`와 같을 때만이다. 그 밖의 명령은 세션의 turn lock을 잡은 뒤 처리한다(`crates/shacs-command/src/lib.rs`의 `dispatch_priority`, `crates/shacs-core/src/runtime/agent_loop.rs`의 `process_message`). `/new`와 `/stop` 처리는 브라우저를 모른다.
- auto mode 안내는 상수 `GUIDANCE` 한 문단이다(`crates/shacs-core/src/runtime/agent_loop_auto_mode.rs`).
- `message` 도구는 `media/<이름>`을 첫 media 폴더 기준으로, 절대 경로는 그대로 푼다(`crates/shacs-core/src/tools/message.rs`의 `parse_media`). CLI는 tool 그림을 `media/api` 아래에 저장하고(`loop_config_for`의 `ToolImageSettings.media_root`), `message`의 첫 media 폴더로는 그 부모인 `media`를 준다(`config.media_roots`). 그래서 `media/tool-results/<파일>`이라는 글은 실제 파일(`media/api/tool-results/<파일>`)과 다른 곳으로 풀린다.
- `look`은 판정 때 만든 digest를 실행 때 다시 확인한다. `CdpDocument::execute_prepared`가 판정 뒤에 온 event를 먼저 읽고, digest와 승인된 주 문서가 지금과 같을 때만 `read_verified(Some(승인된 문서))`로 읽는다. `BrowserSession::read`는 `read_verified(None)`이라 이 확인이 없다(`crates/shacs-cli/src/browser/cdp_document.rs`).
- 외부 channel의 ingress는 `AgentLoop`에 닿기 전에 줄을 세운다. priority가 아닌 메시지는 그 세션이 바쁘면 durable 줄과 `turn_coordinator`에서 기다리고, 실행할 때 세션 자리를 예약한다. priority 명령은 그 세션의 남은 일을 취소시킨 뒤 바로 실행된다(`crates/shacs-cli/src/lib.rs`의 외부 loop, `enqueue_inbound`, `process_external_inbound_with_streaming`).
- manager의 자리는 memory에만 있다. process가 다시 뜨면 새 manager는 빈 자리로 시작하고, 어느 세션이 브라우저를 쓰고 있었는지 그 세션의 다음 호출에 알릴 것이 없다.

## 설계 결정과 근거 (2026-11-11)

1. **답 기다리는 시간을 누가 아는가.** 고른 것: loop가 host에 turn의 시작과 끝을 알리고 manager가 한 generation에 기한 하나만 센다.
    - 다른 길 A: 알림 없이 `idleTimeoutSeconds`를 `replyWaitMinutes`만큼 늘린다. 코드가 가장 적지만 turn이 도는 중에도 30분을 열어 두게 되어 SPEC 소유 범위 2("다음 turn이 시작되면 다시 049의 시간")와 다르다.
    - 다른 길 B: core가 따로 timer를 돌려 host의 닫기를 부른다. 시계가 둘이 되고 049 maintenance와 경합한다.
    - 근거: 049 maintenance는 이미 100 ms마다 한 lock 아래에서 기한을 본다. 기한의 종류만 바꾸면 시험용 시계 하나로 두 시간을 모두 확인할 수 있다.
2. **`capture` 그림을 어디서 저장하는가.** 고른 것: tool이 host에서 PNG를 받아 `tool_images`의 저장 함수로 직접 쓰고 글 결과만 돌려준다.
    - 다른 길: 결과에 `image_url` part를 넣고 `persist`가 "model에 붙이지 않음" 표식을 보고 건너뛴다. 모든 tool이 지나는 공용 경로에 `browser` 전용 분기가 생기고, 표식을 놓치면 그림이 model로 간다.
    - 근거: 그림이 처음부터 message part가 아니면 `RuntimeToolMessage.images`에 들어갈 길이 없다. "model 요청에 붙지 않는다"가 구조로 지켜진다.
3. **`capture`가 판정과 요소 번호를 어떻게 지나는가.** 고른 것: `look`과 같은 준비–실행 길을 그대로 지난다. 판정 때 만든 digest를 실행까지 들고 가 generation lock 아래에서 다시 확인하고, 049의 검증된 읽기를 캡처를 강제해 돌린다. 그래서 새 revision이 생기고 앞선 번호는 못 쓴다. 결과에 "다시 look"을 적는다.
    - 다른 길 A: 판정 뒤 `BrowserSession::read`를 캡처를 강제해 부른다. 코드가 가장 적지만 `read`는 승인된 문서를 보지 않는다(`read_verified(None)`). 판정과 캡처 사이에 페이지가 다른 곳으로 넘어가면 판정한 적 없는 화면이 찍혀 대화방으로 간다.
    - 다른 길 B: snapshot을 건드리지 않는 캡처 전용 transaction을 새로 만든다. 049의 찍기 전후 확인을 복제해야 한다.
    - 근거: SPEC 소유 범위 1의 "판정은 `look`과 같다"는 판정 재료만이 아니라 그 재료가 가리킨 문서에 묶이는 것까지다. 화면을 청한 turn은 그림을 보내고 끝나며, 다음 답에서는 어차피 다시 `look`한다.
4. **`/browser` 명령과 세션의 줄.** 고른 것: `/browser`는 세션 기록을 읽거나 쓰지 않으므로 loop의 turn lock도, 외부 ingress의 세션 줄과 예약도 지나지 않는다. priority 명령으로 만들지는 않는다.
    - 다른 길 A: 다른 prefix 명령처럼 줄을 선다. 그 세션의 turn이 도는 동안에는 `/browser close`가 turn이 끝난 뒤에야 처리되어 "바로 닫는다"를 지키지 못한다. loop에서만 lock을 건너뛰어도 같다. 외부 channel의 메시지는 loop에 닿기 전에 ingress에서 줄을 서기 때문이다.
    - 다른 길 B: priority 명령에 넣는다. 외부 ingress는 priority 명령이 오면 그 세션의 남은 일을 취소한다(`request_external_session_cancellation`). `/browser status`가 진행 중인 일을 끊게 된다.
    - 근거: priority 명령이 turn이 도는 중에 `save_session = false`로 답만 보내는 갈래가 이미 있다(`process_message`의 `AlreadyActive` 처리). 그 갈래에서 취소만 뺀 것이다.
5. **닫힘을 어떻게 알리는가.** 고른 것: manager가 세션별 일회 알림을 두고 그 세션의 다음 `browser` 결과로 한 번 돌려준다. 알림은 memory와 함께 data 폴더의 세션별 표식 파일에도 둔다.
    - 다른 길 A: 닫힐 때 대화방에 글을 보낸다. 사용자가 청하지 않은 메시지가 생기고, SPEC은 "다음 답에서 어시스턴트가 알린다"고 정했다.
    - 다른 길 B: memory에만 둔다. process가 다시 시작되면 알림이 없어 SPEC 소유 범위 2의 "그 세션의 다음 `browser` 호출은 브라우저가 닫혔다를 받는다"를 지키지 못한다.
    - 다른 길 C: 049 상태 파일(`runtime/browser-state.json`)에 세션을 적는다. 그 파일은 049가 owner 검증과 함께 통째로 다시 쓰는 inspect용 snapshot이고, owner가 바뀌면 앞 값을 믿지 않는다(049 PRD 010). 다시 뜬 process가 읽을 값이 아니다.
    - 근거: 표식은 "이 세션이 브라우저를 쓰고 있었다"와 "아직 전하지 못한 알림" 둘만 담으면 된다. 쓰던 process가 살아 있는지는 `flock`으로 안다. process가 죽으면 lock이 풀리므로 죽은 process의 표식을 따로 가려낼 필요가 없다. `fs2`는 이미 쓰는 crate다. 전하지 못한 알림에는 수의 한도를 두지 않는다. 한도를 두고 오래된 것을 버리면 그 세션은 닫혔다는 결과를 받지 못해 SPEC 소유 범위 2를 지키지 못한다. 알림은 세션 하나에 100 bytes가 안 되는 파일 하나라서 한도 없이 둬도 부담이 작다.
6. **닫힘 알림을 언제 지우는가 (2026-11-11 구현 중 고침).** 고른 것: 알림을 보는 것은 지우지 않고, 실제 실행 경로가 알림의 글을 그 호출의 tool 결과로 기록한 뒤 `BrowserHost::acknowledge_closure_notice`로만 지운다.
    - 처음 글: `prepare`가 알림을 돌려주면서 지운다. auto mode는 한 `browser` 호출에 `prepare`를 세 번 부른다(`crates/shacs-core/src/runtime/runner.rs`의 classifier 앞 `tool_context_with_classifier_verdict`와 최종 판정 진단, `tool_execution.rs`의 실행). 모델이 받는 결과는 마지막 것이라, 첫 `prepare`가 알림을 지우면 모델은 알림 대신 `browser_document_changed`나 권한 거부를 받고 Required Contract 6이 깨진다.
    - 다른 길 A: `prepare_browser_action`에 tool call id를 넘겨 같은 호출에는 같은 알림을 돌려준다. `Tool`의 기존 method 모양이 바뀌고, provider가 호출 id를 다시 쓰지 않는다는 데 기댄다.
    - 다른 길 B: auto mode의 `prepare`를 호출당 한 번으로 줄인다. 049가 실행 직전에 일부러 다시 `prepare`해 판정 뒤 바뀐 페이지를 거절하는 것과 부딪친다.
    - 근거: classifier와 진단의 `prepare`가 상태를 바꾸지 않는다는 049의 성질이 그대로 남는다. 지우는 곳이 하나라 mode마다 `prepare` 횟수가 달라도 결과가 같다. token은 지난 알림에 대한 acknowledge가 새 알림을 지우지 못하게 한다.

## Scope

1. **tool 설명:** `browser` tool 설명을 아래 글로 바꾼다. 모든 mode에서 같은 글이다. 앞 다섯 문장은 049 그대로다.

   ```text
   Open and read http(s) pages in the official container. Use element numbers and the latest revision. Click/type/press require browser_act permission and a verified target. Passwords, file upload and downloads are unavailable; use web_fetch for downloads. Canvas controls may be inaccessible. When a step is the user's to decide (a choice among options, a purchase, payment, submission or other commitment they did not ask for, or a page that differs from the request), do not act: tell the user briefly what you did and what the page shows, list the choices by their exact element names from the latest result, ask them to reply with the name, and end your turn. When the reply arrives, look again and click only if an element with that name is on the page now; otherwise describe the changed page. Never ask the user to send passwords, verification codes or card numbers in chat. There is no way to enter them directly yet: say so, list what remains, and stop. Use capture only when the user asks to see the screen; it saves a fresh screenshot and returns a path to send with the message tool. If capture fails, tell the user why and do not send an older image.
   ```

   "There is no way to enter them directly yet: say so, list what remains, and stop." 한 문장은 PRD 010이 `handoff` 안내로 바꾼다. 이 PRD는 그 문장으로 닫힌다.
2. **auto mode 안내:** `GUIDANCE` 뒤에 `\n\n`으로 이어 아래 한 줄을 붙인다. 그 실행의 tool 목록에 `browser`가 있을 때만 붙이고, 없으면 048의 글은 글자 하나 바뀌지 않는다.

   ```text
   In the browser, when a step is the user's to decide, tell them the progress and the choices in text, end the turn, and wait for their reply.
   ```

3. **`capture` 동작:** `Request`와 schema의 `action`에 `capture`를 더한다. 인자는 없다.
    - 판정은 `look`과 같다. `normalize_tool_candidate`의 `open`·`look`·`scroll`·`back`·`wait`·`close` 묶음에 `capture`를 넣는다(`crates/shacs-core/src/runtime/permission_action.rs`). mode별 판정이 같은 묶음을 따로 들고 있으므로 `permission_policy.rs`의 `browser_mode_action`에도 넣는다(2026-11-11 고침). 새 capability나 판정 재료는 없다.
    - 준비: 판정 재료는 `look`처럼 `BrowserHost::prepare(세션, {"action":"capture"})`로 만든다. `CdpDocument::prepare`는 `capture`를 `look`과 같은 관찰 동작으로 묶어 지금 검증된 주 문서로 digest를 만든다(`browser_action_binding::observation`). 그 세션의 generation이 없으면 `ManagedBrowserHost::prepare`가 `capture`에 한해 Scope 6의 알림이 있으면 그것을, 없으면 `browser_not_open`을 돌려준다. 다른 세션의 generation이면 049의 `browser_busy`다. generation은 있는데 검증된 문서가 없으면 049가 `look`에 주는 결과와 같다.
    - 실행: `BrowserHost`에 `capture_prepared(&self, context, digest) -> Result<BrowserCapture, BrowserError>`를 더한다(기본 구현은 `ContextUnavailable`). `BrowserCapture`는 검증된 `BrowserDocument`와 PNG bytes만 담는다. DevTools는 그림을 base64 글로 주는데 shacs-cli에는 base64 crate가 없고 새 crate는 더하지 않으므로, shacs-core의 도우미 `BrowserCapture::from_base64(document, 글)`이 풀어 만든다. 풀지 못하거나 비어 있으면 `browser_capture_failed`다(2026-11-11 고침). `ManagedBrowserHost`는 `acquire`를 거치지 않으므로 브라우저를 새로 띄우지 않는다. 판정과 실행 사이에 generation이 없어졌으면 준비 때와 같은 결과(알림 또는 `browser_not_open`)다. 있으면 `with_generation` 안에서 `CdpDocument::execute_prepared({"action":"capture"}, digest, true, deadline)`을 돌려 `screenshot`만 꺼낸다. `tools.browser.screenshots`와 `native_image_input_supported`는 보지 않는다.
    - 승인된 문서 확인: `execute_prepared`는 `capture`를 `look`과 같은 갈래로 다룬다. 판정 뒤에 온 event를 먼저 읽는다. digest가 지금과 다르거나 승인된 주 문서가 지금 주 문서와 다르면 049의 `browser_stale_revision`을 돌려준다. 이때는 찍지 않고 탭을 닫지 않으며 브라우저는 그대로다. 어시스턴트가 다시 `capture`를 부르면 새 판정을 받는다. 같을 때만 `read_verified(Some(승인된 문서), true, deadline)`으로 찍는다. 판정한 적 없는 문서의 화면은 찍히지 않는다.
    - 찍는 동안: 049의 찍기 직전·직후 주 문서 주소와 loader 확인을 그대로 받는다. 주 문서가 `http:`·`https:`가 아니거나 찍는 사이 문서가 바뀌면 049가 하듯 그 탭을 닫고 049의 오류(`browser_document_changed` 등)를 돌려준다. 그림은 버리고 저장하지 않는다. 캡처가 비어 돌아오면 `browser_capture_failed`다.
    - 저장: `tool_images::keep_image`의 쓰기 부분을 `store(media_root, bytes) -> Result<String, &'static str>`로 떼어 `persist`와 `capture`가 함께 쓴다. `store`는 5 MiB 한도를 보고 bytes의 앞머리로 형식(PNG·JPEG·GIF·WebP)을 가려 확장자를 정한다. 049의 `keep_image`가 하던 그대로이고 아는 형식이 아니면 저장하지 않는다(2026-11-11 고침). `capture`의 그림은 PNG라 이름은 `tool-results/<sha256>.png`다. 5 MiB 한도, 같은 내용이면 다시 쓰지 않는 것까지 049 PRD 000과 같다. `BrowserCallContext`에 `media_root: Option<PathBuf>`를 더해 loop가 `tool_images` 설정의 `media_root`를 넣는다. 없거나 절대 경로가 아니면 `browser_capture_failed`다.
    - 돌려주는 경로: tool은 `store`가 준 이름을 그 `media_root`에 이어 붙인 절대 경로를 돌려준다. `media/…` 꼴은 쓰지 않는다. `message` 도구는 `media/…`를 자기 첫 media 폴더에 이어 붙이는데, CLI에서 그 폴더(`media`)는 그림을 저장한 폴더(`media/api`)와 달라 없는 파일을 가리키게 된다. 절대 경로는 `parse_media`가 그대로 넘긴다. SPEC 소유 범위 1이 적은 두 꼴 가운데 절대 경로만 쓰는 것이다.
    - 결과는 글 part 하나다. `image_url` part를 만들지 않으므로 `RuntimeToolMessage.images`가 비고 model 요청에 그림이 붙지 않는다.

      ```text
      browser_capture: <저장한 파일의 절대 경로>
      이 경로를 그대로 message 도구의 media로 보내세요. 요소 번호를 쓰려면 다시 look하세요.
      ```

    - 대화방으로 보내는 것은 기존 `message` 도구와 그 판정이 한다. 이 PRD는 `message`를 바꾸지 않는다. runtime이 그림을 스스로 보내지 않는다.
4. **새 결과 글:** `BrowserError`에 아래를 더한다. 글은 고정이고 페이지 내용이나 경로를 담지 않는다.

   | 변형 | 글 |
   |---|---|
   | `NotOpen` | `browser_not_open: 열린 브라우저가 없습니다` |
   | `ReplyWaitTimeout` | `browser_reply_wait_timeout: 답을 기다리다 브라우저가 닫혔습니다. 페이지를 다시 여세요` |
   | `Closed` | `browser_closed: 브라우저가 닫혔습니다. 페이지를 다시 여세요` |
   | `CaptureFailed` | `browser_capture_failed: 화면을 찍지 못했습니다` |

5. **답 기다리는 시간:** `tools.browser.replyWaitMinutes`를 더한다. 기본 30, 범위 5–120의 정수다. 범위 밖이면 다른 browser 시간과 같은 config 오류(`tools.browser timeout outside allowed range`)다(`crates/shacs-config/src/browser_config.rs`의 `BrowserConfig`와 `Fields`). onboard의 기본 stub에 더하고 기존 값은 보존한다. 049의 기본값 JSON을 글자 그대로 비교하는 시험(`crates/shacs-config/tests/it/spec049_browser_config.rs`)에는 `"replyWaitMinutes":30`을 더한다. 049 문서는 고치지 않는다(2026-11-11 고침).
    - `BrowserHost`에 `turn_started(&self, session_id)`와 `turn_finished(&self, session_id)`를 더한다(기본 구현은 아무것도 하지 않는다). `AgentLoopConfig`에 `browser_host: Option<Arc<dyn BrowserHost>>`를 더하고 tool에 넘긴 것과 같은 host를 넣는다.
    - host 배선(2026-11-11 고침): 실제 실행의 host는 `crates/shacs-cli/src/spec035_cli_media/tool_registry.rs`의 `production_tool_registry`가 만들어 `BrowserTool`에 넘긴다. `with_browser_clients`는 시험만 쓴다. 그래서 그 host를 `ProductionTooling`에 함께 실어 adapter(`AgentLoopChatCompletionAdapter`)의 새 field `browser_host`에 두고, `loop_config_for`가 `AgentLoopConfig.browser_host`에 넣는다. `with_browser_clients`는 그 field를 자기가 받은 host로 바꾼다. `crates/shacs-cli/src/lib.rs` 시험의 adapter literal에는 `browser_host: None`을 더한다.
    - loop는 그 세션 주인의 turn에서만 알린다. 이 PRD에서 주인의 turn은 사용자가 보낸 메시지로 시작한 turn이다. `process_message`가 받은 inbound 가운데 runtime이 시작한 것(channel이 `system`·`heartbeat`·`cron`·`automation`인 것, 받아들인 알림과 자동화 결과, `goal_continuation`이나 `automation_route` metadata가 달린 것)을 뺀 것의 model turn이다(2026-11-11 독립 검토 뒤 고침. 처음 글은 `system`만 뺐고, 맡긴 일의 실행은 channel `automation`으로 오므로 그 실행이 답 기다리는 시간을 켜거나 사용자 세션의 기다림을 다시 셌다). 시작은 turn guard를 잡은 직후, 끝은 그 turn이 끝나는 모든 길(정상 끝, 오류, 취소, `ask_user` 멈춤)이며 guard 하나의 drop으로 알린다. 명령 처리, subagent, 맡긴 일, 알림으로 시작한 실행은 알리지 않으므로 049의 시간을 그대로 쓴다.
    - 주인의 turn인지는 한 함수(`browser_owner_turn(inbound)`)가 정한다. 그 함수는 045의 사용자 turn 구분(`crates/shacs-core/src/runtime/agent_loop_method.rs`의 `user_turn`)을 그대로 쓴다. PRD 010은 여기에 runtime이 인증한 `handoff` 이어 가기 inbound 하나를 더한다(PRD 010 Scope 9). 그 밖에 runtime이 시작한 inbound는 주인의 turn이 아니다.
    - manager의 `Generation`에 `waiting_since: Option<u64>`를 더한다. `turn_finished`가 온 세션이 그 generation의 세션이면 `waiting_since = now`다. `turn_started`가 오면 `waiting_since = None`, `last_used = now`다. 다른 세션의 알림은 무시한다.
    - `maintenance`는 `waiting_since`가 있으면 `now - waiting_since >= replyWaitMinutes × 60초`일 때, 없으면 049대로 `now - last_used >= idleTimeoutSeconds`일 때 닫는다. 시계는 049의 `BrowserClock`이다. 기다리는 동안 `idleTimeoutSeconds`는 세지 않는다.
    - 답 기다리는 시간이 지나 닫히면 상태 사유는 `browser_reply_wait_timeout`이고 그 세션에 같은 알림을 둔다(Scope 6).
    - 기다리는 동안 generation의 세션은 바뀌지 않는다. 다른 세션의 `browser` 호출은 049의 `browser_busy`다.
6. **닫힘 알림:** manager가 세션 key → `BrowserError` 한 개의 표를 memory에 두고, 같은 내용을 세션별 표식 파일에도 둔다. 전하지 못한 알림은 그 세션이 받아 갈 때까지 둔다. memory의 표와 표식 파일 어느 쪽에도 수의 한도를 두지 않고, 수가 많다거나 오래됐다는 까닭으로 전하지 못한 알림을 버리지 않는다. 알림이 없어지는 때는 아래 "알림을 지울 때" 하나뿐이다. 다시 시작한 뒤에는 표식 파일이 기준이다. memory의 표는 같은 process 안에서 파일을 다시 읽지 않기 위한 사본이고, 표에 없으면 늘 파일을 본다.
    - 알림을 두는 때: 답 기다리는 시간이 지나 닫힘(`ReplyWaitTimeout`), Scope 7·8의 닫기와 process 종료 정리(`Closed`).
    - 그 세션의 다음 `browser` 호출은 동작을 실행하지 않고 그 글을 결과로 받는다. 받은 것이 실행 경로에서 확인되면 알림을 지우고 그 뒤 호출은 평소대로다. 아래 세 항목은 2026-11-11 구현 중 고침이다(설계 결정 6).
    - 알림을 보일 때: `prepare`, `acquire`, `capture_prepared`, `close_prepared`는 그 세션에 알림이 있으면 동작하지 않고 그 `BrowserError`를 돌려준다. 어느 것도 알림을 지우지 않으므로 지우기 전에는 몇 번을 불러도 같은 알림이다. 알림마다 manager가 만든 불투명한 token이 붙는다. `BrowserHost::closure_notice(&self, session_id) -> Option<BrowserClosureNotice>`(기본 구현은 `None`)가 그 세션의 알림(`error`)과 `token`을 지우지 않고 돌려준다. token은 세션 key나 페이지 내용을 담지 않는다.
    - 알림을 지울 때: `BrowserHost::acknowledge_closure_notice(&self, session_id, token)`(기본 구현은 아무것도 하지 않는다)만 지운다. token이 지금 그 세션의 알림 것과 같으면 memory의 항목과 표식 파일을 함께 지운다. 다르거나 모르는 token이면 아무 일도 하지 않는다. 파일을 지우지 못하면 그 process 안에서는 전한 것으로 적어 두고 같은 알림을 다시 돌려주지 않는다.
    - 누가 지우는가: 실제 실행 경로뿐이다. `crates/shacs-core/src/runtime/tool_execution.rs`의 `execute_tool_calls_with_mode`가 그 호출의 판정 재료를 만들 때 `prepare`가 알림으로 거절하면, 그 글을 그 호출의 tool 결과로 기록한 뒤 `closure_notice`로 token을 받아 기록한 글이 그 알림과 같을 때 `acknowledge_closure_notice`를 부른다. 기본 mode는 이 경로로 바로 오고, auto mode는 `runner.rs`의 `direct_runtime_report_with_classifier`가 `execute_tool_calls`로 이 경로를 부른다. `prepare`를 지난 뒤 실행 중에 host가 알림을 돌려준 때에는 `BrowserTool`이 그 글을 결과로 돌려주면서 같은 방법으로 부른다. auto mode가 한 호출에 더 부르는 두 `prepare`(classifier 앞의 `tool_context_with_classifier_verdict`, 최종 판정 진단)는 알림을 보기만 하고 지우지 않는다. core가 host에 닿도록 `Tool`에 `browser_host(&self) -> Option<&dyn BrowserHost>`(기본 구현은 `None`)을 더하고 `BrowserTool`만 자기 host를 돌려준다. `prepare_browser_action`의 모양은 바꾸지 않는다.
    - 지우지 않는 때: 그 호출의 tool 결과가 알림의 글이 아니면 알림은 다음 호출까지 남는다. 인자가 틀려 `prepare`에 닿기 전에 거절된 호출, 앞선 호출이 사용자 승인이나 `ask_user`에서 멈춰 건너뛴 호출, turn이 끊겨 실행에 이르지 못한 호출이 그렇다.
    - 먼저 보는 것: 그 세션에 알림이 있으면 다른 세션이 브라우저를 쓰는 중이어도 049의 `browser_busy`보다 알림을 먼저 돌려준다. 자리 lock을 다른 동작이 쥐고 있을 때는 memory에 있는 알림만 먼저 돌려줄 수 있다. 표식 파일에만 있는 알림(다시 시작한 뒤)은 그 호출에는 `browser_busy`가 가고 다음 호출에 간다(2026-11-11 고침).
    - `BrowserHost`에 더하는 것 모음: 이 PRD는 `capture_prepared`(Scope 3), `turn_started`·`turn_finished`(Scope 5), `closure_notice`·`acknowledge_closure_notice`(Scope 6), `close_for_session`(Scope 7), `close_by_user`·`describe`(Scope 8)를 더한다. 모두 기본 구현이 있어 049의 host 구현은 고치지 않아도 된다.
    - 상태 폴더가 없는 manager: 049 snapshot을 쓸 곳(data 폴더) 없이 만든 manager는 표식 파일을 쓰지 않고 memory의 알림만 둔다. 실제 실행의 manager(`process_browser_host`)는 늘 data 폴더를 받는다(2026-11-11 고침).
    - 표식 파일: `<data 폴더>/runtime/browser-sessions/<hex>.json`이다. `<hex>`는 `owner mode + "\n" + 세션 key`의 SHA-256이다. owner mode는 049가 화면 보기 port를 고를 때 쓰는 값(`gateway`, `api`)이고 없으면 `local`이다. 폴더는 0700, 파일은 0600이고 `O_NOFOLLOW`로 연다. 내용은 `state`(`open`, `reply_wait_timeout`, `closed`)와 쓴 시각뿐이다. 세션 key 원문, 페이지 주소, 페이지 내용은 넣지 않는다. 100 bytes가 안 되는 글을 그 자리에서 고쳐 쓴다. 이름 바꾸기로 갈아 끼우면 lock이 옛 파일에 남기 때문이다.
    - generation이 어떤 세션의 것이 될 때 manager는 그 세션의 표식 파일에 배타 `flock`(`fs2::FileExt::try_lock_exclusive`)을 걸고 `open`을 적는다. lock은 generation이 닫힐 때까지 쥔다. 폴더나 파일을 만들거나 쓰지 못하면 브라우저를 띄우지 않고 049의 `browser_state_unavailable`을 돌려준다. 049의 "상태를 기록하지 못하는 브라우저는 두지 않는다"와 같은 규칙이다. 다른 process가 이미 lock을 쥐고 있을 때만 표식 없이 띄운다(남은 제한).
    - 닫을 때: 알림을 두는 닫기는 `state`를 그 알림으로 고쳐 쓴 뒤 lock을 놓는다. 알림을 두지 않는 닫기(어시스턴트의 `close`, 049의 쓰지 않아 닫힘, 049 오류로 닫힘)는 파일을 지운 뒤 lock을 놓는다. 이 닫기들의 결과는 049 그대로다.
    - 읽을 때: 그 세션의 generation이 없는 채로 `browser` 호출이 오면 memory의 알림을 먼저 본다. 없으면 표식 파일을 열어 배타 lock을 잡아 본다. 파일에서 읽은 알림은 token을 붙여 memory에 옮겨 두고 lock을 놓는다. 파일은 지울 때까지 그대로 둔다.

      | 본 것 | 결과 |
      |---|---|
      | 파일이 없음 | 알림 없음. 평소대로 한다 |
      | lock을 잡았고 `reply_wait_timeout` | 알림은 `browser_reply_wait_timeout`이다 |
      | lock을 잡았고 `closed` | 알림은 `browser_closed`다 |
      | lock을 잡았고 `open`(쓰던 process가 닫지 못하고 끝남)이거나 읽을 수 없는 내용 | 알림은 `browser_closed`다 |
      | lock을 잡지 못함(다른 process가 같은 표식으로 브라우저를 쓰는 중) | 알림 없음. 파일을 건드리지 않는다 |

    - 지울 때의 파일: `acknowledge_closure_notice`는 표식 파일의 배타 lock을 잡은 뒤에 지운다. 잡지 못하면(그 사이 다른 process가 같은 표식으로 브라우저를 띄움) 파일을 건드리지 않고 memory의 항목만 지운다.
    - process가 다시 시작된 뒤: 끝나기 전에 닫기 경로를 탔으면 `closed`가, 타지 못하고 죽었으면 lock이 풀린 `open`이 남아 있다. 어느 쪽이든 새 process에서 그 세션의 다음 `browser` 호출이 `browser_closed`를 받고, 실행 경로가 그것을 tool 결과로 기록하면 지워져 한 번으로 끝난다(SPEC 소유 범위 2). 답 기다리다 닫힌 뒤 전하지 못한 알림은 `browser_reply_wait_timeout` 그대로 간다. 브라우저를 쓰지 않던 세션은 받지 않는다. 기다리던 페이지는 되살리지 않는다.
7. **`/new`와 `/stop`:** `BrowserHost`에 `close_for_session(&self, session_id) -> bool`을 더한다(기본 구현은 `false`). `LoopCommand::New`와 `LoopCommand::Stop` 처리가 그 세션 key로 부른다.
    - 그 세션의 generation이면 049의 닫기 경로로 닫고 `Closed` 알림을 둔다. 번호는 generation과 함께 없어진다. 다른 세션의 것이거나 브라우저가 없으면 아무것도 하지 않는다.
    - 자리 lock을 잡지 못하면(동작 실행 중, 브라우저를 띄우는 중, 정리하는 중) 닫기를 예약할지를 자리 lock 밖의 작은 lock(`deferred`) 하나에서 정한다(2026-11-11 두 번째 독립 검토 뒤 고침). 그 lock 안에 지금 닫기를 받을 수 있는 generation의 번호와 세션(`Live`)을 둔다. 브라우저를 띄우기 시작할 때 넣고(`admit`), generation을 끝내는 모든 길이 지나는 함수 하나(`end`)가 같은 lock으로 꺼낸다(`retire`). 닫기 요청은 그 lock 안에서 `Live`가 있고 세션이 맞을 때만 그 generation에 예약을 남기고 `Scheduled`를 답한다. `Live`가 없거나(끝내기가 이미 시작됨, 브라우저 없음) 다른 세션의 것이면 아무것도 남기지 않고 `NothingOpen`(`close_for_session`은 `false`)을 답한다. 예약은 그 동작을 끝내는 `with_generation`의 끝과 `maintenance`가 보고 닫는다. `/stop`은 기다리지 않는다.
    - 받은 예약은 generation이 어떻게 끝나든 지켜진다. `end`가 `Live`를 꺼낼 때 예약도 같은 lock 안에서 함께 꺼낸다. 예약이 있으면 끝난 까닭(브라우저를 닫는 오류 `PageTimeout`·`DocumentChanged`, 어시스턴트의 `close`, 쓰지 않아 닫힘, 답 기다리다 닫힘)과 상관없이 그 세션에 `Closed` 알림과 `closed` 표식을 남긴다. 띄우는 중에 받은 예약은 띄우기가 성공하면 다음 `settle`이 닫고, 실패하면 띄우기 정리(`Starting`)가 같은 알림과 표식을 남긴다. 꺼낸 뒤에 온 닫기는 `NothingOpen`이다. 그래서 "닫겠습니다"라고 답하고 알림을 남기지 않는 경우가 없다. 예약은 generation 번호에 묶여 있어 그 뒤에 뜬 브라우저를 닫지 않는다. 첫 독립 검토 뒤의 고침(오류 정리가 예약을 먼저 보는 것)은 본 뒤 generation을 버리기까지의 사이에 온 닫기를 놓쳤고, 이 고침으로 바뀌었다.
8. **`/browser close`와 `/browser status`:** `CommandId::Browser`(canonical `/browser`)를 prefix 명령으로 더하고 `LoopCommand::Browser(BrowserCommandArgs)`로 푼다. `BrowserCommandArgs`는 이 PRD에서 `Close`, `Status`, `Invalid`다. `/help` 목록에 한 줄을 더한다.
    - `process_message`는 `LoopCommand::Browser`를 priority 자리(`acquire_priority`)와 turn lock을 잡기 전에 처리한다. 세션을 읽지도 저장하지도 않고 답만 보낸다. 모델 turn을 만들지 않으므로 048 classifier 입력에 들어가지 않는다.
    - 외부·durable ingress: `CommandRouter`에 `is_session_free(text)`를 더한다. 앞뒤 공백을 뗀 글이 `/browser`이거나 `/browser `로 시작하면 참이다(대소문자 구분 없음). `is_priority`는 바꾸지 않는다. CLI의 ingress는 이 값이 참인 메시지를 아래처럼 다룬다(`crates/shacs-cli/src/lib.rs`).
        - 처음 받을 때(외부 loop의 durable 넣기와 `enqueue_inbound`): durable 줄에 넣은 뒤 priority 명령처럼 그 항목을 바로 내보낸다(`dispatch_priority`). 그 세션이 바쁜지는 보지 않는다. priority 명령과 달리 `cancel_external_active_session`과 `request_external_session_cancellation`은 부르지 않는다. 그 세션에서 도는 turn과 줄에 선 일은 그대로다.
        - 내보낸 뒤(외부 loop의 turn 시작): `turn_coordinator`에 넣지 않고 priority 명령처럼 바로 `spawn_external_agent_turn`으로 돌린다. typing 표시는 켜지 않는다.
        - 실행(`process_external_inbound_with_streaming`의 streaming 갈래와 아닌 갈래 모두): 세션 자리 예약(`session_turn_lock.reserve`)을 만들지 않는다.
        - 끝난 뒤(`handle_external_agent_turn_result`): priority 명령처럼 그 세션의 다음 줄을 꺼내지 않고, 그 세션의 도는 turn 표시를 건드리지 않는다.
        - 외부 loop와 자동화 pass(2026-11-11 구현 중 고침, 독립 검토 뒤 다시 고침): 외부 loop는 한 바퀴마다 같은 thread에서 자동화 pass(`automation_worker::process_due_automation`: 036 관찰과 042 웹 감시, 검증 요청, 자동화 일의 실행)를 돌렸다. 그 pass는 세션 mutation lock(`SessionMutationGuard`)을 기다리고 자동화 일을 그 자리에서 실행한다. 도는 turn은 끝날 때까지 그 lock을 쥔다. 그래서 turn이 돌거나 자동화 일이 도는 동안 loop가 멈춰, `/browser`도 priority 명령(`/stop`)도 받지 못했다. 고침: `dispatch_due_external_work`는 inbound만 내보내고 자동화 pass를 돌리지 않는다. 그 pass는 gateway가 외부 channel보다 먼저 띄우는 worker thread(`start_surface_approval_worker`)가 앞선 스펙부터 50ms마다 돌리던 것 하나만 남는다. worker는 자기 dispatcher를 쓰고 pass의 내용과 순서, lock을 기다리는 방식은 그대로다. 외부 loop는 pass가 실패하면 멈췄으므로 그 성질도 지킨다. worker thread가 오류나 panic으로 끝나면 `SurfaceApprovalWorker::failure_signal`이 서고, gateway의 종료 대기(`wait_for_ctrl_c_or_runtime_request`)가 외부 channel 건강 신호와 함께 그것을 보아 runtime을 멈추며 `stop`이 그 오류나 panic을 돌려준다. 첫 고침에서 더한 `DurableWorkDispatcher::defer_sessions`와 036·042 pass의 건너뛰기는 부르는 곳이 없어져 지웠다. 그 dispatch 파일들에서 앞선 스펙과 다른 것은 실행할 일을 고르는 목록 한 줄씩뿐이다(아래 다섯 번째 검토 뒤 고침).
        - gateway의 durable 임대 주인(2026-11-11 두 번째 독립 검토 뒤 고침): worker와 외부 ingress dispatcher는 같은 gateway process의 것인데 임대 주인 값이 달랐다(worker는 runtime `owner_id`, ingress는 따로 만든 `runtime-<pid>-<시각>`). 임대는 30초이고 갱신하지 않는다. 복구 판단(`evaluate_durable_work_recovery_for_owner`)은 다른 주인의 기한 지난 임대를 죽은 process의 것으로 보아 쓰기를 막는다. 그래서 자동화 일이 30초를 넘겨 돌면(기본 한도 120초) ingress loop가 오류로 끝나 `/browser`와 `/stop`을 받지 못했다. 거꾸로 turn이 30초를 넘기면 worker가 오류로 끝나 gateway가 멈췄다. 고침: `GatewayDurableWork`가 gateway의 data 폴더와 runtime `owner_id`를 들고 worker(`start_worker`)와 ingress dispatcher(`open_ingress`)를 둘 다 그 주인으로 연다. `ExternalChannelSupervisor::start`는 주인 값을 만들지 않고 이것을 받는다. 같은 주인의 기한 지난 임대는 복구 대상이 아니므로 둘이 서로를 막지 않는다. 죽은 process의 임대는 다음에 뜨는 process의 주인 값이 달라 전과 같이 복구된다. `shacs-session`의 복구 판단은 바꾸지 않았다.
        - 주인 값이 같아졌으므로, 종료 때 임대를 줄로 돌려놓는 `requeue_owned_external_work`는 일의 종류가 inbound turn(`INBOUND_WORK_KIND`)인 임대만 돌려놓는다. ingress dispatcher는 그 종류만 임대한다. worker가 임대한 자동화 일과 surface 승인은 건드리지 않는다. gateway 종료 순서는 그대로 외부 channel(처리 loop) 먼저, worker 다음이다. 처리 loop의 종료는 앞선 스펙대로 바쁜 세션의 취소를 청한다. 그 세션에서 돌던 자동화 일은 취소를 받고, worker가 멈출 때 그 끝을 한 번 기록한다(`Failed`, `automation:cancelled`). 줄로 돌아가지 않고 두 번 돌지 않는다.
        - worker와 외부 channel 처리 thread의 끝 신호(2026-11-11 세 번째 독립 검토 뒤 고침): 실패 신호는 pass가 오류를 돌려줄 때만 섰다. 자동화 일이 도는 중에 panic이 나면 thread는 끝나는데 신호가 서지 않았다. gateway의 종료 대기는 신호만 보고, thread를 join하는 것은 멈추기로 한 뒤다. 그 일은 실행 직전에 임대되므로 `Leased`로 남고, 임대 주인을 하나로 한 뒤로는 gateway가 사는 동안 복구되지 않는다. 그래서 gateway와 받는 loop는 살아 있는데 자동화 일, 관찰, 웹 감시, surface 승인이 gateway를 다시 띄울 때까지 멈췄다. 고침: thread가 도는 동안 쥐는 `ThreadEndSignal`이 버려질 때 신호를 세운다. worker는 pass가 정상으로 끝났을 때만 그것을 끈다. 그래서 오류와 panic이 둘 다 `failure_signal`을 세운다. `catch_unwind`는 쓰지 않는다. 외부 channel의 처리 thread(`ExternalChannelSupervisor::spawn_processor`)도 같은 것을 쥐고, 끄지 않는다. 처리 loop가 건강 신호를 세우지 않고 오류로 돌아가거나 panic해도 신호가 선다. 정상 종료에서는 전부터 그 신호가 섰으므로 달라지지 않는다. 신호를 본 gateway는 전과 같이 멈추고(`StartupFailed`), `stop`이 그 오류나 panic을 돌려준다. 남은 `Leased` 임대는 다음에 뜨는 process가 복구한다.
        - 자동화 일과 세션의 turn 자리(2026-11-11 네 번째 독립 검토 뒤 고침): worker thread와 외부 ingress는 같은 세션에서 turn을 시작한다. turn 자리(`SessionTurnLock::acquire`)는 기다리지 않고, 먼저 잡은 쪽만 돈다. 진 쪽이 ingress의 turn이면 `session_busy`로 줄에 돌아가 다시 돌았다. 진 쪽이 worker의 자동화 일이면 이미 임대된 뒤라 `Failed`(`automation:agent:session_busy`)로 끝나고 다시 돌지 않았다. 자동화 일은 임대되면 한 번만 돌기 때문이다. 같은 세션에 자동화 일 둘이 서 있으면 첫 일이 끝난 뒤 그 결과를 알리는 turn(`automation_route: notify`)을 ingress가 시작하는데, 그것이 둘째 일의 실행 시작과 자리 잡기 사이에 끼면 둘째 일이 그렇게 실패했다(수용 조건 17이 가끔 실패한 까닭). 앞선 스펙의 코드에서도 같았다. 고침: worker는 자동화 일을 임대하기 전에 그 세션의 turn 자리를 맡는다(`SessionTurnLock::try_reserve`, `AutomationOwnerAdapters::claim_session_turn`). 그 세션에 도는 turn이나 맡아 둔 자리가 있으면 맡지 못하고, 그 일은 임대하지 않은 채 줄에 남아 다음 pass(50ms 뒤)에 다시 본다. 다른 세션의 일은 그 pass에서 그대로 돈다. 맡은 자리는 worker thread의 것이라 그 일의 turn만 쓸 수 있다. `reserve`는 다른 thread가 맡아 둔 자리를 밀어내지 않는다. 그래서 그 사이에 온 ingress의 turn이 `session_busy`를 받아 전부터 있던 길로 줄에 돌아간다. 맡아 둔 자리에 `/stop`이나 종료 취소가 닿으면 그 일의 turn은 시작하지 않고 끝나고, 그 일은 `Failed`(`automation:cancelled`)로 기록된다. 임대한 뒤에 기다리거나 다시 줄에 넣는 길은 고르지 않았다. 앞의 것은 그 일의 실행 기한을 기다림에 쓰고, 뒤의 것은 임대된 일을 두 번 돌리지 않는다는 규칙을 깬다. `automation_dispatch.rs`와 cli `lib.rs`는 이 고침에서 바꾸지 않았다.
        - 맡아 둔 자리의 주인, 실행 후보 목록, 같은 세션의 순서(2026-11-11 다섯 번째 독립 검토 뒤 고침): 네 번째 고침이 worker의 자리 맡기를 더하면서 앞선 코드와 맞물려 네 가지가 틀어졌다. (1) 맡아 둔 자리에 취소가 닿은 뒤 그 세션에 들어온 다른 thread의 `acquire`가 주인보다 취소를 먼저 보아, 남의 자리를 지우고 `Cancelled`를 받았다. 그러면 worker의 `acquire`는 빈 세션을 보고 취소된 자동화 일을 돌렸다. (2) priority 명령의 `acquire_priority`는 맡아 둔 자리를 보지 않고 세션을 잡았다. worker가 자리를 맡은 뒤 `/status` 같은 명령이 세션을 잡으면 이미 임대된 일이 다시 `session_busy`로 실패했다. (3) 바쁜 세션의 자동화 일이 줄에 남게 되자, 기한이 된 일 목록(`DurableWorkAdmission.due_work_ids`, 가장 오래된 256개)을 그 일들이 채우고 빠지지 않았다. dispatcher는 그 목록에서 고르므로 257번째부터의 일(다른 세션의 자동화 일, inbound, 036 관찰, 042 웹 감시와 기한, surface 승인)이 골라지지 않았다. 열린 일은 1,024개까지 허용된다. (4) 한 pass에서 A가 세션이 바빠 남겨진 뒤 B를 보기 전에 그 세션이 비면, 같은 세션의 B가 A보다 먼저 돌았다. 고침은 넷이다. (1)·(2) 맡아 둔 자리와 그 취소는 그 자리를 묶은 thread만 쓴다. `SessionTurnLock`의 자리 잡기는 취소보다 주인을 먼저 본다. 주인이 아닌 호출은 `acquire`든 `acquire_priority`든 `AlreadyActive`를 받고 자리는 그대로 남는다. 아직 묶지 않은 자리도 같다. 취소된 자리도 묶을 수 있다(`bind_to_current_thread`). 그래서 REPL처럼 묶기 전에 `/stop`을 받은 turn은 전과 같이 묶은 뒤 `acquire`에서 `Cancelled`를 받는다. `AlreadyActive`를 받은 priority 명령은 도는 turn이 있을 때와 같은 `AgentLoop`의 길로 답한다(세션 기록을 쓰지 않는다). `/stop`은 그 앞에서 전과 같이 맡아 둔 자리를 취소하고, 그 취소는 자리의 주인이 받는다. (3) `shacs-session`의 admission에 실행 후보 목록 `runnable_work_ids`(기한이 된 일 전부, 줄에 든 순서)를 더했다. dispatcher 여섯 곳(inbound, 자동화, 036 관찰, 042 웹 감시, 042 기한, surface 승인)과 worker의 pass는 이 목록에서 고른다. `due_work_ids`는 보고용으로 256개 그대로이고 JSON 모양도 그대로다. `runnable_work_ids`는 직렬화하지 않는다. (4) worker의 pass는 자리 맡기에 실패한 세션을 그 pass 동안 기억하고(`waiting_sessions`), 그 세션의 뒤 일은 다음 pass로 넘긴다. 다른 세션의 일은 그대로 돈다. 검토가 견주어 보라고 한 다른 길(`reserve`와 `try_reserve`를 앞선 뜻으로 되돌리고, worker가 자리를 못 잡으면 시도 수를 쓰지 않고 임대를 줄로 돌려놓기)은 고르지 않았다. 까닭은 SPEC.md의 2026-11-11 기록에 있다.
    - 그래서 `/browser` 명령은 세션의 기록, 예약, 줄, 취소 상태 어느 것도 바꾸지 않고, 그 세션의 뒤 메시지 순서도 그대로다. bus가 없는 일회 CLI는 loop의 갈래만 지난다.
    - `/browser close`는 어느 대화방에서 보내든 process의 브라우저를 닫는다. `BrowserHost::close_by_user(&self) -> BrowserCloseOutcome`이 Scope 7과 같은 닫기·예약을 하고, generation의 세션에 `Closed` 알림을 둔다. `BrowserCloseOutcome`은 `Closed`, `Scheduled`, `NothingOpen`이고 기본 구현은 `NothingOpen`이다(2026-11-11 고침). 답은 그 순서대로 아래 셋 중 하나다.

      ```text
      브라우저를 닫았습니다.
      실행 중인 동작이 끝나면 브라우저를 닫습니다.
      열린 브라우저가 없습니다.
      ```

    - `/browser status`는 `BrowserHost::describe(&self) -> BrowserDescription`이 준 값을 아래 줄로 보인다. `BrowserDescription`은 `state`(`BrowserDescriptionState`: `Open`, `Closed`, `Disabled`, `Unavailable`), `session: Option<String>`, `reply_wait_remaining_minutes: Option<u64>`(올림한 분)이고 기본 구현은 `Unavailable`에 나머지는 `None`이다(2026-11-11 고침). 이 PRD는 세 줄만 소유한다. 조작 상태·넘겨받은 곳·한도·화면 보기 주소는 PRD 010이 더한다.

      ```text
      브라우저: 열림 | 닫힘 | 꺼짐 | 쓸 수 없음
      쓰는 세션: <세션 key> | 없음
      답 기다리기: 남은 <n>분 | 아님
      ```

    - 그 밖의 인자는 `사용법: /browser status | close`를 답한다.
    - 답은 한국어 고정 글이다. 다른 built-in 명령의 답은 영어지만, 브라우저 표면의 글은 049가 `browser` 결과를 한국어로 정했고 PRD 010의 `handoff` 질문도 한국어라 그쪽에 맞춘다.
9. **inspect와 diagnostics:** `BrowserStatus`에 `reply_wait_minutes`(설정값)와 `reply_wait_deadline_ms: Option<u64>`(기다리는 중일 때만)를 더해 049 snapshot에 함께 쓴다. `runtime inspect`의 글·JSON과 `diagnostics_snapshot_from_runtime_inspect`는 기다리는 중인지와 남은 시간을 이 두 JSON field(`replyWaitMinutes`, `replyWaitDeadlineMs`)로만 보인다. inspect의 글도 049의 `Browser:` 줄에 `BrowserStatus`의 JSON을 싣기 때문에 줄을 따로 더하지 않는다. 남은 시간은 `replyWaitDeadlineMs`에서 지금 시각을 뺀 값이다(2026-11-11 고침). owner 검증에 실패한 snapshot은 049처럼 live 값을 주장하지 않는다. 세션 key나 페이지 주소는 더하지 않는다.
10. **문서:** `docs/USAGE.md`와 README의 브라우저 설명에 대화로 고르기, 답에 요소 이름을 쓰라는 것, `replyWaitMinutes`, 화면을 청하면 그림이 channel 회사의 서버로도 간다는 것, `/browser close`·`/browser status`, 비밀을 직접 입력할 방법은 PRD 010이 닫힌 뒤에 생긴다는 것을 적는다.
11. **실제 Docker 확인:** SPEC 수용 조건 5에 이 PRD의 몫은 없다. 이 PRD가 더하는 것은 049가 실제 Docker로 확인한 조각(검증된 읽기와 `Page.captureScreenshot`, `tool-results/` 저장, 브라우저 닫기)을 다시 엮은 것이라 가짜 DevTools와 시험용 시계로 다 볼 수 있다. opt-in 스크립트를 따로 만들지 않는다. PRD 010의 `scripts/spec050-takeover-check.sh`가 `/browser status`와 `/browser close`를 실제 container에서 한 번씩 부른다(PRD 010 Scope 17). 그 실행은 사용자 승인 뒤에만 하며 이 PRD의 종료 조건이 아니다.

## Non Scope

- `handoff`, `user` 조작 상태, 화면 보기 입력, `/browser takeover`·`/browser return`, `tools.browser.view.url`(PRD 010).
- 로그인 유지와 프로필(PRD 020).
- 어시스턴트가 언제 멈추고 물을지를 runtime이 강제하는 것. 안내로만 이끈다.
- `message` 도구, 049의 `look`·`click` 판정, `[Browser Commitment]` 규칙의 변경.
- 닫힘을 대화방에 스스로 알리는 메시지.

## Parent Requirement Mapping

- [050 SPEC](../SPEC.md) 소유 범위 1(비밀 단계는 "직접 입력할 방법이 아직 없다"로 끝내기까지), 2, 5 중 `/browser close`와 쓰는 세션·답 기다리기만 보이는 `/browser status`, 9 중 답 기다리기를 소유한다. 수용 조건 1과 6(자기 변경)을 소유한다.
- SPEC "앞선 스펙에서 바꾸는 것" 중 049 tool 설명 행, `capture` 행, 쓰지 않으면 닫히는 시간 행의 답 기다리기 몫, 048 auto mode 안내 행의 첫 줄, 그리고 이 PRD의 결정으로 더한 한 행(012의 "prefix 명령은 정상 turn 경계를 따른다")을 구현한다. 같은 표의 `handoff`·화면 보기·판정 예외 행과 안내 둘째 줄은 PRD 010, 프로필·비밀 파일 행은 PRD 020 소유다.
- SPEC 소유 범위 2의 "process가 다시 시작되면 그 세션의 다음 `browser` 호출은 브라우저가 닫혔다를 받는다"는 Scope 6의 표식 파일로 채운다.
- 049 PRD 000의 그림 저장, PRD 010의 process 전체 자리와 닫기 경로, PRD 020의 검증된 읽기를 소비한다. 049 문서는 고치지 않는다.

## Required Contract

1. 사용자가 정할 단계의 답은 다음 사용자 메시지다. runtime은 그 답을 도구 결과로 바꾸지 않는다.
2. `capture`는 `screenshots` 설정과 model의 그림 입력 여부와 무관하게 지금 화면만 찍는다. 판정한 문서가 실행 때의 주 문서와 다르면 찍지 않고, 찍는 동안의 확인에 실패하면 경로를 돌려주지 않고 탭을 닫는다. 그림은 model 요청에 붙지 않는다.
3. 그림은 `message` 도구가 부를 때만 대화방으로 간다. `capture`가 돌려준 경로는 `message` 도구가 그대로 여는 실제 파일이다.
4. 사용자 메시지로 시작한 turn이 브라우저를 연 채 끝나면 `replyWaitMinutes`만 세고, 그 세션의 다음 turn이 시작되면 `idleTimeoutSeconds`로 돌아간다.
5. 답 기다리는 동안에도 브라우저는 그 세션의 것이다.
6. 그 세션의 `/new`·`/stop`과 어느 대화방의 `/browser close`는 브라우저를 닫고, 그 세션의 다음 `browser` 호출이 닫혔다는 결과를 한 번 받는다. process가 다시 시작된 뒤에도 브라우저를 쓰고 있던 세션의 다음 호출이 같은 결과를 한 번 받는다. 전하지 못한 닫힘 알림은 그런 세션이 몇이든 그 세션이 받을 때까지 남는다. 알림을 보는 것(`prepare`, `acquire`, `capture_prepared`, `close_prepared`, `closure_notice`)은 알림을 지우지 않는다. 알림의 글이 그 호출의 tool 결과로 기록된 뒤 실행 경로가 부르는 `BrowserHost::acknowledge_closure_notice(session_id, token)`만 지운다. 그래서 한 호출에 `prepare`가 세 번 불리는 auto mode에서도 모델은 그 결과를 한 번 받고, 거절되었거나 실행에 이르지 못한 호출 뒤에도 알림은 남는다(2026-11-11 고침).
7. `/browser` 명령은 모델 turn을 만들지 않고 세션의 기록·줄·예약·취소 상태를 바꾸지 않는다. 그 세션의 turn이 도는 중에도 외부 channel에서 바로 답한다.
8. tool 설명과 안내는 비밀을 대화로 달라고 하지 말라고 적는다.
9. Cargo 시험은 가짜 DevTools, 가짜 process 실행기, 가짜 provider, 시험용 시계만 쓰고 `Cargo.lock`은 바뀌지 않는다.

## Acceptance Criteria

1. `crates/shacs-core/tests/it/spec050_conversational_choice.rs`의 `tool_description_and_auto_guidance_carry_choice_rules`: `browser` tool 설명이 Scope 1의 글과 같고, auto mode system 글에 Scope 2의 줄이 `browser` tool이 있을 때만 있는지 확인한다. 설명에 비밀을 대화로 달라고 하지 말라는 문장과 "직접 입력할 방법이 아직 없다"와 남은 일을 적으라는 문장이 있는지 확인한다.
2. 같은 파일의 `reply_after_choices_reaches_classifier_as_user_message`: 실제 `AgentLoop` turn, 외부 channel, 가짜 provider, 가짜 host로 선택지 글에서 turn을 끝낸 뒤 사용자가 "결제하기 눌러"를 보낸다. 다음 turn의 `[Browser Commitment]`에 걸리는 요소 `click`에서 가짜 classifier가 받은 요청 transcript에 그 글이 사용자 메시지로 있는지 확인한다.
3. 같은 파일의 `capture_sends_fresh_screen_without_model_image`: 화면을 청한 turn에서 가짜 provider가 `capture`와 `message`를 부른다. `screenshots: false`, 그림 입력을 받지 않는 model, 앞선 캡처 뒤 페이지가 바뀐 경우 각각에서 그때 찍은 그림이 지금 대화방의 첨부로 가고, 다음 model 요청에 `image_url` part가 없는지 확인한다. 결과의 경로는 절대 경로다. 다른 세션이 쓰는 중이면 `browser_busy`가 돌아오고 첨부가 없다. host가 문서 변경이나 `browser_stale_revision`을 돌려주면 경로가 없고 첨부가 없다.
4. 같은 파일의 `new_and_stop_close_waiting_browser`: 답을 기다리는 세션의 `/new`와 `/stop` 각각이 host의 `close_for_session`을 부르고, 그 뒤 그 세션의 `browser` 호출이 `browser_closed`를 받는지 확인한다. 그 호출 뒤 host가 `acknowledge_closure_notice`를 그 알림의 token으로 한 번 받고, 다음 `browser` 호출은 평소대로인지도 본다.
5. 같은 파일의 `browser_command_close_and_status_skip_model_turn`: `/browser close`·`/browser status`·잘못된 인자가 provider 요청 없이 Scope 8의 글을 답하고 세션 기록이 그대로인지, 다른 대화방의 `/browser close`도 닫는지, 그 세션의 turn이 도는 중에도 답하는지 확인한다.
6. `crates/shacs-cli/tests/it/spec050_browser_reply_wait.rs`의 `reply_wait_replaces_idle_timeout_until_next_turn`: 실제 `BrowserManager`, 가짜 process 실행기, 가짜 DevTools, 시험용 시계로 `turn_finished` 뒤 `idleTimeoutSeconds`가 지나도 열려 있고 `replyWaitMinutes`가 지나면 닫히는지, 닫힌 뒤 그 세션의 호출이 `acknowledge_closure_notice` 전에는 몇 번이든 `browser_reply_wait_timeout`을 받고 그 뒤에는 받지 않는지, 지난 token이나 모르는 token의 acknowledge는 아무것도 지우지 않는지, 알림이 있는 세션은 다른 세션이 쓰는 중이어도 `browser_busy`가 아니라 알림을 받는지, 기다리는 동안 다른 세션이 `browser_busy`를 받는지, `turn_started` 뒤에는 `idleTimeoutSeconds`로 닫히는지 확인한다.
7. 같은 파일의 `capture_forces_verified_screenshot_and_closes_tab_on_failure`: `screenshots: false`·그림 입력 없음에서도 `Page.captureScreenshot`이 가고 PNG가 돌아오는지, 브라우저가 없을 때 준비와 실행 어느 쪽에서도 새 process가 뜨지 않고 `browser_not_open`인지, 주 문서가 `http(s)`가 아닐 때와 찍는 사이(찍기 직전 확인과 직후 확인 사이) 문서가 바뀔 때 그림이 없고 탭이 닫히는지 확인한다.
8. 같은 파일의 `capture_after_navigation_since_authorization_is_stale`: `prepare`로 digest를 받은 뒤, `capture_prepared`를 부르기 전에 가짜 DevTools가 주 문서 이동 event를 낸다. 결과가 `browser_stale_revision`이고 `Page.captureScreenshot`이 한 번도 가지 않으며 탭과 generation이 그대로인지, 지난 digest로 다시 불러도 같은지, 새로 `prepare`한 digest로는 찍히는지 확인한다. 7번의 "찍는 사이" 경우와 따로 본다.
9. 같은 파일의 `close_request_during_action_runs_after_action`: 동작이 자리 lock을 쥔 동안 온 `close_by_user`와 `close_for_session`이 예약되고 동작이 끝난 뒤 닫히며 알림이 남는지, inspect snapshot에 답 기다리는 기한이 있다가 닫힌 뒤 없어지는지 확인한다. 같은 파일의 `close_request_survives_action_that_fails_after_it`(2026-11-11 독립 검토 뒤 더함): 닫기가 예약된 뒤 그 동작이 `PageTimeout`과 `DocumentChanged`로 끝나는 경우 각각에서 그 세션의 다음 호출(`prepare`와 `acquire`)이 `browser_closed`를 받고 표식 파일이 `closed`인지, 같은 data 폴더의 새 manager도 `browser_closed`를 한 번 전하고 acknowledge 뒤 표식이 없어지는지, 다른 세션이 건 닫기는 `NothingOpen`으로 답하고 알림도 표식도 남기지 않는지 확인한다. 같은 파일의 `close_request_during_teardown_finds_nothing_open`(2026-11-11 두 번째 독립 검토 뒤 더함): 동작이 `PageTimeout`으로 끝나 manager가 예약을 본 뒤 generation을 치우는 사이(가짜 DevTools가 버려지는 순간, 자리 lock은 아직 잡혀 있다)에 `close_by_user`와 그 세션의 `close_for_session`을 부른다. 답이 `NothingOpen`이고 알림과 표식이 남지 않으며, 그 세션이 다시 연 브라우저가 닫히지 않는지 확인한다. `close_request_during_start_is_told_however_start_ends`(같은 때 더함): 브라우저를 띄우는 중에 온 닫기가 `Scheduled`이고(다른 세션의 것은 `NothingOpen`), 띄우기가 성공하면 곧 닫히고 `StartupTimeout`으로 실패해도 그 세션에 `browser_closed` 알림과 `closed` 표식이 남는지 확인한다.
10. 같은 파일의 `restart_reports_browser_closed_once`: 임시 data 폴더에서 manager A가 세션 S로 브라우저를 연다. (가) A가 종료 정리로 닫힌 뒤, (나) A가 닫기 경로 없이 버려져 lock만 풀린 `open` 표식이 남은 뒤 각각 새 manager B를 같은 data 폴더로 만든다. S의 첫 `browser` 호출(`prepare`와 `acquire` 각각)이 `browser_closed`를 받고 새 process를 띄우지 않으며, `acknowledge_closure_notice` 뒤의 둘째 호출은 평소대로이고 표식 파일이 없어지며, 다른 세션 T는 알림을 받지 않는지 확인한다. 답 기다리다 닫힌 뒤 전하지 못한 알림은 B에서 `browser_reply_wait_timeout`으로 한 번 가는지, A가 살아 lock을 쥔 동안에는 B가 표식을 건드리지 않는지, 표식 파일이 0600이고 세션 key 원문과 주소가 없는지, 어시스턴트의 `close`와 쓰지 않아 닫힘은 표식을 남기지 않는지, 표식을 쓸 수 없는 폴더에서는 브라우저가 뜨지 않는지 확인한다.
11. `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `browser_command_bypasses_busy_session_through_external_ingress`: 실제 외부 channel adapter와 runner(`ExternalTransportRunner`, durable 줄, `turn_coordinator`)로, 가짜 provider가 응답을 붙들고 있어 세션의 turn이 도는 동안 같은 대화방에서 `/browser status`와 `/browser close`를 보낸다. 두 답이 그 turn이 끝나기 전에 그 대화방으로 가는지, 도는 turn과 그 세션의 줄에 선 다른 메시지가 취소되지 않고 보낸 순서대로 처리되는지, 세션 파일과 예약이 명령 전후로 같은지, provider 요청이 늘지 않는지 확인한다. 같은 자리에서 `/stop`은 지금처럼 취소하는지 함께 본다. 이 ingress 함수들은 crate 밖에 열려 있지 않아 `tests/it/`에서 부를 수 없다. 그래서 `spec035_cli_media/tests.rs`처럼 `src` 아래 test module에 둔다.
12. 같은 파일의 `capture_path_reaches_message_tool_with_distinct_media_roots`: 실제 CLI adapter의 loop 설정(`loop_config_for`: 그림은 `media/api`, `message`의 첫 media 폴더는 `media`)과 가짜 host로 `capture` 뒤 `message`를 부른다. `capture`가 돌려준 경로가 `media/api/tool-results/` 아래의 있는 파일이고, 대화방으로 나간 첨부가 그 파일이며 내용이 찍은 PNG와 같은지 확인한다. `media/tool-results/<파일>`로는 그 파일에 닿지 않는다는 것도 함께 확인한다.
13. `crates/shacs-config/tests/it/spec050_browser_config.rs`의 `reply_wait_default_and_range`: 기본 30, 5와 120은 받고 4와 121은 config 오류인지 확인한다. `crates/shacs-command/tests/router.rs`에 `/browser` prefix와 인자 풀이, `is_session_free`(`/browser`와 `/browser close`는 참, `/browsers`, `/stop`, 보통 글은 거짓이고 `is_priority`는 `/browser`에 거짓) 검사를 보탠다. `tests/it/`의 새 파일은 기존 `tests/it/main.rs`에만 등록한다.
14. `crates/shacs-cli/tests/it/spec050_browser_reply_wait.rs`의 `undelivered_closure_notices_are_kept_past_sixteen_sessions`: 임시 data 폴더와 시험용 시계로 세션 20개가 차례로 브라우저를 열고 닫힘 알림을 남긴다(답 기다리다 닫힘과 `/browser close`를 섞는다). 어느 세션도 그 뒤 `browser`를 부르지 않은 채로, 같은 manager에서 20개 세션의 알림이 memory의 표에 모두 있고 표식 파일 20개가 모두 있는지 확인한다. 이어서 새 manager를 같은 data 폴더로 만들어(다시 시작) 가장 먼저 닫힌 세션부터 20개 세션 각각의 첫 `browser` 호출이 자기 알림(`browser_reply_wait_timeout` 또는 `browser_closed`)을 받고 `acknowledge_closure_notice` 뒤의 둘째 호출은 평소대로인지, 받은 세션의 표식 파일만 없어지고 아직 받지 않은 세션의 파일은 그대로인지 확인한다. 다시 시작하지 않은 manager에서도 20개 모두가 한 번씩 받는지 따로 본다.
15. `crates/shacs-core/tests/it/spec050_conversational_choice.rs`의 `closure_notice_survives_auto_mode_prepares_until_execution`(2026-11-11 더함): 실제 auto mode 실행 경로(`AgentRunner`의 `direct_runtime_report_with_classifier`)와 알림을 든 가짜 host로 `browser` 호출 하나를 돌린다. classifier 앞·최종 판정 진단·실행의 `prepare` 세 번이 모두 알림을 보고, 모델이 받은 tool 결과에 `browser_closed`(다른 경우에는 `browser_reply_wait_timeout`)가 한 번 있으며, `acknowledge_closure_notice`가 그 token으로 한 번만 불리고, 다음 `browser` 호출은 평소대로 실행되는지 확인한다. 인자가 틀려 거절된 `browser` 호출 뒤에는 acknowledge가 없고 알림이 남아 그다음 호출이 받는지, 기본 mode에서도 한 번 받고 지워지는지 함께 본다.
16. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 전체 workspace test는 사용자 승인 뒤에만 한다. 사용 문서와 `Cargo.lock` 무변경을 확인한다.
17. (2026-11-11 독립 검토 뒤 바꿈. 처음의 `spec042_web_watch/deferral.rs` 시험은 `defer_sessions`와 함께 지웠다.) `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `browser_command_is_answered_before_automation_queued_for_running_session`: 11번과 같은 실제 외부 ingress에 gateway의 worker(`GatewayDurableWork::start_worker`)를 함께 띄운다. 세션의 turn이 도는 동안 실제 cron 넣기(`automation_producer::enqueue_cron`)로 같은 세션에 자동화 일 둘을 줄 세운 뒤 `/browser status`와 `/browser close`를 보낸다. 두 답이 그 turn이 끝나기 전에 가고, 그때까지 자동화 일이 끝나지 않았으며 provider 요청이 늘지 않는지 확인한다. turn이 끝난 뒤 두 자동화 일이 `Succeeded`로 끝나고 맡긴 글이 provider에 한 번씩 가는지(굶지 않음. 2026-11-11 네 번째 독립 검토 뒤: 이 확인이 실패하면 각 일의 시도 수, 상태, 끝 기록의 까닭을 함께 찍는다), 다음 turn이 도는 중에 자동화 일을 줄 세우고 보낸 `/stop`이 바로 답하고 그 turn을 취소하는지, worker의 실패 신호가 서지 않았는지 본다. `crates/shacs-cli/src/surface_approval_worker.rs`의 `failed_pass_raises_failure_signal_and_stop_returns_error`: pass가 오류로 끝난 worker만 실패 신호를 세우고 `stop`이 그 오류를 돌려주는지 확인한다. `crates/shacs-core/tests/it/spec050_conversational_choice.rs`의 `runtime_started_turns_leave_owner_reply_wait_untouched`, `tagged_turns_on_owner_channel_leave_owner_reply_wait_untouched`(2026-11-11 두 번째 독립 검토 뒤 더함), `automation_turn_opening_browser_starts_no_reply_wait`: 실제 `AgentLoop`에서 사용자 turn은 host에 시작과 끝을 알린다. channel이 `system`·`automation`·`cron`·`heartbeat`인 turn은 알리지 않는다. 사용자 channel(`telegram`)의 그 세션으로 온 runtime 표식 turn 둘도 알리지 않는다. 하나는 `goal_continuation`이 달린 inbound로, loop가 목표를 이어 가려고 durable 줄에 넣은 것을 dispatcher로 꺼낸 것이다. 다른 하나는 `automation_route`가 달린 inbound다. 두 turn 모두 그 세션의 브라우저를 쓰는데(`look`) 시작·끝 알림이 늘지 않고 사용자 세션의 기다림이 그대로인지 확인한다. 자동화 turn이 연 브라우저에는 답 기다리는 시간이 켜지지 않는지도 본다.
18. (2026-11-11 두 번째 독립 검토 뒤 더함) `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 시험 rig는 gateway와 같은 `GatewayDurableWork` 하나로 worker와 ingress dispatcher를 연다. 두 dispatcher에 같은 주인을 직접 주던 시험용 상수는 지웠다. `commands_are_answered_while_automation_runs_past_its_lease`: 가짜 provider가 자동화 일의 응답을 붙든 채 실제 임대 기한(30초)이 지나기를 기다린 뒤 `/browser status`와 `/browser close`를 보낸다(시험 하나가 31초쯤 걸린다). 두 답이 가고, 처리 loop가 살아 있고, worker의 실패 신호가 서지 않고, 그 일이 첫 시도의 임대 그대로인지 확인한다. 응답을 놓은 뒤 그 일이 `Succeeded`로 끝나고 맡긴 글이 provider에 한 번만 갔는지, 그 뒤 사용자 글도 한 번 처리되는지 본다. `worker_keeps_working_while_turn_runs_past_its_lease`: ingress가 임대한 turn의 기한이 지난 상태에서 worker가 자동화 일을 `Succeeded`로 끝내고 실패 신호가 서지 않는지 확인한다. `shutdown_requeue_returns_turns_and_leaves_other_work_of_the_owner`: 같은 주인이 임대한 inbound turn과 자동화 일 가운데 `requeue_owned_external_work`가 turn만 줄로 돌려놓는지 확인한다. `processor_shutdown_leaves_running_automation_to_the_worker`: 자동화 일이 도는 중에 처리 loop를 멈추면 그 일이 첫 시도의 임대 그대로 남고 취소만 청해지며, 그 뒤 worker가 멈출 때 한 번 `Failed`(`automation:cancelled`)로 끝나고 맡긴 글이 provider에 한 번만 갔는지 확인한다.
19. (2026-11-11 세 번째 독립 검토 뒤 더함) 같은 파일의 `automation_run_that_panics_ends_the_gateway_wait`: 18번의 rig에서 가짜 provider가 자동화 일의 요청에 panic한다. worker가 그 일을 임대하고 실행하다 panic으로 끝난다. `stop`을 부르기 전에 worker의 실패 신호가 서는지, 그 일이 첫 시도의 `Leased` 그대로이고 처리 loop는 살아 있는지 확인한다. gateway가 보는 것과 같은 신호 둘(처리 thread의 건강 신호, worker의 실패 신호)을 gateway의 종료 대기 `wait_for_ctrl_c_or_runtime_request`에 넣어 `StartupFailed`로 끝나는지, 그 뒤 `stop`이 panic을 오류로 돌려주는지 본다. `processor_thread_that_panics_or_fails_ends_the_gateway_wait`: `ExternalChannelSupervisor::spawn_processor`로 띄운 처리 thread가 panic하거나 건강 신호를 세우지 않고 오류로 돌아갈 때, `stop` 전에 건강 신호가 서고 종료 대기가 `StartupFailed`로 끝나며 `stop`이 까닭을 돌려주는지 확인한다. rig의 처리 loop도 `spawn_processor`로 띄운다. `crates/shacs-cli/src/surface_approval_worker.rs`의 `panicked_pass_raises_failure_signal_before_stop`: pass가 panic한 worker의 실패 신호가 `stop` 전에 서는지 확인한다.
20. (2026-11-11 네 번째 독립 검토 뒤 더함) 같은 파일의 `automation_run_stays_queued_until_its_session_is_free`: 18번의 rig에서 시험이 한 세션의 turn 자리를 쥔 채로 그 세션과 다른 세션에 자동화 일을 하나씩 줄 세운다. 다른 세션의 일이 `Succeeded`로 끝난 뒤에도 바쁜 세션의 일은 임대되지 않은 `Pending`(시도 0, 끝 기록 없음)이고 맡긴 글이 provider에 가지 않았는지 확인한다. 자리를 놓으면 그 일이 첫 시도에 `Succeeded`로 끝나고 맡긴 글이 한 번 가며 worker의 실패 신호가 서지 않는지 본다. `stop_on_claimed_automation_turn_ends_the_run_as_cancelled`: worker의 pass(`process_due_automation`)가 자리를 맡은 직후 그 세션에 취소가 닿게 한다. 그 일이 한 번의 시도로 `Failed`(`automation:cancelled`)로 끝나고, provider 요청이 없으며, 세션이 비어 있는지 확인한다. `crates/shacs-core/tests/it/spec050_conversational_choice.rs`의 `claimed_session_turn_refuses_other_turns_until_it_ends`: 한 thread가 `try_reserve`로 맡은 세션에서 다른 thread의 `reserve`와 `acquire`가 거절되고 둘째 `try_reserve`가 없음을 돌려주는지, 맡은 thread의 `acquire`는 되는지, turn이 끝나면 세션이 비고 다시 맡을 수 있는지 확인한다. 안정성: `spec050_browser_ingress_tests` 전체를 차례로 여러 번 돌려 모두 통과하는지 본다(횟수는 SPEC.md의 2026-11-11 기록).
21. (2026-11-11 다섯 번째 독립 검토 뒤 더함) `crates/shacs-core/tests/it/spec050_conversational_choice.rs`의 `stop_on_a_claimed_session_turn_reaches_only_its_claimer`: 한 thread가 맡은 자리에 취소가 닿은 뒤(`Reserved`) 다른 thread의 `reserve`, 묶기, `acquire`가 `AlreadyActive`를 받고 세션이 여전히 바쁜지 확인한다. 이어서 맡은 thread의 `acquire`가 `Cancelled`를 받고, 자리를 놓으면 세션이 비는지 본다. `priority_commands_leave_a_claimed_session_turn_to_its_claimer`: 다른 thread가 자리를 맡고 있는 세션에 실제 `AgentLoop`로 `/status`, `/restart`, `/stop`을 보낸다. 셋 다 답이 나가고, provider 요청이 없고, 세션 파일이 생기지 않고, `acquire_priority`가 `AlreadyActive`인지 확인한다. 그 뒤 맡은 thread의 `acquire`가 `/status`와 `/restart`에서는 되고 `/stop`에서는 `Cancelled`인지 본다. `crates/shacs-session/tests/durable_work.rs`의 `due_work_past_the_report_cap_stays_runnable`: 기한이 된 일 259개에서 `runnable_work_ids`가 모두를 줄에 든 순서로 담고, `due_work_ids`와 직렬화한 JSON은 앞의 256개만 담으며, JSON에 `runnable_work_ids`가 없는지 확인한다. `crates/shacs-core/tests/it/spec050_runnable_work.rs`의 `work_behind_a_full_report_of_waiting_runs_is_still_taken`: 바쁜 세션의 자동화 일 256개를 먼저 줄 세운다. 그 뒤에 줄 선 042 기한 알림(wake와 그 알림 inbound), 042 웹 감시 확인, 사용자 inbound가 모두 처리되고 256개는 `Pending`(시도 0) 그대로인지 확인한다. `crates/shacs-core/tests/spec036_observation.rs`의 `due_observation_behind_a_full_report_of_waiting_work_is_still_read`: 같은 256개 뒤의 036 관찰이 읽히는지 확인한다. `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `automation_behind_a_full_report_of_waiting_runs_still_runs`: 실제 cron 넣기로 바쁜 세션의 자동화 일 256개와 그 뒤 다른 세션의 일 하나를 줄 세우고 worker의 pass를 한 번 돌린다. 다른 세션의 일이 `Succeeded`로 끝나고 256개는 `Pending`(시도 0) 그대로인지 확인한다. `later_run_of_a_refused_session_waits_behind_the_refused_one`: 같은 세션에 자동화 일 A, B를 줄 세우고, A의 자리 맡기가 실패한 바로 뒤에 그 세션이 비게 한다. 그 pass에서 둘 다 `Pending`(시도 0)이고 provider 요청이 없는지, 다음 pass에서 A, B 순서로 돌아 둘 다 `Succeeded`인지 확인한다. 각 시험이 보는 고침을 하나씩 뺐을 때 그 시험이 실패하는 것도 확인한다(결과는 SPEC.md의 2026-11-11 기록). surface 승인 worker가 후보를 고르는 줄은 같은 목록으로 바꿨지만 256개 뒤의 승인을 따로 보는 시험은 없다.

## Closure Evidence

1. tool 설명·안내 글 비교 결과와 classifier 요청 transcript의 사용자 메시지 증거.
2. 시험용 시계로 본 두 기한의 전환, 닫힘 알림이 한 번만 나가는 결과, 다시 뜬 manager에서 받은 `browser_closed`와 표식 파일의 전후 내용, 세션 20개의 전하지 못한 알림이 다시 시작한 뒤 각각 한 번씩 간 기록.
3. `capture`가 보낸 첨부 경로(저장 폴더와 `message`의 media 폴더가 다른 구성)와 model 요청에 그림이 없다는 증거, 판정 뒤 이동 때 찍지 않은 기록, 찍는 동안의 확인 실패 때 탭 닫힘.
4. `/browser close`·`/browser status`의 답과 provider 요청이 없었다는 증거, 외부 ingress에서 도는 turn 중에 답한 순서 기록과 취소가 없었다는 증거, inspect·diagnostics 예.
5. Cargo 검증 기록, 사용 문서와 `Cargo.lock` 확인.

PRD 000은 `handoff` 없이 닫힌다. 그때 비밀이 필요한 단계는 "직접 입력할 방법이 아직 없다"로 끝난다. 넘겨받기와 로그인 유지는 종료 조건이 아니다.

## 남은 제한

- 언제 멈추고 물을지는 어시스턴트의 판단이다. 묻지 않고 누르면 049 판정이 마지막 방어선이다.
- 번호만 쓴 답은 classifier가 명시 의도로 보지 못할 수 있다.
- `capture`는 revision을 새로 만든다. 그 결과에는 요소 목록이 없으므로 누르려면 다시 `look`해야 한다.
- 판정과 캡처 사이에 페이지가 넘어가면 `capture`는 `browser_stale_revision`으로 끝난다. 어시스턴트가 다시 불러야 한다.
- 같은 owner mode의 두 process가 같은 세션 key로 동시에 브라우저를 쓰면 나중 것은 표식 없이 돈다. 그 process가 죽으면 그 generation에 대해서는 다시 시작 뒤의 닫힘 알림이 없다. owner가 하나인 gateway와 api에서는 생기지 않고, 같은 세션으로 일회 CLI를 둘 띄울 때만 생긴다.
- 표식 파일 이름은 세션 key의 hash다. data 폴더를 읽을 수 있는 사람은 아는 세션 key가 브라우저를 쓰고 있었는지 맞춰 볼 수 있다.
- 전하지 못한 닫힘 알림의 표식 파일은 그 세션이 다시 `browser`를 부를 때까지 남는다. 다시 부르지 않는 세션의 파일(100 bytes 미만)은 계속 남고, shacs-bot은 수나 나이로 지우지 않는다. 지우려면 shacs-bot을 멈추고 `runtime/browser-sessions/`의 파일을 직접 지운다. 그러면 그 세션들은 닫힘 알림을 받지 않는다.
- 자동화 pass(관찰, 웹 감시, 검증 요청, 자동화 일, surface 승인)는 gateway의 worker thread 하나가 차례로 돈다. 앞선 스펙부터 그 thread는 turn이 도는 세션의 mutation lock을 기다리고 자동화 일을 그 자리에서 실행하므로, 그동안 다른 세션의 맡긴 일과 surface 승인은 늦는다. 이 PRD는 그 순서를 바꾸지 않았다. 바꾼 것은 자동화 일을 임대하는 때 하나다(다음 줄). `/browser`와 `/stop`을 받는 외부 loop는 그 thread를 기다리지 않는다.
- 자동화 일은 그 세션에 도는 turn도 맡아 둔 turn 자리도 없을 때만 임대되어 돈다. 세션이 바쁜 동안에는 줄에 남고 worker가 50ms마다 다시 본다. 전에는 그때 `Failed`(`automation:agent:session_busy`)로 끝났다. 이 PRD는 줄에서 기다리는 시간에 한도를 두지 않았다. 세션이 계속 바쁘면 그 일은 그동안 시작하지 않는다. agent turn을 돌리지 않는 자동화 일도 같은 규칙으로 세션이 빌 때까지 기다린다. 사용자 turn과 자동화 일이 같은 순간에 같은 세션을 잡으려 하면 먼저 맡은 쪽이 돌고, 사용자 turn이 졌으면 전부터 있던 길로 줄에 돌아가 자동화 일 뒤에 돈다.
- 같은 세션의 자동화 일은 줄에 든 순서로 돈다. 한 pass에서 자리 맡기에 실패한 세션의 뒤 일은 그 pass에 다시 보지 않으므로, 세션이 pass 도중에 비어도 다음 pass(50ms 뒤)에야 돈다.
- 다른 thread가 turn 자리를 맡아 둔 세션에 온 priority 명령(`/status`, `/restart`, `/stop`)은 도는 turn이 있을 때처럼 답만 하고 세션 기록에 남지 않는다. 전에는 맡아 둔 자리를 보지 않고 세션을 잡았다. `/stop`은 전과 같이 그 자리를 취소한다.
- 보고(`runtime inspect` 등)의 `due_work_ids`는 기한이 된 일 가운데 가장 오래된 256개만 보인다. dispatcher가 고르는 목록(`runnable_work_ids`)은 보고에 나오지 않고, 직렬화한 admission을 다시 읽으면 비어 있다. worker의 pass는 50ms마다 기한이 된 일을 모두(열린 일 한도 1,024개까지) 훑고, 기다리는 세션마다 자리 맡기는 pass에 한 번만 해 본다.
- 같은 승인 요청이 이미 줄에 있는지 찾는 코드(`surface_action/approval.rs`의 `existing_request_outcome`)는 일 목록의 앞 256개만 본다. 앞선 스펙 그대로이고 이 PRD는 바꾸지 않았다.
- worker나 외부 channel 처리 thread의 실패가 gateway를 멈추는 연결은 조각을 이어 시험했다(실제 worker와 처리 thread가 세운 신호를 gateway의 종료 대기 함수에 넣음). `run_gateway` 전체를 띄워 실패시키는 시험은 없다. 실제 transport를 붙인 `ExternalChannelSupervisor::start`도 시험에서 띄우지 않는다.
- gateway의 종료 대기가 보는 신호는 외부 channel 처리 thread와 worker 둘뿐이다. heartbeat thread, cron thread, 처리 loop가 띄우는 turn thread의 panic은 이 신호로 보지 않는다. `serve`(API만 띄움)는 worker의 실패 신호를 보지 않는다. 모두 앞선 스펙 그대로이고 이 PRD는 바꾸지 않았다.
- generation을 끝내기 시작한 뒤에 온 `/browser close`는 "열린 브라우저가 없습니다"로 답한다. 그 세션은 닫힘 알림이 아니라 그 동작의 오류를 tool 결과로 받는다. 브라우저를 띄우는 중에 온 `/browser close`는 띄우기를 멈추지 않는다. 띄우기가 끝난 뒤에 닫는다.
- gateway의 worker와 외부 ingress는 같은 임대 주인을 쓴다. gateway가 사는 동안에는 자기 임대가 기한(30초)을 넘겨도 버려진 것으로 보지 않는다. 일을 끝내지 못하고 남긴 임대(예: 끝 기록 실패)는 gateway를 다시 띄울 때 복구된다. 전에는 그때 상대 쪽 loop가 오류로 멈췄다.
- gateway가 멈출 때 바쁜 세션에서 돌던 자동화 일은 취소되어 `Failed`(`automation:cancelled`)로 끝나고 다시 돌지 않는다. 앞선 스펙의 종료 취소가 하는 일이고 이 PRD는 바꾸지 않았다.
- 답을 기다리는 동안 다른 대화방과 051 웹 감시는 브라우저를 쓰지 못한다. `/browser close`로 푼다.
- subagent처럼 사용자 메시지로 시작하지 않은 실행이 연 브라우저는 049의 시간으로 닫힌다.
- `/browser`는 `/` 글을 가로채는 channel(Slack)에서 쓸 수 없다. 그때는 "브라우저 닫아"라고 말해 어시스턴트의 `close`를 쓴다. 그 길은 브라우저를 쓰는 세션에서만 된다.
- 사용자가 청해 보낸 그림은 channel 회사의 서버에 닿는다.
