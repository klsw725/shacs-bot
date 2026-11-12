# PRD 010. 넘겨받기와 돌려주기

Status: Complete (Scoped)

Depends on: PRD [000](000-conversational-choice.md)의 닫힌 결과(`/browser` 명령 자리와 세션의 줄을 서지 않는 ingress 처리, 답 기다리는 시간과 turn 알림(`browser_owner_turn`), 닫힘 알림, tool 설명과 안내 첫 줄). [049](../../049-container-browser-computer-use/SPEC.md) PRD 010–040의 닫힌 결과(process 전체 자리, 검증된 읽기와 입력, 화면 보기), 013의 `ask_user` 멈춤과 재개, 048·022의 판정, 033의 자동화 결과 받기(`automation_producer`), 012의 외부 channel 전송 worker와 inbound 받기를 소비한다. PRD 020과 051의 구현에 의존하지 않는다.

## Goal

사용자가 브라우저를 넘겨받아 화면 보기 페이지에서 직접 누르고 치고, 끝나면 돌려준다. 넘겨받은 동안에만 화면 보기가 입력을 받고, Mac에서 쓴 글(한글 포함)이 글자 그대로 브라우저에 들어간다. 어시스턴트는 로그인·비밀번호·캡차·본인 확인이 필요하면 `handoff`로 넘기고, 사용자가 답하면 같은 turn을 이어 간다. 넘겨받은 동안 어시스턴트는 브라우저를 조작하지도 보지도 못한다.

## 지금 상태

- 049는 `x11vnc`를 아래 인자로 한 번 띄운다(`crates/shacs-cli/src/browser/browser_viewer.rs`의 `x11vnc_arguments`). 비밀번호 파일은 `rm:`이라 읽은 뒤 없어지고, viewer는 시작할 때 비밀번호를 꺼내 쓴 뒤 버린다(`BrowserViewer::start`의 `secret.take()`). generation 안에서 다시 띄우지 않는다.

  ```text
  x11vnc -norc -noremote -display :<번호> -auth <Xauthority> -rfbport 5900 -listen 127.0.0.1 -localhost -viewonly -nosel -noclipboard -forever -shared -passwdfile rm:<임시 password 파일>
  websockify --web=/usr/share/novnc 0.0.0.0:6080 127.0.0.1:5900
  ```

- 보이는 주소는 `http://127.0.0.1:<6080|6081>/vnc.html`이다(`view_url`). `tools.browser.view`의 키는 `enable`과 `password`뿐이다.
- `BrowserManager`의 자리(`Slot`)에는 조작 상태가 없다. `with_generation`은 세션만 확인한다.
- process를 끝내는 것은 process group에 `SIGKILL`을 보내고 `wait`로 거두는 것이다(`browser_process.rs`의 `stop`). `5900` listener가 누구 것인지는 `viewer_listener`가 `Absent`·`OwnedByChild`·`Foreign`으로 알려 준다. 이 함수는 `/proc/net/tcp`에서 `127.0.0.1`(`0100007F`)의 행만 본다. `0.0.0.0:6080`에 듣는 `websockify`의 listener는 보지 못하고, 049는 그 listener를 확인하지 않는다(Scope 3의 "listener 확인"이 넓힌다).
- turn을 멈추는 tool 결과는 `ToolResult::AskUserInterrupt { question, options }`이고 runner가 `RuntimeInterrupt::AskUser { tool_call_id, name, question, options }`로 바꾼다(`crates/shacs-core/src/runtime/tool_execution.rs`). 재개는 `pending_ask_user_id`가 찾은 답 없는 호출에 `append_ask_user_resume`이 이름 `ask_user`의 tool 메시지를 붙이는 것이다(`crates/shacs-core/src/tools/ask.rs`, `agent_loop.rs`). `pending_ask_user_id`는 이름이 `ask_user`인 호출만 본다.
- `Channel::send`의 성공은 전달이 아니다. 외부 channel adapter의 `send`는 outbound를 worker의 queue에 넣고 돌아온다(`crates/shacs-cli/src/spec035_cli_media/channel_adapter.rs`). 실제 전송과 그 결과(`pending`·`sent`·`failed`)는 worker의 `drain_outbound`에서 정해진다(`crates/shacs-cli/src/lib.rs`). WebSocket adapter의 `send`는 event를 sink에 넣을 뿐이고(`WebSocketEventChannelAdapter::send`), 연결에 쓰는 것은 그 뒤 `sink.take_events()`를 도는 쪽이다.
- `Channel::send`와 전송 함수는 보낸 메시지의 id를 돌려주지 않는다. Telegram 전송은 응답의 `message_id`를 읽고 버린다(`send_telegram_message`). Telegram·Discord inbound는 그 메시지가 어느 메시지에 단 답글인지 읽지 않는다(`telegram_update_to_inbound`, `TelegramInbound`·`DiscordInbound`에 그 자리가 없다). Slack의 `thread_ts`는 세션 key에 들어간다. 그래서 지금은 사용자의 답이 어느 질문에 단 답인지 알 수 없다.
- `websockify` 0.10.0은 연결마다 process를 하나 만든다. 그 process는 `handle_upgrade`에서 token으로 대상을 먼저 찾고, 그 뒤 `new_websocket_client`에서 대상에 연결한다(`websockifyserver.py`, `websocketproxy.py`). 049는 `websockify`를 자기 process group으로 띄우고 group 전체에 `SIGKILL`을 보내 끝낸다.
- 묶음 digest에는 조작 상태를 가리키는 값이 없다. `browser_action_binding.rs`의 `observation`(`look`·`scroll`·`back`·`wait`·`close`, PRD 000의 `capture`)은 문서의 target·session·frame·loader·url을, `empty`(`open`·`close`)는 origin만 담는다. 실행은 `cdp_document.rs`의 `execute_prepared`가 digest를 다시 계산해 비교한다. 그래서 문서가 그대로면 오래전에 준비한 `look`의 digest가 지금 것과 같다.
- capability가 없는 action은 거절되고 `ask_user`만 `LocalUserInteraction`으로 허용된다. auto mode에서는 이 갈림이 사용자 deny 규칙(`denied_by_user`)보다 먼저다(`auto_mode/auto_decision.rs`의 `decide_auto_mode`: 정규화 거절 → capability 없음 → 사용자만 쓰는 파일 → `denied_by_user` → 도구 거절 → 기억 저장소 → …). 다른 mode는 `before_mode_rules`(사용자 deny 규칙) → `decide_without_user_rules`(정적 거절 → `ask_user` → capability 없음 거절 → mode별 판정) → `after_mode_rules`(사용자 ask 규칙) 순서다(`permission_policy.rs`의 `decide_permission`).
- loop 밖에서 끝난 일을 세션에 돌려주는 길은 subagent 결과의 합성 inbound다(`subagent.rs`의 `synthetic_inbound_for_result`: channel `system`, `session_key_override`, metadata `injected_event`). CLI는 모든 inbound를 `automation_producer::enqueue_result`에 먼저 넘기고, `result_source`는 `injected_event`가 있는데 runtime이 붙인 표식(`owner_accepted_automation_result`)이 없는 inbound를 `automation result lacks an owner-accepted terminal boundary`로 거절한다(`crates/shacs-cli/src/automation_producer.rs`). 그 표식은 `InboundMessage`의 private `#[serde(skip)]` 자리라 channel이나 저장된 payload로는 만들 수 없고, durable 줄에서 꺼낼 때 work id 접두사(`child-reentry-`)와 dedupe hint(`subagent.reentry:`)를 보고 다시 붙인다(`crates/shacs-core/src/runtime/durable_dispatch.rs`). 밖에서 온 inbound의 work id는 `work-<digest>-<nonce>`다(`crates/shacs-cli/src/lib.rs`의 `durable_inbound_identity`).
- 049는 popup을 거절하지 않는다. 페이지가 새 창을 열면 새 page target을 지금 탭으로 삼고 앞 탭을 닫는다(`cdp_document.rs`의 `awaiting_popup` 처리). 탭은 늘 하나다.
- 돌려받은 뒤 `look`이 쓸 문서는 `CdpDocument`의 `verified`다. `prepare`는 `verified`가 있으면 observation 묶음을, 없으면 `open`·`close`만 되는 빈 묶음(`browser_action_binding::empty`)을 만든다. `verified`는 main frame의 navigation 사건에서만 새로 잡힌다.

## 설계 결정과 근거 (2026-11-11)

Debian bookworm이 싣는 판은 `x11vnc 0.9.16-9`, `novnc 1:1.3.0-1`, `websockify 0.10.0+dfsg1-4`다(packages.debian.org/bookworm). 아래 근거는 그 판의 man page와 upstream tag의 source다. 구현은 공식 image에 실제로 깔린 package에서 다시 확인하고, Scope 17의 Docker 확인이 마지막 증거다.

1. **`user` 상태에서 입력을 어떻게 켜는가.** 고른 것: 상태가 바뀔 때마다 `x11vnc`를 끝내고 다른 인자로 다시 띄운다.
    - 다른 길 A: `-noremote`를 빼고 실행 중에 `x11vnc -R noviewonly`/`-R viewonly`로 바꾼다. 이 길은 X display의 property(`X11VNC_REMOTE`)로 명령을 받는다(x11vnc(1) `-remote`, `-noremote`: "Do not process any remote control commands or queries"). 같은 display에 붙은 X client는 누구나 그 명령을 보낼 수 있다. 그 display에는 sandbox 없이 도는 Chromium이 붙어 있다. 그래서 페이지가 브라우저를 뚫으면 `assistant` 상태의 보기 전용도 풀 수 있다. 명령은 비동기라 꺼졌는지 확인하려면 다시 물어야 하고, 실패하면 결국 끝내야 한다.
    - 다른 길 B: 비밀번호 파일의 `__BEGIN_VIEWONLY__`로 보기 전용 비밀번호를 따로 둔다. 권한은 연결할 때 정해져 연결이 살아 있는 동안 바뀌지 않는다(x11vnc(1) `-passwdfile`). 돌려준 뒤에도 입력 연결이 남는다.
    - 근거: 다시 띄우면 `assistant` 상태의 process는 인자에 `-viewonly`가 있는 process뿐이고, 입력을 받던 process가 끝났는지를 pid와 listener로 확인할 수 있다. 049의 `-noremote`를 그대로 둔다. 값은 전환 때마다 보기 연결이 끊기는 것이다(남은 제한).
2. **클립보드.** 고른 것: `x11vnc`의 selection은 두 상태 모두 끈다(`-nosel -noclipboard` 유지). 보기 페이지 → 브라우저 글은 4번의 붙여넣기 길 하나로만 간다.
    - 다른 길: `user` 상태에서 `-seldir recv`로 viewer → X 방향만 연다. x11vnc 0.9.16 `selection.c`의 `selection_request()`는 `TARGETS`와 `XA_STRING`만 내세워 `UTF8_STRING`이 없다. `xevents.c`의 `xcut_receive()`는 받은 글을 root window의 `CUT_BUFFER0`에 쓰는데 이 값은 `x11vnc`가 끝나도 남는다. noVNC 1.3.0 `core/rfb.js`의 `clipboardPasteFrom`은 확장 클립보드가 없으면 `text.charCodeAt(i)`를 한 byte로 보낸다("FIXME: text can have values outside of Latin1/Uint8"). 그래서 한글이 깨지고, 돌려줄 때 지울 것이 X에 남는다.
    - 근거: X selection을 열지 않으면 브라우저 → 보기 페이지 방향은 구조로 막히고, X에 남는 클립보드가 없어 "돌려줄 때 지운다"가 따로 할 일이 없이 지켜진다. `-nosel`이면 `xcut_receive()`가 첫 줄에서 돌아간다(`if (!watch_selection) return`).
3. **붙여넣기가 어디로 들어오는가.** 고른 것: 이미 열려 있는 `websockify`의 token 경로 하나를 더 쓴다. `websockify`가 `paste` token을 generation 임시 폴더의 Unix socket으로 잇고, shacs-bot이 `user` 상태 동안만 그 socket을 듣는다.
    - 다른 길 A: 대화로 받는다(`/browser paste <글>`). 글이 channel 회사의 서버와 대화 기록을 지나므로 비밀번호를 붙여넣을 수 없다.
    - 다른 길 B: shacs-bot이 HTTP 서버를 띄우고 host에 port를 하나 더 연다. compose의 port와 049의 "보기 port 하나"가 바뀌고 `Cargo.lock`에 서버 crate가 필요해질 수 있다.
    - 다른 길 C: VNC 서버를 UTF-8 클립보드가 되는 것(TigerVNC)으로 바꾼다. 049가 실제 Docker로 확인한 구성을 통째로 바꾸고 image가 커진다.
    - 다른 길 D: 글자마다 Unicode keysym을 보낸다(`-add_keysyms`). X keymap을 실행 중에 바꾸는 방식이라 빠른 입력에서 글자가 빠진다.
    - 근거: websockify 0.10.0 `websocketproxy.py`는 token plugin이 준 host가 `unix_socket`이면 그 경로로 잇고, token이 없으면 연결을 끊는다. `token_plugins.py`의 `TokenFile`은 `<token>: <host>:<port>` 줄을 찾을 때마다 파일에서 다시 읽고, `ReadOnlyTokenFile`은 처음 한 번만 읽는다. 경로를 상태에 따라 바꿔야 하므로(결정 8) `TokenFile`을 쓴다. noVNC 1.3.0 `app/ui.js`는 `path` 설정(기본 `websockify`)을 주소 뒤에 그대로 붙이므로 `path=websockify?token=view`로 보기 연결도 같은 서버를 쓴다. 새 port, 새 process, 새 crate가 없다.
4. **붙여넣은 글을 브라우저에 어떻게 넣는가.** 고른 것: DevTools `Input.insertText` 한 번. 049의 `type`이 쓰는 것과 같은 명령이고 UTF-8을 그대로 넣는다. X를 거치지 않는다.
5. **붙여넣기 인증과 Origin.** 고른 것: 두 겹이다. `websockify`가 WebSocket 연결의 `Origin`이 `Host`와 같은지 보고(같은 곳에서 연 페이지만), 붙여넣기 listener가 화면 보기 비밀번호로 challenge–response를 한다(nonce와 SHA-256). 비밀번호를 보내지 않는다.
    - 다른 길: 인증 없이 `user` 상태인지만 본다. 049의 `websockify`는 Origin을 보지 않으므로 사용자의 Mac 브라우저가 연 아무 웹페이지나 `ws://127.0.0.1:6080/websockify?token=paste`로 글을 넣을 수 있게 된다.
    - 다른 길: 비밀번호를 그대로 보낸다. 화면 보기 연결은 암호화되지 않아(049) VNC 인증보다 약해진다.
    - 다른 길: Origin을 보지 않고 인증만 둔다. 사용자가 연 아무 웹페이지가 틀린 인증을 되풀이해 그 회차의 붙여넣기 실패 횟수(Scope 6)를 다 써 붙여넣기를 끌 수 있다.
    - 다른 길: `websockify`에 든 `ExpectOrigin`에 받는 Origin 목록을 준다. 목록에는 사용자가 VPN이나 터널로 여는 주소가 들어가야 하는데, shacs-bot이 아는 것은 `tools.browser.view.url`뿐이고 SPEC 소유 범위 5는 그 값이 보여 주기만 한다고 정했다. 그 값을 받는 Origin으로 쓰려면 SPEC을 바꿔야 한다.
    - 근거: websockify 0.10.0 `websocketproxy.py`는 `--auth-plugin`에 점이 든 이름을 주면 그 module을 import해 class를 만들고, WebSocket 연결마다 `authenticate(headers, target_host, target_port)`를 부른다. 정적 파일 요청에는 부르지 않는다(`--web-auth`가 없을 때). 같은 곳 확인은 주소 목록이 필요 없어 127.0.0.1, VPN 주소, 다른 port의 터널에서 모두 같게 돈다. `sha2`는 이미 쓰는 crate다. 페이지 쪽 SHA-256은 작은 함수 하나를 페이지에 넣는다. 평문 `http` VPN 주소에서는 브라우저의 `crypto.subtle`을 쓸 수 없기 때문이다.
    - 이 인증이 막지 못하는 것: nonce와 digest는 비밀번호를 그대로 보내지 않게 할 뿐이다. salt 없는 빠른 hash라서 주고받은 값을 엿들은 사람은 비밀번호를 offline으로 짐작해 볼 수 있다. 붙여넣은 글은 암호화되지 않은 채 지나가고, 연결 사이에 끼어든 사람은 글을 바꿀 수 있다. 화면 보기 비밀번호가 암호화되지 않은 연결에서 주는 보호보다 강하지 않다(남은 제한). Origin 확인은 브라우저가 연 다른 곳의 페이지만 막는다. 브라우저가 아닌 client는 `Origin`을 꾸밀 수 있다.
6. **멈춘 `handoff`를 다른 곳에서 끝났을 때 어떻게 잇는가.** 고른 것: manager가 회차가 끝난 것을 알리면 CLI가 runtime만 만들 수 있는 표식을 단 합성 inbound를 durable 줄에 넣고, loop가 그것을 도구 결과로 붙여 turn을 잇는다.
    - 다른 길 A: 아무것도 보내지 않고 그 대화방의 다음 사용자 메시지에서 정리한다. SPEC 소유 범위 3은 "돌려줬다"·"시간이 지났다"는 도구 결과로 이어 간다고 정했고, 다른 대화방에서 돌려준 사용자는 그 대화방에 다시 말하지 않을 수 있다.
    - 다른 길 B: metadata `injected_event`만 단 inbound를 낸다. `automation_producer::result_source`가 표식 없는 `injected_event`를 거절하므로 loop에 닿지 않는다. 그 거절을 metadata 값으로 푸는 것은 밖에서 온 메시지가 같은 metadata를 흉내 낼 수 있게 하므로 하지 않는다.
    - 다른 길 C: subagent의 표식(`OwnerAcceptedAutomationResult`)에 종류를 하나 더한다. 그 표식은 "자동화 결과로 기록한다"는 뜻이라 브라우저 이어 가기가 자동화 결과 목록에 들어가게 된다.
    - 근거: 표식을 private `#[serde(skip)]` 자리로 두면 channel adapter와 저장된 payload는 만들 수 없다. durable 줄에서 꺼낼 때는 subagent 재진입과 같은 방법(runtime이 정한 work id 접두사와 dedupe hint)으로 다시 붙인다. 새 queue나 timer를 만들지 않는다.
7. **`handoff`가 turn을 멈추는 방법.** 고른 것: `ToolResult::AskUserInterrupt`를 그대로 쓴다. `RuntimeInterrupt::AskUser`는 이미 tool 이름을 담는다. 새 interrupt 종류를 만들지 않는다.
8. **넘겨받는 순서와 화면 보기 경로.** 고른 것: 먼저 회차를 기록하고 상태를 `user`로 바꾼 뒤에야 입력을 받는 process를 띄운다. `x11vnc`를 바꾸기 전에 `websockify`를 그 연결 process까지 모두 끝내고 없어진 것을 확인한다. 새 `x11vnc`의 listener가 그 process의 것임을 확인한 뒤에만 경로 파일을 쓰고 `websockify`를 새로 띄운다. 끄는 순서는 하나만 두고, 돌려주기와 넘겨받기 실패가 같이 쓴다.
    - 다른 길 A: 입력을 받는 `x11vnc`와 붙여넣기 listener를 먼저 띄우고 마지막에 상태를 `user`로 바꾼다. 그 사이 `websockify`가 계속 이어 주므로 비밀번호를 아는 client가 `assistant` 상태에서 입력을 보낼 수 있고, 다른 process가 `5900`을 차지했는지 확인하기 전에 그 listener가 밖에서 닿는다. 실패했을 때 입력 process가 끝났는지 확인하지 않고 `assistant`로 남을 수도 있다.
    - 다른 길 B: `websockify`는 그대로 두고 전환하는 동안 경로 파일만 비운다. 경로 파일은 새로 token을 찾는 연결만 막는다. 이미 `view`로 `127.0.0.1:5900`을 찾아 놓고 아직 연결하지 않은 연결 process는 남는다. 그 process가 전환 뒤에 연결하면 소유를 확인하기 전의 새 `x11vnc`나, 그 사이 `5900`을 차지한 다른 process에 닿는다.
    - 다른 길 C: `websockify`를 두고, 연결하는 순간마다 회차와 대상의 소유를 다시 확인한다. websockify에는 token을 찾은 뒤 연결 직전에 부르는 자리가 없어서 `websocketproxy.py`를 고친 판을 image에 실어야 한다. 확인과 연결 사이의 틈도 그 안에서 다시 막아야 한다.
    - 근거: 상태가 먼저 `user`가 되면 어시스턴트의 동작은 첫 단계부터 막힌다. `websockify`와 그 연결 process가 하나도 없으면 화면 보기 port에서 `5900`이나 붙여넣기 socket으로 가는 길이 없고, 늦게 연결할 process도 없다. 연결 process는 `websockify`의 process group에 남는다(websockify는 `--daemon`이 없으면 group을 바꾸지 않는다). 그래서 group 전체를 끝내고 그 group에 산 process가 없는지 보면 된다. 끝난 process의 socket은 kernel이 닫는다. 새 `websockify`는 새 listener의 소유를 확인한 뒤에 뜨므로 그 연결 process가 찾는 대상은 늘 확인된 것이고, 다음 전환은 그 `websockify`부터 끝낸다. 끄는 순서가 하나면 "입력 process가 끝난 것을 확인하지 못하면 `assistant`가 되지 않는다"를 한 곳에서 지킨다. 값은 전환 때마다 `websockify`도 다시 떠서 화면 보기 페이지가 잠깐 열리지 않는 것이다(남은 제한).
9. **답과 돌려주기를 어느 회차에 맞추는가.** 고른 것(SPEC 결정 기록 (마)): 두 가지를 본다. channel이 답글 정보를 주는 메시지는 그 답글이 가리키는 메시지로 맞춘다. 지금 회차의 안내 메시지에 단 답글만 지금 회차의 것이다. 지난 회차의 질문이나 다른 메시지에 단 답글은 지금 회차를 끝내지 못한다. 답글 정보가 없는 보통 메시지와 인자 없는 `/browser return`은 shacs-bot에 닿은 때 열려 있던 회차의 것으로 본다(답이면 그 질문이 이미 전달된 회차). 이 값은 메시지가 처음 들어올 때 ingress가 메시지에 적어 둔다(도착 표시). `/browser return <회차 표시>`는 적은 회차만 끝낸다.
    - 다른 길 A: 처리할 때 열려 있는 회차를 끝낸다. H1이 한도로 끝나고 같은 대화방에서 H2가 열린 뒤, H1 때 닿아 줄에서 기다리던 "다 했어"가 H2를 끝낸다. 사용자는 아직 로그인하는 중인데 입력이 꺼진다.
    - 다른 길 B: 답글 정보가 있는 메시지만 답으로 받는다. 답글 기능이 없는 channel(Email, WhatsApp, WebSocket, API)과 답글을 쓰지 않는 사용자는 답할 수 없다. SPEC 소유 범위 5는 대화 길이 channel의 기능에 기대지 않는다고 정했다.
    - 다른 길 C: 답에 회차 표시를 적게 한다("다 했어 7f3a-2"). SPEC 소유 범위 6은 사용자가 그 질문에 답하면 답의 내용과 상관없이 돌아온다고 정했고, 결정 (마)도 회차 표시를 꼭 적게 하지 않는다.
    - 다른 길 D: 답글이 지난 회차의 질문을 가리키는 것이 확인될 때만 막고, 모르는 메시지에 단 답글은 보통 메시지로 본다. 지난 질문의 id를 얼마나 오래 들고 있느냐에 따라, 오래된 질문에 단 답글이 지금 회차를 끝낼 수 있다.
    - 근거: 답글 정보는 사용자가 어느 메시지를 보고 답했는지를 channel이 알려 주는 값이라 늦게 닿아도 바뀌지 않는다. 지금 회차의 안내 메시지 id 하나와만 견주므로 지난 질문의 기록이 남아 있는지에 기대지 않는다. 답글 정보가 없으면 shacs-bot이 아는 가장 이른 때는 메시지가 닿은 때다. 그래서 channel이 늦게 전한 답글 없는 메시지는 지금 회차의 답으로 쓰일 수 있다. SPEC이 (마)로 정한 보장이 여기까지다. 질문이 전달되기 전에 닿은 메시지는 그 질문을 보고 한 답일 수 없으므로 답으로 쓰지 않는다. 답글 정보와 도착 표시는 보안 장치가 아니다. 주인의 메시지는 어차피 회차를 끝낼 수 있다.
10. **돌려받은 뒤의 문서.** 고른 것: 넘겨받기 전에 준비한 묶음은 종류와 상관없이 모두 못 쓰게 한다. 모든 묶음 digest에 조작 판 번호를 넣고 넘겨받을 때와 돌려줄 때 올린다. 요소 번호는 049대로 문서 epoch를 올리고 snapshot을 버려 못 쓰게 한다. 검증된 대상은 버리지 않고 그 자리에서 지금 문서로 새로 맞춘다.
    - 다른 길 A: epoch를 올리고 snapshot만 버린다. `look`·`capture`의 observation 묶음과 `open`·`close`의 빈 묶음에는 epoch가 없다(`browser_action_binding.rs`). 사용자가 문서를 바꾸지 않고 돌려주면, 넘겨받기 전에 판정받은 `look`이나 `capture`가 돌려준 뒤에도 같은 digest로 실행된다. 그 사이 사용자가 로그인해 둔 화면을 그 판정으로 읽게 된다.
    - 다른 길 B: snapshot과 검증된 대상을 함께 버린다. 돌려받은 뒤 navigation 사건이 없으면 `verified`가 비어 `look`의 `prepare`가 빈 묶음이 되고, 사용자가 로그인해 둔 페이지를 다시 `open`해야 한다.
    - 근거: 049가 지키는 것은 "판정한 문서와 실행하는 문서가 같다"이다. 넘겨받기는 문서의 주소가 같아도 그 안의 상태(로그인)를 바꾼다. 조작 판 번호가 digest에 들어가면 049의 digest 비교(`execute_prepared`)가 그대로 "넘겨받기 전의 판정"을 거른다. 돌려받은 뒤의 첫 `look`은 새 조작 판과 새로 맞춘 문서에 묶여 판정받는다.
11. **`handoff` 판정의 자리.** 고른 것: `ask_user` 예외 줄에 끼워 넣지 않고, 사용자 deny 규칙과 도구 거절, 기억 저장소 확인을 지난 자리에 `handoff` 갈림을 따로 둔다.
    - 다른 길: `decide_auto_mode`의 `ask_user` 갈림에 `browser`의 `handoff`를 더한다. 그 갈림은 `denied_by_user`보다 먼저라서 `deny: ["browser"]`를 쓴 사용자에게도 `handoff`가 허용된다.
    - 근거: 사용자가 `browser`를 막았으면 넘겨받기 질문도 나가지 않아야 한다. `ask_user`의 자리는 바꾸지 않는다.
12. **질문이 전달된 때를 어디서 아는가.** 고른 것: outbound를 실제로 내보내는 마지막 자리가 성공으로 끝난 것만 전달로 본다. 외부 channel은 worker의 `drain_outbound`에서 전송이 성공으로 끝난 가지, WebSocket은 그 event를 연결에 쓴 뒤, HTTP API는 응답을 다 쓴 뒤, 직접 CLI는 출력한 뒤다.
    - 다른 길: `Channel::send`가 성공으로 돌아온 때로 본다. 외부 adapter의 `send`는 queue에 넣기만 한다. worker가 아직 보내지 않았거나 전송이 실패해도 전달된 것이 되고, 그 사이 닿은 메시지가 사용자가 보지도 못한 질문의 답으로 쓰인다.
    - 근거: 전송 결과는 그 자리에서만 정해진다. `record_delivery_state`가 `sent`·`failed`를 적는 자리와 같다. channel이 준 메시지 id도 그때 나오고, 결정 9의 답글 맞추기가 그 id를 쓴다.
13. **끝난 회차의 기록을 언제까지 두는가.** 고른 것: 이어 가기를 기다리는 끝난 회차의 기록은 그 이어 가기가 처리될 때까지 수와 상관없이 둔다. 다 처리된 기록만 8개까지 둔다.
    - 다른 길: 끝난 회차를 모두 최근 8개만 둔다. 이어 가기 inbound가 durable 줄에서 기다리는 동안 다른 회차 8개가 끝나면 기록이 밀려나고, loop의 확인(Scope 9)이 맞는 이어 가기를 버린다.
    - 근거: 기다리는 기록은 멈춘 `handoff` 호출 하나에 하나다. 세션 하나에 멈춘 호출은 하나라서 그 수는 멈춘 세션의 수를 넘지 않는다. 따로 한도를 두지 않아도 끝없이 늘지 않는다.

## Scope

1. **조작 상태:** 자리(`Slot`)에 조작 상태 `assistant`·`user`를 둔다. generation이 없거나 새로 뜨면 `assistant`다. 상태는 회차 기록이 있는 동안 `user`다(Scope 2).
    - `user` 상태에서 `browser`의 모든 동작(`open`, `look`, `capture`, `click`, `type`, `press`, `scroll`, `back`, `wait`, `close`, `handoff`)은 어느 세션이 부르든 실행하지 않고 아래 글을 돌려준다. `prepare`, `close_prepared`, `acquire`, `capture` 모두 같다. 이 호출들은 DevTools 명령을 만들지 않는다.

      ```text
      browser_user_control: 사용자가 조작 중입니다. 돌려받을 때까지 기다리세요
      ```

    - 049의 문서 지키기(탭 하나, download·파일 고르기 거절, process와 연결 확인)는 `user` 상태에서도 `maintenance`가 그대로 한다. 캡처와 읽기는 하지 않는다.
    - `user` 상태에서도 `/browser close`, 그 세션의 `/new`·`/stop`, process 종료는 브라우저를 닫는다(PRD 000). 닫히면 회차는 `closed`로 끝난다.
2. **넘겨받기 회차:** 넘겨받을 때마다 회차 기록 하나를 자리에 둔다.
    - 기록: 회차 번호(process가 뜰 때 만든 nonce 16 hex, `-`, 1부터 세는 번호. 예 `9f2c41d07ab3e6c1-3`), 단계(`opening`·`open`·`closing`), 넘겨받은 세션 key와 channel·chat, 넘겨받은 turn의 답이 가던 자리(아래 "답이 가던 자리"), `handoff`가 넘긴 `tool_call_id`(`handoff`로 시작한 회차에만), 그 호출이 실제로 멈췄는지(`paused`), 그 질문이 전달됐는지(`asked`), 안내 메시지(`handoff` 질문 또는 `/browser takeover`의 답)의 channel 메시지 id(전달된 뒤에, channel이 줄 때만), 입력을 받는 `x11vnc`를 띄우기 시작했는지(`input_spawned`), 기한.
    - 회차 표시: 사용자에게 보이는 짧은 이름이다. nonce의 앞 4 hex, `-`, 번호다(예 `9f2c-3`). `/browser takeover`의 답, `handoff` 질문, `/browser status`에 나온다.
    - 끝내기: 회차를 끝내는 쪽은 자리 lock 아래에서 단계를 `open`에서 `closing`으로 바꾼 쪽 하나다. 그쪽만 Scope 4를 하므로 한 번만 끝난다. 끝난 까닭은 `answered`, `returned`, `limit`, `cancelled`, `closed`, `view_failed`, `failed`(Scope 3의 실패) 중 하나다.
    - 받은 호출 id와 멈춘 호출: 둘은 다르다. `tool_call_id`는 `handoff`가 `BrowserHost::takeover`를 부를 때 넘긴 값이라 회차 기록을 만들 때(단계 `opening`)부터 있다. 그것만으로는 호출이 멈췄다고 보지 않는다. 호출이 멈추는 것은 tool이 `ToolResult::AskUserInterrupt`를 돌려줄 때뿐이다(`tool_execution.rs`의 도구 결과 처리. 글 결과는 보통 도구 결과로 붙고 turn이 그대로 이어진다). tool은 `takeover`가 성공으로 돌아온 뒤에만 그것을 돌려준다(Scope 8). 그래서 `paused`는 꺼진 채로 시작하고, Scope 3의 9번에서 단계를 `open`으로 바꿀 때 `tool_call_id`가 있으면 같은 자리 lock 아래에서 켠다. 켜는 곳은 거기 하나다. 이 문서에서 "멈춘 `handoff`가 있다"와 "멈춘 호출이 있다"는 `paused`가 켜져 있다는 뜻이다. 단계가 `opening`인 채 끝난 회차는 `tool_call_id`가 있어도 멈춘 호출이 없다. `tool_call_id`가 없는 회차(`/browser takeover`로 시작한 것)는 `paused`가 늘 꺼져 있다.
    - 끝내려는 요청은 무엇을 끝내려는지 밝힌다. `handoff` 답은 세션 key·`tool_call_id`·회차 번호로, `/browser return`은 회차 번호로(Scope 11), 한도와 보기 실패는 회차 번호로 맞춘다. 지금 회차와 맞지 않으면 아무것도 바꾸지 않는다. "지금 열려 있는 회차를 그대로 끝낸다"는 요청은 없다. 그 세션의 `/stop`·`/new`만 세션 key로 맞춘다. 그 둘은 그 세션이 지금 하는 일을 끝내는 명령이기 때문이다.
    - 답이 가던 자리(2026-11-12 독립 검토 뒤 더함): channel·chat만으로는 대화방 안의 topic이나 thread를 가리키지 못한다. 그래서 넘겨받을 때, 넘겨받은 turn의 inbound metadata 가운데 channel이 답 보낼 자리를 정하는 데 쓰는 key만 골라 기록에 둔다: `message_thread_id`(Telegram topic), `slack`(Slack thread의 `thread_ts`), `thread_id`, `parent_channel_id`, `subject`(`browser_host.rs`의 `REPLY_ROUTE_METADATA_KEYS`). 이 inbound는 channel adapter가 만든 것이라 믿는 값이다. 다른 key는 두지 않는다.
    - 끝난 회차의 기록: 번호, 까닭, 세션 key, channel·chat, 답이 가던 자리, `tool_call_id`, `paused`를 memory에 둔다(Scope 9의 확인과 답 보낼 곳). 두 가지로 나눠 둔다.
        - 이어 가기를 기다리는 기록: `paused`가 켜진 채 끝났고(멈춘 `handoff`가 있었고) 까닭이 `answered`·`cancelled`가 아닌 회차다. 수와 상관없이 두고 8개 한도에 넣지 않는다. 다 처리된 기록으로 옮기는 때는 둘뿐이다. loop가 그 이어 가기를 도구 결과로 붙여 세션에 저장한 뒤 `BrowserHost::resume_settled(회차 번호)`를 부른 때와, 그 호출이 다른 길로 닫힌 때(Scope 8의 `browser_takeover_over`, 그 세션의 `/stop`·`/new`)다. 기다리는 기록은 멈춘 호출 하나에 하나라서 멈춘 세션의 수를 넘지 않는다.
        - 다 처리된 기록: 그 밖의 끝난 회차와 위에서 옮겨 온 것이다. 최근 8개만 두고 넘으면 가장 오래된 것을 버린다. 진단용이다. 회차를 끝내거나 이어 가기를 받는 판단에는 쓰지 않는다. 그래서 버려져도 어떤 요청의 결과가 달라지지 않는다.
    - 넘겨받는 중에 실패한 회차: 단계가 `opening`인 채 끝난 회차는 `handoff`로 시작했어도 이어 가기를 기다리는 기록을 만들지 않고 Scope 9의 알림도 내지 않는다. 까닭이 `failed`일 때도, 되돌리다가 Scope 4의 4번으로 generation이 닫혀 까닭이 `closed`가 된 때도 같다. 그 호출은 멈춘 적이 없고 Scope 8의 글 결과 하나로 이미 닫혔다. 여기에 이어 가기를 내면 이미 결과가 붙은 호출에 두 번째 결과를 붙이려는 inbound가 생긴다. 이런 회차는 다 처리된 기록에만 진단용으로 남는다.
    - 도착 표시용 값: 지금 열려 있는 회차의 번호와 안내 메시지 전달 여부(`asked`)를 자리 lock을 기다리지 않고 읽을 수 있게 따로 둔다(Scope 8). 단계가 `open`일 때만 회차 번호가 있다.
    - `user` 상태에서 다시 넘겨받으려는 요청은 거절한다. `/browser takeover`는 `이미 넘겨받은 상태입니다.`를 답하고 `handoff`는 `browser_user_control`을 받는다.
3. **넘겨받기 순서:** 자리 lock 아래에서 한다. 앞의 세 확인은 아무것도 바꾸지 않는다.
    - 누구의 브라우저인가: `handoff`는 그 세션의 generation이거나 generation이 없을 때만 된다(다른 세션의 것이면 049의 `browser_busy`). `/browser takeover`는 어느 세션의 generation이든 넘겨받고 generation의 세션은 바꾸지 않는다. generation이 없으면 049 `acquire`와 같은 시작 경로(process gate, `tools.browser.enable`, policy 확인)로 띄워 `about:blank` 한 탭으로 시작하고, 요청한 세션이 그 generation의 세션이 된다. 시작이 거절되면 049의 오류를 그대로 알린다.
    - 실행 중인 동작: 자리 lock을 다른 동작이 쥐고 있으면 넘겨받기를 예약하고 그 동작이 끝난 직후 이어서 한다. 요청은 `pageTimeoutSeconds + 5초`까지 기다리고, 넘으면 예약을 지우고 `browser_busy`를 알린다.
    - 화면 보기 확인: viewer 상태가 `open`이어야 한다(`tools.browser.view.enable`, 유효한 비밀번호, 두 process가 살아 있음). 아니면 049의 view 사유를 붙여 거절하고 아무것도 바꾸지 않는다.
    - 그다음은 아래 순서다. 어느 단계든 실패하면 거기서 멈추고 Scope 4를 까닭 `failed`로 한다. 넘겨받기는 사유 한 줄과 함께 거절된다. 이 회차는 `paused`가 꺼진 채 끝나므로 이어 가기 알림도 이어 가기를 기다리는 기록도 없다(Scope 2).
        1. 회차 기록을 만들고(단계 `opening`) 상태를 `user`로 바꾼다. 이때부터 Scope 1대로 어시스턴트의 동작이 막힌다.
        2. `view-routes`를 빈 내용으로 바꾸고(Scope 5) `websockify`를 끝낸다(아래 "`websockify` 끝내기"). 화면 보기 port가 닫히고 이어져 있던 보기 연결이 끊긴다.
        3. 보기 전용 `x11vnc`를 끝내고, process가 거둬졌고 `5900` listener가 없는지(`Absent`) 확인한다.
        4. 회차 기록의 `input_spawned`를 켠다. 그 뒤에 새 비밀번호 파일(`<generation root>/view-<32 hex>`, 0600, `create_new`)을 쓰고 아래 인자로 `x11vnc`를 띄운다.

           ```text
           x11vnc -norc -noremote -display :<번호> -auth <Xauthority> -rfbport 5900 -listen 127.0.0.1 -localhost -nosel -noclipboard -forever -shared -passwdfile rm:<임시 password 파일>
           ```

           049 인자에서 `-viewonly` 하나만 뺀 것이다. viewer는 검증한 비밀번호를 generation 동안 memory에 둔다(`Debug`에 나오지 않게 한다). 파일은 띄울 때마다 새로 쓰고 읽힌 뒤 없어진다.
        5. `5900` listener가 그 process의 것(`OwnedByChild`)이고 비밀번호 파일이 없어졌는지 10초 안에 확인한다. listener는 아래 "listener 확인"의 `Loopback`(`127.0.0.1:5900`)으로 본다. 049가 하던 확인 그대로다. 다른 process의 listener(`Foreign`)이거나 확인되지 않으면 실패다.
        6. 붙여넣기 listener를 연다(Scope 6).
        7. `view-routes`에 `view`와 `paste` 두 줄을 쓴다.
        8. `websockify`를 Scope 5의 인자로 새로 띄우고, 화면 보기 port(container의 `6080`)의 listener가 그 process의 것(`OwnedByChild`)인지 10초 안에 확인한다. 아래 "listener 확인"의 `Any`(`0.0.0.0:6080`)로 본다. 049의 `viewer_listener`는 `127.0.0.1`의 행만 보아 이 listener를 보지 못하므로 그대로 쓰지 않는다. `Foreign`이거나 확인되지 않으면 실패다. 입력을 받는 process가 화면 보기 port로 닿는 것은 이때부터다.
        9. 단계를 `open`으로 바꾸고 기한을 이때부터 센다.
    - listener 확인: 넘겨받기와 Scope 4가 같이 쓴다. 049의 `BrowserProcessRunner::viewer_listener(port, owner)`는 `/proc/net/tcp`에서 local 주소가 `0100007F:<port>`(`127.0.0.1`)인 LISTEN 행만 본다(`browser_process.rs`의 `linux_loopback_listener`). `websockify`는 `0.0.0.0:6080`에 듣기 때문에 이 함수로 물으면 떠 있어도 늘 `Absent`가 나온다. 049는 `websockify`의 listener를 확인한 적이 없다. 그래서 듣는 주소를 인자로 더해 `viewer_listener(address, port, owner)`로 바꾼다(SPEC "앞선 스펙에서 바꾸는 것"의 049 `x11vnc`·`websockify` 행). `websockify`용 함수를 따로 두지 않는 것은 읽는 표와 주인을 가리는 법이 같기 때문이다.
        - `address`는 둘뿐이다. `ViewerListenAddress::Loopback`은 `127.0.0.1`이고 `/proc/net/tcp`의 `0100007F`다. `ViewerListenAddress::Any`는 `0.0.0.0`이고 `00000000`이다. `x11vnc`의 `5900`은 늘 `Loopback`으로, `websockify`의 화면 보기 port(`6080`)는 늘 `Any`로 본다. 049가 부르던 두 자리(`browser_viewer.rs`의 `5900` 확인)는 `Loopback`을 넘기고 결과가 그대로다.
        - 읽는 법: `/proc/net/tcp`의 첫 줄(제목)을 빼고, 넷째 칸이 `0A`(LISTEN)이고 둘째 칸의 port가 4 hex 대문자로 그 port인 행을 본다. `5900`은 `170C`, `6080`은 `17C0`이다. `Loopback`은 그 가운데 둘째 칸이 `0100007F:170C`와 글자 그대로 같은 행만 센다(049와 같다). `Any`는 그 port의 행을 주소와 상관없이 모두 센다. `0.0.0.0`에 듣는 것은 모든 IPv4 주소의 그 port를 차지하는 것이라, 다른 주소의 같은 port에 듣는 process가 있으면 남의 listener가 있는 것이기 때문이다. 센 행의 열째 칸이 socket inode다.
        - 답: 센 행이 없으면 `Absent`다. `Loopback`은 049대로 센 행 가운데 하나라도 `/proc/<owner pid>/fd`의 link `socket:[<inode>]`와 맞으면 `OwnedByChild`다. `Any`는 주소가 `00000000`인 행이 있고 센 행의 inode가 모두 owner의 fd에 있을 때만 `OwnedByChild`다. `websockify`가 `127.0.0.1:6080`에만 듣고 있으면(`0100007F:17C0`) `OwnedByChild`가 아니다. 그 밖(`owner`가 없음, fd를 읽지 못함, 맞지 않는 inode가 있음, `/proc/net/tcp`를 읽지 못함)은 `Foreign`이다. Linux가 아니면 049대로 `Foreign`이다.
        - IPv6(`/proc/net/tcp6`)는 049와 같이 보지 않는다. `websockify`는 `0.0.0.0`으로 IPv4에만 듣는다.
        - 시험할 수 있게 나눈다: 표를 읽고 답을 정하는 부분을 `/proc`를 읽지 않는 `pub` 함수 `viewer_listener_from(address, port, tcp 표의 글, owner의 fd link 목록)`으로 `browser_process.rs`에 떼어 둔다. fd link 목록이 없으면(`owner`가 없거나 읽지 못함) 센 행이 있을 때 `Foreign`이다. Linux 구현은 `/proc/net/tcp`와 `/proc/<pid>/fd`를 읽어 이 함수에 넘기기만 한다. 이 함수에는 `cfg(target_os = "linux")`를 붙이지 않아 Linux가 아닌 개발 host의 Cargo 시험에서도 돈다.
    - `websockify` 끝내기: 넘겨받기와 Scope 4가 같이 쓴다. 049의 process group 종료(group 전체에 `SIGKILL`)로 끝내고 아래 셋을 모두 확인한다.
        - 직접 띄운 `websockify` process가 거둬졌다.
        - 그 process group에 산 process가 없다. `/proc`에서 process group이 그 `websockify`의 것이고 zombie가 아닌 process를 찾아 하나도 없어야 한다. `websockify`의 연결 process는 같은 group에 있으므로, token으로 대상을 찾아 놓고 아직 연결하지 않은 연결 process도 여기서 없어진 것이 확인된다.
        - 화면 보기 port에 listener가 없다. `viewer_listener(Any, 6080, owner 없음)`이 `Absent`여야 한다(위 "listener 확인"). `Loopback`으로 물으면 `websockify`가 떠 있어도 `Absent`가 나오므로 그렇게 보지 않는다.
    - 이 셋이 확인되기 전에는 `x11vnc`를 끝내지도 띄우지도 않는다. 그래서 앞 `websockify`의 연결 process는 새 `x11vnc`에도, 앞 `x11vnc`가 끝난 뒤 `5900`을 차지한 다른 process에도 연결하지 못한다. 2초 안에 확인되지 않으면 Scope 4의 4번대로 generation을 닫는다.
    - 조작 판 번호: 1번에서 상태를 `user`로 바꿀 때 자리의 조작 판 번호를 1 올린다(Scope 4의 6번).
4. **입력 끄기와 돌려주기:** 입력을 끄는 순서는 이것 하나다. 회차를 끝내는 모든 까닭(`answered`, `returned`, `limit`, `cancelled`, `view_failed`)과 Scope 3의 실패(`failed`)가 같은 함수를 쓴다. 자리 lock 아래에서 아래 순서로 하고, `assistant`로 바꾸는 것은 맨 끝이다.
    1. `view-routes`를 빈 내용으로 바꾸고 `websockify`를 끝낸다(Scope 3의 "`websockify` 끝내기"). 이미 끝나 있으면 세 확인만 한다. 화면 보기 port에서 안으로 가는 길과 연결 process가 모두 없어진다.
    2. 붙여넣기 listener와 그 연결을 닫고 socket 파일을 지운다(열려 있을 때).
    3. 떠 있는 `x11vnc`를 049의 process group 종료로 끝내고, process가 거둬졌고 `5900` listener가 없는지(`Absent`) 확인한다. `input_spawned`가 켜져 있으면 그것이 입력을 받던 process다. 떠 있는 `x11vnc`가 없어도 `5900` listener가 없는지는 확인한다.
    4. 1–3 가운데 하나라도 2초 안에 확인되지 않으면 generation 전체를 049의 닫기 경로로 닫는다. `websockify`, `x11vnc`, `Xvfb`가 함께 끝나므로 화면 보기 port로 닿는 길과 입력할 display가 없어진다. 회차의 까닭은 `closed`가 되고 PRD 000의 닫힘 알림을 남긴다. generation이 없으므로 상태는 `assistant`다. 입력이 꺼졌는지 모르는 채로 살아 있는 브라우저가 `assistant`가 되는 길은 없다. 아래 5–7은 하지 않는다.
    5. 보기 전용 `x11vnc`를 049 인자(`-viewonly` 있음)로 띄우고 Scope 3의 5번과 같은 방법으로 확인한다. 확인되면 `view-routes`에 `view` 한 줄만 쓰고 `websockify`를 새로 띄워 Scope 3의 8번과 같이 확인한다. 둘 중 하나라도 실패하면 띄운 process를 끝내고 viewer를 `failed`로 둔다. 그때 `view-routes`는 빈 채이고 `websockify`는 떠 있지 않다. 브라우저는 계속 쓴다(049 PRD 040의 "화면 보기가 실패해도 브라우저는 뜬다"와 같다). 까닭이 `view_failed`이면 둘 다 띄우지 않고 viewer를 `failed`로 둔다.
    6. 문서: 조작 판 번호를 1 올린다. 조작 판 번호는 자리에 두는 수다. process가 뜰 때 0이고, 넘겨받을 때(Scope 3의 1번)와 여기서 오르며, generation이 바뀌어도 줄지 않는다. `browser_action_binding`이 만드는 모든 묶음(번호 동작의 묶음, `observation`, `empty`)은 이 값을 `control` 자리에 담는다. `look`, `capture`, `open`, `close`, `scroll`, `back`, `wait`, `click`, `type`, `press`의 준비와 실행이 모두 해당한다. 실행은 049대로 digest를 다시 계산해 견주므로(`execute_prepared`), 넘겨받기 전에 준비한 묶음은 문서가 그대로여도 `browser_stale_revision`을 받는다. 넘겨받을 때 이미 올렸으므로 회차가 generation 닫힘(4번)으로 끝나도 같다. 이어서 epoch를 올리고 snapshot을 버린다. 이전 revision의 번호 동작은 049의 `browser_stale_revision`을 받는다. 그다음 밀린 DevTools 사건을 처리하고 `CdpDocument::current`로 지금 main 문서를 읽어 `verified`에 둔다. 주소가 `http(s)`가 아니면(넘겨받은 뒤 아무 데도 가지 않은 `about:blank`) `verified`를 비운다. 읽다가 049가 탭을 닫는 오류가 나면 049대로 탭이 닫히고 다음 동작은 `open`부터 한다. 이 단계의 실패는 돌려주기를 막지 않는다.
    7. 회차 기록을 지우고 상태를 `assistant`로 바꾼 뒤 시간을 다시 센다. `last_used = now`이고, 그 generation 세션의 turn이 돌고 있지 않으면 `waiting_since = now`다(PRD 000).
    8. 끝난 회차의 기록에 적는다(Scope 2). `paused`가 켜져 있고(멈춘 `handoff`가 있고) 까닭이 `answered`·`cancelled`가 아니면 이어 가기를 기다리는 기록으로 적고 Scope 9의 알림을 낸다. 4번으로 끝난 회차도 `paused`가 켜져 있으면 같다. `paused`가 꺼져 있으면 까닭이 무엇이든 다 처리된 기록으로만 적고 알림을 내지 않는다. Scope 3의 실패로 끝난 `handoff` 회차가 그렇다. 까닭이 `failed`일 때도, 되돌리다가 4번으로 닫혀 `closed`가 된 때도 `tool_call_id`만 있고 `paused`는 꺼져 있다.
5. **화면 보기 구성:** `websockify`를 token 경로와 Origin 확인으로 띄운다. 049의 `websockify_arguments`를 바꾼다.

   ```text
   websockify --web=/usr/share/novnc --token-plugin=TokenFile --token-source=<generation root>/view-routes --auth-plugin=shacs_view_auth.SameOrigin 0.0.0.0:6080
   ```

    - 듣는 주소: `websockify`는 `0.0.0.0:6080`에, `x11vnc`는 `127.0.0.1:5900`에 듣는다(049와 같다). 두 listener는 주소가 달라서 Scope 3의 "listener 확인"이 `Any`와 `Loopback`으로 나눠 본다.
    - `view-routes`의 내용은 상태를 따른다. generation을 띄울 때는 `view` 한 줄이다. 넘겨받는 동안과 끄는 동안은 비어 있다. 단계 `open`에서는 두 줄이다. viewer가 `failed`이면 비어 있다. `websockify`는 넘겨받는 동안과 끄는 동안 떠 있지 않고, 뜨는 때는 늘 경로 파일을 쓴 뒤다.

      ```text
      view: 127.0.0.1:5900
      paste: unix_socket:<generation root>/paste.sock
      ```

    - 바꿀 때는 같은 폴더의 임시 파일(0600, `create_new`)에 쓰고 `rename`으로 바꾼다. `TokenFile`은 연결마다 파일을 다시 읽으므로 반쯤 쓴 내용을 읽지 않는다. 파일에 비밀은 없다. generation root 경로에 `:`이 없는지 확인하고, 있으면 화면 보기를 띄우지 않는다. 경로 파일을 비우는 것은 새로 token을 찾는 연결만 막는다. 이미 대상을 찾은 연결 process와 이미 이어진 연결은 막지 못한다. 그래서 전환의 보장은 경로 파일이 아니라 `websockify`를 끝내고 확인하는 것이다(Scope 3). `paste` 줄만 빼는 때(Scope 6의 10번 틀림)에는 붙여넣기 listener와 socket 파일을 먼저 없애므로 늦은 연결 process가 닿을 곳이 없다.
    - Origin 확인: `docker/shacs_view_auth.py`를 더하고 `Dockerfile`이 image의 Python module 경로에 복사한다(`root:root`, 0644). class `SameOrigin` 하나다. `authenticate`는 요청의 `Origin`에서 scheme을 뗀 `host[:port]`가 `Host`와 글자 그대로 같을 때만 지나게 하고, `Origin`이 없거나 다르면 `websockify.auth_plugins.AuthenticationError`(403)를 낸다. `view`와 `paste`의 WebSocket 연결 모두에 걸린다. 정적 파일(`shacs-view.html`, `vnc.html`)에는 걸리지 않는다. module을 import하지 못하면 `websockify`가 뜨지 않으므로 viewer는 049대로 `failed`다.
    - 보기 페이지 `docker/shacs-view.html`을 더하고 `Dockerfile`이 `/usr/share/novnc/shacs-view.html`로 복사한다(`root:root`, 0644). 정적 파일 하나이고 밖의 자원을 부르지 않는다. 내용은 `vnc.html?path=websockify%3Ftoken%3Dview&resize=scale`을 담은 iframe과 붙여넣기 칸(비밀번호 칸, 글 칸, 보내기 버튼, 상태 한 줄)이다. 비밀번호는 페이지의 memory에만 두고 주소나 저장소에 쓰지 않는다. 보낸 뒤 글 칸을 비운다. 페이지는 보낼 때마다 붙여넣기 연결을 새로 연다.
    - 보이는 주소는 `<기준 주소>/shacs-view.html`이다. 기준 주소는 Scope 12다. `view_url`과 inspect의 `view.url`이 이 주소를 쓴다.
    - `vnc.html`을 직접 열면 `?path=websockify%3Ftoken%3Dview`를 붙여야 연결된다. token이 없는 `/websockify`는 끊긴다.
    - 상태가 바뀔 때 `x11vnc`와 `websockify`가 다시 뜬다. 그때 연결된 보기는 끊기고 화면 보기 페이지가 잠깐 열리지 않는다. 사용자가 다시 연결한다. 전환은 자리 lock 아래에서 하므로 `maintenance`의 process 확인(049)은 전환이 끝난 뒤의 process를 본다. `websockify`의 pid가 바뀐 것은 실패가 아니다.
6. **붙여넣기 길:** 단계 `open` 동안만 `BrowserManager`가 `<generation root>/paste.sock`(0600, 0700 임시 폴더 안)을 듣는다. 밖에서는 `/websockify?token=paste`로만 닿는다.
    - 줄 규약(줄 끝은 `\n`, 그 밖은 UTF-8 bytes). 한 연결은 인증 한 번, 붙여넣기 한 번이다. `done`이나 `error`를 보낸 뒤 서버가 끊는다.

      ```text
      S→C  shacs-paste 1 <회차 표시> <nonce 32 hex>
      C→S  auth <hex SHA-256("shacs-paste-v1\n" + nonce + "\n" + 화면 보기 비밀번호)>
      S→C  ok            | error auth
      C→S  paste <n>     (뒤에 n bytes)
      S→C  done          | error not_user | error too_long | error invalid_text | error browser_closed
      ```

    - 인증: nonce는 연결마다 OS 난수로 새로 만든다. 그래서 다른 연결에서 엿들은 digest를 다시 보내면 틀린다. 비교는 길이가 같은 digest를 끝까지 비교한다. 틀리면 1초 뒤 `error auth`를 보내고 끊는다. 한 회차에서 10번 틀리면 그 회차의 listener를 닫고 `view-routes`에서 `paste` 줄을 뺀다(VNC 입력은 그대로 된다).
    - 크기와 시간: 줄 하나는 `\n`까지 256 bytes다. 넘으면 읽기를 멈추고 끊는다. `auth` 줄은 연결한 뒤 10초 안에, `paste` 줄은 `ok` 뒤 10초 안에, 글은 `paste` 줄 뒤 10초 안에 다 와야 한다. 넘으면 끊는다. 시간이 넘은 것은 틀린 횟수에 넣지 않는다. `n`은 글을 읽기 전에 확인하고 딱 `n` bytes만 읽는다. 동시에 2연결까지 받고, 그보다 많으면 받자마자 끊는다. HTTP 쪽의 줄 길이와 header 한도는 `websockify`(Python `http.server`)의 것을 그대로 쓴다.
    - 글: `n`은 1–20000이다. 넘으면 `too_long`이다. UTF-8이 아니거나 NUL, `\n`·`\t` 말고 다른 C0 제어 문자가 있으면 `invalid_text`다. `\r\n`은 `\n`으로 바꾼다.
    - 넣기: 글마다 자리 lock을 잡고 단계가 `open`이고 회차가 그 연결의 회차인지 다시 확인한 뒤 `CdpDocument::dispatch_input`으로 `Input.insertText` 한 번을 보낸다. 아니면 `not_user`를 보내고 끊는다. 글은 페이지에서 지금 focus가 있는 곳에 들어간다. 비밀번호 칸이어도 넣는다(사용자의 입력이다. 049의 `browser_input_denied`는 어시스턴트의 `type`에만 해당한다).
    - 글은 그 한 번의 명령에만 쓰고 저장하지 않는다. log, 상태 파일, inspect, tool 결과에 글이나 길이를 남기지 않는다.
    - 단계가 `open`이 아니면 socket도 `paste` 경로도 없어 `websockify`가 잇지 못하고 끊는다.
7. **한도와 시간:** `tools.browser.takeoverMinutes`를 더한다. 기본 15, 범위 5–60의 정수다. 범위 밖이면 다른 browser 시간과 같은 config 오류다. onboard stub에 더한다.
    - `user` 상태에서 `maintenance`는 `idleTimeoutSeconds`와 `replyWaitMinutes`를 보지 않고 회차의 기한(`단계가 open이 된 때 + takeoverMinutes`)만 본다. 지나면 그 회차를 `limit`으로 끝낸다(Scope 4). 시계는 049의 `BrowserClock`이다.
    - 단계 `open`에서 입력용 `x11vnc`나 `websockify`가 shacs-bot이 끝내지 않았는데 죽으면 그 회차를 `view_failed`로 끝낸다. 보기 전용 `x11vnc`와 `websockify`를 다시 띄우지 않고 viewer는 `failed`다. Scope 3·4가 순서대로 끝내는 것은 여기에 들지 않는다.
    - `user` 상태에서 온 `turn_started`·`turn_finished`(PRD 000)는 turn이 도는지만 적어 두고 시간에 쓰지 않는다.
8. **`handoff` 동작:** `Request`와 schema에 `handoff`를 더한다. 인자는 `reason`(1–300자, 줄바꿈 없음) 하나다.
    - 판정 뒤 tool은 `BrowserHost::takeover`(이 PRD가 더한다. 기본 구현은 `ContextUnavailable`)를 그 세션 key, channel·chat, 답이 가던 자리(Scope 2), `tool_call_id`와 함께 부른다. 성공하면 아래 질문으로 `ToolResult::AskUserInterrupt`를 돌려준다. 호출이 멈추는 것은 이 결과를 돌려줄 때뿐이다. `takeover`에 넘긴 `tool_call_id`는 받은 호출 id일 뿐이고, 회차가 단계 `open`이 되어 `takeover`가 성공으로 돌아오기 전에는 멈춘 호출이 아니다(Scope 2의 `paused`). 선택지 버튼은 없다.

      ```text
      브라우저를 넘깁니다: <까닭>
      화면 보기: <주소>
      화면 보기 비밀번호로 연결하세요. 한글이나 긴 글은 화면 아래 붙여넣기 칸을 쓰세요.
      끝나면 이 메시지에 답해 주세요(예: "다 했어"). 지금 화면을 볼 수 없으면 "나중에"라고 답해 이 단계를 넘길 수 있습니다.
      다른 대화방에서는 /browser return <회차 표시>로 돌려줄 수 있습니다.
      비밀번호나 인증 코드는 대화로 보내지 마세요.
      <n>분이 지나면 자동으로 돌려받습니다.
      ```

    - 넘기지 못하면 멈추지 않고 글 결과를 돌려준다. 그 호출의 도구 결과는 이 글 하나이고 같은 turn이 그대로 이어진다. Scope 3의 순서가 실패한 회차는 이어 가기 알림을 내지 않고 이어 가기를 기다리는 기록도 만들지 않는다(Scope 2). 되돌리다가 generation이 닫혀 회차의 까닭이 `closed`가 된 때에도 같고 글도 같다. 그래서 이미 결과가 붙은 호출에 이어 가기 inbound가 뒤따르지 않는다. 비대화형이면 판정에서 거절된다(Scope 10). 화면 보기가 열려 있지 않으면 `browser_handoff_unavailable: 화면 보기를 쓸 수 없습니다(<049 view 사유>)`, Scope 3의 순서가 실패하면 `browser_handoff_unavailable: 넘겨주지 못했습니다(<사유>)`, 그 밖은 Scope 1·3의 글이다.
    - `pending_ask_user_id`를 "답 없는 멈춤 호출" 찾기로 넓혀 `ask_user`와, 인자의 `action`이 `handoff`인 `browser` 호출을 찾고 이름을 함께 돌려준다. 재개 때 붙이는 tool 메시지의 이름은 그 호출의 이름이다.
    - 안내 메시지: 회차마다 사용자에게 가는 안내 메시지가 하나 있다. `handoff`로 연 회차는 위 질문이고 `/browser takeover`로 연 회차는 Scope 11의 답이다. 두 outbound에는 runtime만 쓰는 metadata(`browser_episode_notice`: 회차 번호)를 단다. 안내 글은 한 메시지에 들어가는 길이다(까닭 300자, 주소 200자). 전송이 글을 나눠 보냈으면 모든 조각의 id를 그 회차의 안내 메시지 id로 둔다.
    - 안내 메시지가 전달된 때: outbound를 실제로 내보내는 마지막 자리가 성공으로 끝난 뒤에만 `BrowserHost::episode_notice_delivered(회차 번호, channel 메시지 id 또는 없음)`를 부른다. `Channel::send`가 성공으로 돌아온 것은 전달이 아니다(외부 adapter는 queue에 넣기만 한다).

      | 보내는 쪽 | 전달된 때 | 메시지 id |
      |---|---|---|
      | 외부 channel(Telegram, Discord, Slack, Email, WhatsApp) | worker의 `drain_outbound`에서 재시도까지 끝나 전송이 성공한 가지(`record_delivery_state`가 `sent`를 적는 자리) | Telegram은 응답의 `message_id`, Discord는 응답의 `id`. 그 밖은 없음 |
      | WebSocket | sink에서 꺼낸 그 event를 연결에 쓰는 것이 성공한 뒤. 이를 위해 `emit`이 쓰기 결과를 돌려주게 한다. sink에 넣은 것은 전달이 아니다 | 없음 |
      | HTTP API | 그 답을 담은 응답을 다 쓰고 flush한 뒤 | 없음 |
      | 직접 CLI | 출력하고 flush한 뒤 | 없음 |

      queue에 들어가 있기만 한 outbound, `failed`로 끝난 전송, `stop`으로 그만둔 전송은 부르지 않는다. 그때 `asked`는 거짓으로 남는다. 전송 함수(`send_telegram_message` 등)는 channel이 준 메시지 id를 돌려주게 바꾼다. Telegram·Discord 전송이 성공인데 id가 없으면 실패로 다룬다(Telegram은 지금도 그렇다). host는 그 회차가 지금 열려 있을 때만 `asked`를 참으로 하고 id를 적는다. 끝났거나 모르는 회차면 아무것도 바꾸지 않는다.
    - 답글 정보: Telegram과 Discord에서만 쓴다. `TelegramInbound`·`DiscordInbound`에 "이 메시지가 답글로 가리키는 메시지 id" 자리를 더한다(Telegram `reply_to_message.message_id`, Discord `message_reference.message_id`). adapter는 그 값을 inbound metadata `reply_to_message_id`에 적는다. 다른 channel에는 답글 정보가 없는 것으로 다룬다. Slack의 `thread_ts`는 세션 key에 들어 있어 같은 thread 안의 회차를 가르지 못하므로 쓰지 않는다.
    - 도착 표시: 메시지가 처음 들어올 때 ingress가 host에서 Scope 2의 도착 표시용 값을 읽어 메시지의 metadata `browser_arrival`에 적는다(`episode`: 회차 번호 또는 없음, `asked`: 안내 메시지 전달 여부, `reply_to`: 답글이 가리키는 메시지 id 또는 없음). `reply_to`는 channel이 Telegram·Discord일 때만 그 adapter가 적은 `reply_to_message_id`에서 옮기고, 다른 channel에서는 늘 없음이다. 자리는 PRD 000 Scope 8의 "처음 받을 때" 두 곳(외부 loop의 durable 넣기 앞, `enqueue_inbound`)이고 모든 inbound에 적는다. channel이 같은 이름의 값을 실어 왔으면 덮어쓴다. durable 줄에 메시지와 함께 저장되므로 줄에서 기다리거나 다시 시작한 뒤에도 그대로다. ingress를 거치지 않는 직접 CLI 실행은 loop가 메시지를 받을 때 같은 함수로 적는다.
    - 답 맞추기: 세션에 답 없는 `handoff` 호출(`tool_call_id` T)이 있을 때 온 사용자 메시지(명령이 아닌 것)를 loop는 아래처럼 다룬다. host에는 `BrowserHost::handoff_episode(세션 key, T)`로 그 호출의 회차가 지금 열려 있는지와, 열려 있으면 그 회차의 안내 메시지 id를 묻는다. 표는 위에서부터 보고 처음 맞는 줄을 쓴다. 답글 정보는 channel·chat과 메시지 id를 함께 견준다.

      | 그 호출의 회차 | 메시지의 답글 정보(`reply_to`) | 도착 표시 | 하는 일 |
      |---|---|---|---|
      | 끝났거나 모른다 | 무엇이든 | 무엇이든 | 끝낼 회차가 없다. `browser_takeover_over`를 붙여 turn을 잇는다 |
      | 열려 있다(E) | E의 안내 메시지 | 무엇이든 | 답이다. `BrowserHost::finish_handoff(세션 key, T, E)`로 회차를 `answered`로 끝내고 `browser_handoff_answer`를 붙여 turn을 잇는다 |
      | 열려 있다(E) | 있지만 E의 안내 메시지가 아니다(지난 회차의 질문, 그 밖의 메시지, E의 안내 id를 아직 모를 때) | 무엇이든 | 답이 아니다. 회차와 세션을 바꾸지 않고 모델 turn을 만들지 않는다. 아래 첫째 안내를 그 대화방에 답하고 메시지는 기록하지 않는다 |
      | 열려 있다(E) | 없다 | `episode`가 E이고 `asked`가 참 | 답이다. 둘째 줄과 같다 |
      | 열려 있다(E) | 없다 | 그 밖(다른 회차, 없음, `asked`가 거짓) | 답이 아니다. 셋째 줄과 같되 아래 둘째 안내를 답한다 |

      ```text
      이 메시지는 다른 메시지에 단 답글이라 지금 넘겨받기의 답으로 쓰지 않았습니다. 끝나면 지금 넘겨받기 안내에 답글을 달거나 답글 없이 답해 주세요.
      ```

      ```text
      이 메시지는 넘겨받기 안내가 가기 전에 보낸 것이라 답으로 쓰지 않았습니다. 끝나면 다시 답해 주세요.
      ```

    - 이 표가 지키는 것: 답글 정보가 있는 메시지는 지금 회차의 안내 메시지에 단 답글일 때만 지금 회차를 끝낸다. 지금 회차의 id 하나와만 견주므로, 지난 회차의 질문에 단 답글은 그 회차의 기록이 남아 있든 없든 지금 회차를 끝내지 못한다. 답글 정보가 없는 메시지는 shacs-bot에 닿은 때 열려 있고 안내가 전달된 회차의 답으로만 쓰인다. channel이 늦게 전한 답글 없는 메시지는 닿은 때의 회차에 묶인다(SPEC 결정 (마), 남은 제한).

    - 답은 runtime이 해석하지 않는다. 답은 사용자 메시지로 기록되지 않으므로 classifier 요청의 사용자 메시지에 없다. 붙이는 글은 아래다.

      ```text
      browser_handoff_answer: 사용자가 답했고 브라우저를 돌려받았습니다. 다시 look하세요.
      <사용자 글>
      ```

      그 호출의 회차가 이미 끝났거나 없으면(지난 답, process가 다시 뜬 뒤의 답) 지금 회차를 건드리지 않고 아래를 붙인다.

      ```text
      browser_takeover_over: 넘겨받기는 이미 끝났습니다. 다시 look하세요.
      <사용자 글>
      ```

9. **멈춘 `handoff`를 밖에서 끝내기:** 회차가 `handoff` 답이 아닌 까닭으로 끝났고 멈춘 호출이 있으면(Scope 2의 `paused`가 켜져 있으면. `tool_call_id`가 있는 것만으로는 아니다) manager가 CLI가 넣어 둔 알림 함수를 부른다. 알림 함수는 `browser` 도구를 등록하는 오래 도는 실행이 모두 넣는다: gateway(`run`. 외부 channel이 있든 없든)와 `serve`다(`install_browser_resume_notifier`). `browser` 도구는 side-effect 도구를 켠 실행에만 등록된다. `run`은 기본으로 켜고(`--no-side-effects`를 주거나 `--allow-remote`를 `--allow-side-effects` 없이 주면 끈다), `serve`는 `--allow-api-side-effects`를 줘야 켠다. 도구가 없는 실행에서는 `handoff`가 멈출 수 없으므로 이어 갈 것이 없다. `web`이 그렇다. `web`은 `browser` 도구를 등록하지 않고(049 때와 같다. `web_ui_adapter`) 알림 함수도 넣지 않는다(2026-11-12 두 번째 독립 검토 뒤 고침. 앞서 `web`도 넣는다고 적었으나 browser host가 없어 그 호출은 아무 일도 하지 않았다). CLI는 호출이 멈춘 대화가 있는 곳에 따라 두 길 가운데 하나로 그 세션의 믿는 이어 가기 inbound를 낸다. 외부 channel의 대화방이면 durable 줄(아래 "넣기"), WebSocket 대화면 그 대화를 맡은 연결(아래 "WebSocket 대화")이다. 어느 길이든 사용자 메시지 없이 이어진다(2026-11-12 독립 검토 뒤 고침. 처음 글은 "bus가 있는 실행(gateway, api)"였다. 구현 중에 "외부 channel이 하나라도 켜진 gateway"로 좁혔던 것은 검토가 돌려보내 걷어냈다. SPEC의 2026-11-12 두 note).
    - 표식: `InboundMessage`에 private `#[serde(skip)]` 자리 `owner_accepted_browser_resume`(회차 번호, `tool_call_id`, 까닭)과 만드는 함수 `with_owner_accepted_browser_resume`, 읽는 함수를 더한다. 직렬화되지 않으므로 channel adapter, bus 밖의 입력, 저장된 payload는 이 표식을 만들 수 없다.
    - inbound: channel `system`, sender `browser`, `session_key_override`는 넘겨받은 세션, metadata는 `injected_event: "browser_takeover_end"`, 회차 번호, `tool_call_id`, 까닭이다. 내용은 빈 글이다.
    - 넣기(외부 channel의 대화방): 기록의 channel이 `websocket`이 아니면 CLI의 알림 함수는 표식을 단 inbound를 durable 줄에 work id `browser-resume-<회차 번호>`, dedupe hint `browser.resume:<회차 번호>`로 넣는다(`enqueue_inbound`). 같은 회차는 한 번만 들어간다. 줄에 넣지 못하면 거짓을 돌려주고, manager는 기록이 기다리는 동안 다음 `maintenance`에서 다시 부른다. 표식을 붙이는 곳은 inbound를 만드는 함수 하나(`browser_resume_inbound`. 이 길과 아래 "WebSocket 대화"가 함께 쓴다)와 아래 "다시 붙이기"뿐이다.
    - 다시 붙이기: durable 줄에서 꺼낼 때(`durable_dispatch.rs`) work id가 `browser-resume-`로 시작하고 dedupe hint가 `browser.resume:`에 같은 회차 번호를 이은 글이면, payload의 회차 번호·`tool_call_id`·까닭으로 표식을 다시 만든다. subagent 재진입이 쓰는 방법과 같다. 밖에서 온 inbound의 work id는 `work-`로 시작하므로 이 조건에 맞지 않는다.
    - WebSocket 대화(2026-11-12 독립 검토 뒤 더함): 기록의 channel이 `websocket`이면 durable 줄에 넣지 않는다. 줄의 일을 돌리는 외부 processor에는 `websocket` channel이 없어 답이 연결에 닿지 못하기 때문이다. 알림 함수는 (chat, 회차 번호)를 `shacs-api`의 `WebSocketRuntimeTurns`에 내놓고 거짓을 돌려준다. 그래서 manager는 기록이 기다리는 동안 `maintenance`(100ms)마다 다시 내놓는다. 외부 channel이 있든 없든 같다.
        - 건네기: `WebSocketRuntimeTurns`는 그 chat을 맡은 열린 연결 하나에만 건넨다. 연결은 열릴 때 자기 기본 chat(`default`)을 맡고, 어떤 chat의 event를 쓰면 그 chat을 맡는다. 같은 chat을 맡은 연결이 여럿이면 그 chat의 event를 가장 나중에 쓴 연결이다. 건넨 것은 그 연결이 끝낼 때까지 다시 건네지 않는다.
        - 돌리기: 연결의 처리 loop는 client frame을 기다리는 같은 자리에서 건넨 것을 받아 adapter의 `process_websocket_runtime_turn_streaming`을 부른다. CLI는 건네받은 값을 믿지 않고 host의 `pending_resume(회차 번호)`를 읽는다. 그 기록의 channel이 `websocket`이고 chat이 건네받은 chat일 때만 위 "inbound"의 표식 단 inbound를 만들어 보통 WebSocket turn과 같은 길로 돌린다. 아래 "loop의 확인"과 "정리"는 durable 줄로 온 것과 같다. 진행과 답은 그 연결의 event로 나간다.
        - 끝: turn이 끝났거나(오류 포함), 기록이 없거나 다른 곳의 것이면 그 회차 번호를 끝난 것으로 기억하고(최근 64개) 다시 건네지 않는다. turn이 오류로 끝나 호출이 닫히지 않았으면 그 대화의 다음 사용자 메시지가 Scope 8의 `browser_takeover_over`로 닫는다. 그 세션의 다른 turn이 돌고 있으면(돌리기 전에 본 때와, 자리를 잡다가 `session_busy`를 받은 때 모두) 끝난 것으로 기억하지 않고 다음 내놓기에서 다시 건넨다.
        - 연결이 없을 때: 그 chat을 맡은 열린 연결이 없으면 아무에게도 건네지 않고 기록은 그대로 기다린다. 호출은 없어지지 않는다. 연결이 그 chat을 맡으면(새 연결의 기본 chat, 또는 그 chat의 event를 쓴 연결) 다음 내놓기에서 건네져 사용자 메시지 없이 이어진다. 그 전에 그 대화의 사용자 메시지가 오면 Scope 8의 `browser_takeover_over`가 호출을 닫는다. 건넨 연결에 client frame이나 닫힘이 이미 와 있으면 그것을 먼저 처리하고 건넨 것은 되돌린다. 건네받은 연결이 돌리기 전에 닫히면 되돌려져 다른 연결에 건네진다. turn이 시작된 뒤에 연결이 끊기면 호출은 이미 이어졌고 결과는 세션 기록에 남는다. 마지막 글의 전달만 빠지며, 보통 WebSocket turn이 도는 중에 연결이 끊긴 것과 같다.
    - admission: `automation_producer::result_source`는 표식이 있는 inbound를 자동화 결과가 아닌 것으로 지나게 한다(`Ok(None)`). 표식이 없고 `injected_event`가 있는 inbound는 지금처럼 `automation result lacks an owner-accepted terminal boundary`로 거절한다. metadata 값으로 지나게 하는 길은 만들지 않는다. 다른 `injected_event` 값의 처리는 바꾸지 않는다.
    - loop의 확인: 표식이 있는지(metadata가 아니라 표식의 값), channel `system`·sender `browser`, 그 세션의 답 없는 멈춤 호출이 표식의 `tool_call_id`를 가진 `handoff`인지, host의 이어 가기를 기다리는 기록(Scope 2)에 그 번호·까닭·세션 key·`tool_call_id`가 있는지를 모두 확인한다. 하나라도 다르면 버리고 아무것도 바꾸지 않으며 turn을 시작하지 않는다. 버려진 inbound는 host의 기록을 옮기지 않는다. 맞으면 아래 글을 도구 결과로 붙여 세션에 저장하고 `BrowserHost::resume_settled(회차 번호)`를 부른 뒤 turn을 잇는다. 기다리는 기록은 8개 한도에 들지 않으므로, 이어 가기가 줄에서 기다리는 동안 다른 회차가 몇 개 끝나도 이 확인은 맞는 이어 가기를 버리지 않는다.

      | 까닭 | 글 |
      |---|---|
      | `returned` | `browser_returned: 사용자가 돌려줬습니다. 다시 look하세요.` |
      | `limit` | `browser_takeover_timeout: 넘겨받기 시간이 지났습니다. 다시 look하세요.` |
      | `closed` | `browser_closed: 넘겨받은 동안 브라우저가 닫혔습니다. 페이지를 다시 여세요.` |
      | `view_failed` | `browser_view_failed: 화면 보기가 꺼져 돌려받았습니다. 다시 look하세요.` |

    - `failed`는 이 표에 없다. `failed`는 단계 `opening`에서만 나는 까닭이고 그 회차에는 멈춘 호출이 없어 알림도 이어 가기를 기다리는 기록도 없다(Scope 2). 표의 `closed`는 `paused`가 켜진 회차가 닫힌 때에만 온다. 넘겨받는 중에 실패해 닫힌 회차의 `closed`는 오지 않는다. 그런 회차의 번호나 까닭 `failed`를 단 표식이 있는 inbound가 닿더라도 기다리는 기록에 맞는 것이 없어 위 확인에서 버려진다.
    - 답이 가는 곳: 이어진 turn의 답은 host의 이어 가기를 기다리는 기록에 있는 channel·chat(넘겨받은 대화방)과 답이 가던 자리(Scope 2. Telegram topic, Slack thread)로 보낸다. loop는 이어 가기 inbound에서 답 보낼 자리의 key를 모두 지운 뒤 기록의 값을 넣는다. inbound의 metadata에 적힌 값은 쓰지 않는다. WebSocket 대화는 turn이 도는 연결의 event로 나간다.
    - 누구의 turn인가: PRD 000의 `browser_owner_turn`은 이 표식이 있는 inbound에 참을 돌려준다. 그래서 이어진 turn이 시작할 때 `turn_started`, 끝날 때 `turn_finished`가 가고, 그 turn이 도는 동안 답 기다리는 시간은 세지 않는다. 확인에서 버려진 inbound는 turn을 시작하지 않으므로 알림도 없다. 이 inbound는 도구 결과로 붙을 뿐 사용자 메시지가 아니다. classifier 요청의 사용자 메시지에 들어가지 않고 승인이나 명시 의도를 만들지 않는다.
    - 넘겨받은 대화방의 `/browser return`도 명령이므로 답보다 먼저 처리되고 이 길로 이어진다.
    - 넘겨받은 세션의 `/stop`은 회차를 `cancelled`로 끝내고, 멈춘 `handoff` 호출에 `browser_cancelled: 실행이 중단되었습니다`를 도구 결과로 붙여 닫는다. 모델 turn은 만들지 않는다. `/new`는 회차를 `cancelled`로 끝내고 세션을 비운다. 그 세션이 generation의 세션이면 PRD 000대로 브라우저도 닫힌다.
    - 알림 함수가 없는 실행(일회 CLI, REPL)에서는 합성 inbound가 없다. 그 대화의 다음 사용자 메시지가 Scope 8의 `browser_takeover_over`로 호출을 닫는다. HTTP API는 대화형이 아니어서 `handoff`가 거절되므로 멈춘 호출이 생기지 않는다.
10. **판정:** `normalize_tool_candidate`는 `handoff`를 아는 동작으로 받고 capability 없는 action으로 만든다. 정규화 거절이 아니다. 모르는 동작이 정규화 오류로 거절되는 것은 그대로다. `ask_user` 예외 줄은 바꾸지 않는다. `handoff` 갈림은 "tool 이름이 `browser`이고, 정규화된 동작이 `handoff`이고, capability가 비어 있다"일 때만 탄다.
    - auto mode(`decide_auto_mode`): 순서는 아래다. 굵은 줄만 새로 넣는다.
        1. 정규화 거절(`blocked_by_normalization`) → 거절. 그대로다.
        2. capability가 비어 있을 때: `ask_user`는 허용, 그 밖은 `ModeBaselineDeny`. **`handoff`는 여기서 정하지 않고 아래로 내려간다.**
        3. `user_only_write` → `denied_by_user`(비밀 파일 쓰기, 사용자 deny 규칙, 기억된 거절) → 도구 거절(`preflight_refusal`) → 기억 저장소를 못 읽음(`store_unavailable_decision`). 그대로다.
        4. **`handoff` 갈림:** 대화형이 아니면 `ModeBaselineDeny`로 거절한다. 사용자 ask 규칙이 맞고 이 호출이 승인되지 않았으면 `ask_or_deny(대화형, UserRuleAsk)`다. 그 밖에는 `LocalUserInteraction`으로 허용한다(ask 규칙으로 물은 호출이 승인됐으면 그 승인으로 허용한다). 여기서 끝나므로 `unattended_browser_act_denial`, 자동 허용, classifier에는 닿지 않는다.
    - 다른 mode(`decide_permission`): 상속받은 한도 확인과 `before_mode_rules`(사용자 deny 규칙, 기억된 거절)는 그대로 먼저다. `decide_without_user_rules` 안에서 정적 거절, `ask_user` 줄 다음이고 "capability 없음 거절" 앞인 자리에 `handoff` 갈림을 둔다. 갈림 안의 순서는 기억된 거절 → 기억 저장소를 못 읽음(`store_unavailable_decision`) → 대화형이 아니면 `ModeBaselineDeny` → 정적 ask(`static_ask_blocks_approval`)이면 `ask_or_deny(대화형, StaticAskRequired)` → `LocalUserInteraction` 허용이다. mode별 판정(`plan`, `bypass_permissions`)보다 앞에서 끝나므로 mode가 결과를 바꾸지 않는다. `after_mode_rules`는 그대로라서 사용자 ask 규칙이 맞으면 허용이 질문으로 바뀌고 `dont_ask`에서는 거절이 된다.
    - 결과 표다. 048의 규칙 맞추기(`deny_rule`, `ask_rule`을 만드는 것)는 바꾸지 않는다.

      | 경우 | 대화형 | 비대화형 |
      |---|---|---|
      | 규칙 없음, `default`·`accept_edits`·`plan`·`auto`·`dont_ask`·`bypass_permissions` | 허용(`LocalUserInteraction`) | 거절(`ModeBaselineDeny`) |
      | `deny: ["browser"]`, 모든 mode | 거절(`UserRuleDeny`) | 거절(`UserRuleDeny`) |
      | 기억된 거절, 모든 mode | 거절(`RememberedDeny`) | 거절(`RememberedDeny`) |
      | `ask: ["browser"]`, `dont_ask` 말고 | 질문(`UserRuleAsk`) | 거절 |
      | `ask: ["browser"]`, `dont_ask` | 거절 | 거절 |
      | 정적 거절 | 거절 | 거절 |

    - `browser`의 다른 동작은 049 capability와 판정 그대로다. `handoff`는 ask 규칙으로 물은 때 말고는 승인을 만들지 않고 승인 cache에 들어가지 않는다.
11. **`/browser takeover`·`/browser return`·`/browser status`:** PRD 000의 `BrowserCommandArgs`에 `Takeover`와 `Return { 회차 표시(선택) }`을 더한다. PRD 000 Scope 8과 같이 turn lock과 세션의 줄 없이 처리하고 모델 turn을 만들지 않는다. `/browser`로 시작하는 글은 PRD 000의 `is_session_free`가 이미 참이므로 외부·durable ingress의 네 자리(처음 받을 때, 꺼낸 뒤, 실행, 끝난 뒤)를 그대로 쓴다. 그 세션의 turn이 돌고 있어도 줄을 서지 않고, 도는 turn을 취소하지 않으며, 세션 예약을 잡지 않는다.
    - `/browser takeover`는 Scope 3을 그 대화방의 세션 key로 한다. 답은 아래다. 거절이면 사유 글 한 줄이다.

      ```text
      브라우저를 넘겨받았습니다(<회차 표시>).
      화면 보기: <주소>
      끝나면 /browser return으로 돌려주세요. <n>분이 지나면 자동으로 돌려받습니다.
      ```

    - `/browser return`(인자 없음)은 어느 대화방에서든 된다. 답글 정보가 있으면 그것으로, 없으면 그 메시지의 도착 표시로 회차를 맞춘다. 줄에서 기다리거나 다시 시작한 뒤에 처리돼도 닿은 때의 회차에 묶인다. 표는 위에서부터 보고 처음 맞는 줄을 쓴다.

      | 답글 정보(`reply_to`) | 도착 표시의 회차 | 답 |
      |---|---|---|
      | 지금 열려 있는 회차의 안내 메시지 | 무엇이든 | `returned`로 끝내고 `브라우저를 돌려줬습니다.` |
      | 있지만 그 안내 메시지가 아니다 | 무엇이든 | 아무것도 끝내지 않는다. 아래 첫째 글 |
      | 없다 | 지금도 열려 있고 `asked`가 참 | `returned`로 끝내고 `브라우저를 돌려줬습니다.` |
      | 없다 | 지금도 열려 있고 `asked`가 거짓 | 아무것도 끝내지 않는다. 아래 둘째 글 |
      | 없다 | 없다(닿은 때 넘겨받은 상태가 아니었다) | `넘겨받은 상태가 아닙니다.` 그 뒤에 열린 회차는 건드리지 않는다 |
      | 없다 | 지금 열려 있지 않다(이미 끝났다) | `그 넘겨받기는 이미 끝났습니다.` 그 뒤에 열린 회차는 건드리지 않는다 |

      ```text
      답글로 보낸 /browser return은 답글이 가리키는 안내의 넘겨받기만 돌려줍니다. 그 넘겨받기는 지금 열려 있지 않습니다. 지금 넘겨받기를 돌려주려면 답글 없이 /browser return을 보내 주세요.
      ```

      ```text
      넘겨받기 안내가 전달되기 전에 보낸 /browser return이라 돌려주지 않았습니다. 다시 보내거나 /browser return <회차 표시>를 쓰세요. 회차 표시는 /browser status에 있습니다.
      ```

    - `/browser return <회차 표시>`는 적은 표시의 회차가 지금 열려 있을 때만 `returned`로 끝낸다. 도착 표시와 답글 정보는 보지 않는다. 회차 표시는 꼭 적어야 하는 것이 아니고, 다른 대화방이나 안내를 받지 못한 때에 쓰는 길이다. 아니면 `그 넘겨받기는 이미 끝났습니다.`를 답한다. 표시의 모양(4 hex, `-`, 숫자)이 아니면 사용법 글이다.
    - `/browser status`는 PRD 000의 세 줄에 아래를 더한다. `user` 상태에서 `답 기다리기` 줄은 `멈춤`이다.

      ```text
      조작: 어시스턴트 | 사용자
      넘겨받은 곳: <세션 key> <회차 표시> (남은 <n>분) | 없음
      화면 보기: <주소> | 꺼짐(<049 view 사유>)
      ```

    - 사용법 글은 `사용법: /browser status | takeover | return [회차] | close`가 된다. `/help`도 맞춘다.
12. **`tools.browser.view.url`:** `BrowserViewConfig`에 `url`(기본 빈 글)을 더한다. 보여 주기만 하는 기준 주소다. port를 열거나 연결을 만들지 않고 `websockify` 인자에 들어가지 않는다. Origin 확인(Scope 5)에도 쓰지 않는다.
    - 받는 모양: `http://` 또는 `https://`, host, 선택 port, 선택 끝 `/`. 사용자 정보, 경로, query, fragment가 있거나 200자를 넘거나 ASCII가 아니면 쓰지 않는다. 그때는 기본 주소를 보이고 inspect에 `view.url_ignored: true`를 둔다. 브라우저 시작을 막지 않는다.
    - 기준 주소는 값이 있으면 그 값, 없으면 049의 `http://127.0.0.1:<6080|6081>`이다. 보이는 주소는 여기에 `/shacs-view.html`을 붙인다.
13. **tool 설명과 auto mode 안내:** PRD 000의 설명에서 "There is no way to enter them directly yet: say so, list what remains, and stop." 한 문장을 아래로 바꾼다.

    ```text
    For logins, passwords, verification codes, CAPTCHAs or identity checks, and when the user asks to take over the browser, call handoff with a short reason; the user then works on the screen viewer. Any reply to handoff returns the browser: read it, and if the user postponed, do not hand off again; finish what you can and list what remains. While the user controls the browser every action returns browser_user_control. After control returns, look again; earlier element numbers are invalid.
    ```

    PRD 000의 안내 줄 뒤에 `\n`으로 아래 한 줄을 붙인다. `browser` tool이 있을 때만이다.

    ```text
    For login, passwords, CAPTCHAs or identity checks in the browser, use the browser tool's handoff action; use it too when the user asks in words to take over the browser. If they postpone, do not hand off again: do what you can and tell them what remains.
    ```

14. **inspect와 diagnostics:** `BrowserStatus`에 조작 상태(`assistant`·`user`), `takeover_minutes`(설정값), 넘겨받은 중일 때의 기한을 더한다. 넘겨받은 세션 key, 회차 번호와 표시, 붙여넣은 글은 넣지 않는다. `view.url`은 Scope 5의 주소다. owner 검증에 실패하면 049처럼 live 값을 주장하지 않는다.
15. **049 시험과 스크립트 맞추기:** `websockify` 인자와 주소가 바뀌므로 049의 Cargo 시험(`spec049_browser_viewer.rs`의 `viewer_arguments_and_password_file_cleanup`, `viewer_lifecycle_and_owner_inspect`)과 `scripts/spec049-viewer-fixtures/`의 `viewer_check.py`·`host_check.py`를 새 인자(`TokenFile`, `shacs_view_auth.SameOrigin`), `/websockify?token=view`, `/shacs-view.html`에 맞춘다. fixture의 WebSocket client는 `Host`와 같은 `Origin`을 보낸다. 시험과 스크립트는 코드이므로 고친다. 049 스펙 문서는 고치지 않는다. 사용자 승인 뒤 `scripts/spec049-viewer-check.sh --approved`를 다시 돌려 보기 전용이 그대로인지 확인한다. `viewer_listener`가 듣는 주소를 받게 바뀌므로(Scope 3의 "listener 확인") 049 시험의 가짜 실행기 셋(`spec049_browser_viewer.rs`의 `ViewerRunner`, `spec049_browser_runtime.rs`의 둘)도 새 인자를 받게 고친다. `ViewerRunner`는 지금 `5900`이 아닌 port에 늘 `Absent`를 답한다. `(Loopback, 5900)`의 답은 그대로 두고, `(Any, 6080)`에는 가짜 `websockify`가 떠 있는 동안에만 `OwnedByChild`를 답하게 한다. 지금처럼 늘 `Absent`이면 `websockify` listener 확인이 들어간 경로가 모두 실패한다.
16. **문서:** `docs/USAGE.md`와 README의 브라우저 설명에 넘겨받고 돌려주는 방법(말로, `/browser takeover`·`/browser return [회차]`), 새 화면 보기 주소와 붙여넣기 칸, `takeoverMinutes`, `view.url`, 밖에서 여는 방법(개인 VPN, `ssh -L 6080:127.0.0.1:6080 <host>`)과 그 위험(암호화되지 않는 연결, 비밀번호를 아는 사람은 넘겨받은 동안 조작할 수 있음, 엿들은 사람은 비밀번호를 짐작해 볼 수 있음), `Host`를 바꾸는 proxy 뒤에서는 연결이 거절된다는 것, 새 창에 기대는 로그인은 되지 않을 수 있다는 것, 넘겨받기 안내보다 먼저 보낸 메시지는 답으로 쓰이지 않는다는 것, Telegram·Discord에서 답글로 답할 때는 지금 넘겨받기 안내에 달아야 하고 지난 안내에 단 답글은 쓰이지 않는다는 것, 답글 없이 보낸 메시지는 shacs-bot에 닿은 때의 넘겨받기에 묶인다는 것, 전환 때 화면 보기가 잠깐 닫혀 다시 연결해야 한다는 것, image를 다시 build해야 한다는 것을 적는다.
17. **실제 Docker 확인(opt-in):** `scripts/spec050-takeover-check.sh --approved`와 `scripts/spec050-takeover-fixtures/`를 더한다. 사용자 승인 뒤에만 돌린다. `--approved`가 없으면 아무것도 하지 않고 끝난다.
    - 준비는 `scripts/spec049-viewer-check.sh`와 같다. `mktemp -d`의 일회용 data 폴더와 고유 compose project를 쓰고 사용자의 data 폴더와 `docker-compose.yml`은 건드리지 않는다. 고정 test 페이지는 별도 container(`172.30.250.2`)이고 `tools.ssrfWhitelist`에 그 `/32`만 넣는다. provider는 loopback의 fixture다. 화면 보기 비밀번호는 임시 `.env.json`에만 있고 출력하지 않는다. 끝나면 `down --volumes --remove-orphans --rmi local`로 지운다.
    - gateway role(`127.0.0.1:6080`)에서 WebSocket channel로 명령을 보내며 아래를 확인한다(수용 조건 5의 첫 항목).
        - `/browser takeover`의 답에 `http://127.0.0.1:6080/shacs-view.html`이 있고 그 주소가 200이다.
        - `/websockify?token=view`의 RFB 연결로 보낸 클릭과 키가 test 페이지 입력 칸의 값을 바꾼다.
        - `/websockify?token=paste`로 정해 둔 한글 문장(받침 있는 글자, 띄어쓰기, 문장 부호 포함)을 보내면, 돌려준 뒤 `look`으로 읽은 입력 칸의 값이 그 문장과 글자 하나까지 같다.
        - 틀린 비밀번호의 붙여넣기는 `error auth`이고 값이 바뀌지 않는다. 앞 연결의 digest를 새 연결에 다시 보내도 `error auth`다.
        - `Origin`이 없거나 `Host`와 다른 WebSocket 연결은 `view`와 `paste` 모두 403으로 거절된다. 정적 페이지는 그대로 200이다.
        - `x11vnc`의 실행 인자에 `-viewonly`가 `user` 상태에서만 없고, `-nosel -noclipboard -noremote`는 두 상태 모두 있다.
        - `view-routes`가 `assistant` 상태에서 `view` 한 줄, `user` 상태에서 두 줄이다. `assistant` 상태의 `token=paste` 연결은 이어지지 않는다.
        - 넘겨받을 때와 돌려줄 때 `websockify`의 pid가 바뀌고, 앞 `websockify`의 process group에 남은 process가 없다. 연결을 하나 열어 둔 채 `/proc`에서 그 연결 process가 `websockify`와 같은 process group인지 본다. 전환 전에 열어 둔 `view` WebSocket은 전환 뒤 끊겨 있고, 그 연결로 보낸 클릭과 키가 입력 칸을 바꾸지 않는다.
        - `user` 상태에서 보낸 RFB `ClientCutText`가 X의 `PRIMARY`·`CLIPBOARD`·`CUT_BUFFER0`을 바꾸지 않고, `ServerCutText`가 오지 않는다.
        - `/browser return` 뒤 새 RFB 연결의 클릭·키와 붙여넣기 연결이 입력 칸을 바꾸지 않는다. `paste.sock`이 없다. 그 뒤 주소를 다시 열지 않고 부른 `look`이 로그인해 둔 그 페이지를 읽는다.
        - `/browser status`와 `/browser close`가 답한다(PRD 000의 명령을 실제 container에서 한 번 본다).
    - api role에서 `/browser status`의 주소가 `http://127.0.0.1:6081/shacs-view.html`인지 한 번 본다.

## Non Scope

- 로그인 유지와 프로필(PRD 020).
- 휴대전화용 화면, 보기 페이지에서 직접 치는 한글 조합, 브라우저 → 보기 페이지 클립보드.
- 화면 보기를 host의 `127.0.0.1` 밖에 여는 것, TLS, VPN이나 터널을 만들거나 확인하는 것.
- 넘겨받은 동안 창을 여럿 두는 것, 파일 올리기, download 허용. 049의 문서 지키기를 바꾸지 않는다. 049는 페이지가 새 창을 열면 그 창을 지금 탭으로 삼고 앞 탭을 닫는다. 넘겨받은 동안에도 같다. 그래서 새 창이 앞 창에 결과를 돌려주는 로그인(`window.opener`에 기대는 일부 OAuth)은 끝나지 않는다. 같은 탭에서 주소를 옮겨 가는 로그인은 된다.
- 넘겨받기가 끝났다는 것을 대화방에 스스로 알리는 메시지(멈춘 `handoff`를 잇는 것 말고).
- 승인이나 명시 의도를 만드는 것. 돌려받은 뒤의 동작은 049 판정 그대로다.
- 답이나 `/browser return`에 회차 표시를 꼭 적게 하는 것.
- Telegram·Discord 밖의 channel(Slack, Email, WhatsApp, WebSocket, API)에서 답글 정보로 회차를 맞추는 것. 이 channel의 메시지는 모두 답글 정보가 없는 것으로 다룬다.
- 끝난 회차의 기록을 디스크에 두어 process가 다시 시작한 뒤에도 이어 가기를 잇는 것(남은 제한).

## Parent Requirement Mapping

- [050 SPEC](../SPEC.md) 소유 범위 1 중 비밀 단계를 `handoff`로 넘기기, 3, 4, 5(PRD 000 몫 빼고), 6, 9 중 조작 상태와 밖에서 화면 보기 안내를 소유한다. 수용 조건 2, 3, 5의 첫 항목, 6(자기 변경)을 소유한다.
- SPEC "앞선 스펙에서 바꾸는 것" 중 쓰지 않으면 닫히는 시간 행의 넘겨받기 몫, `handoff` 행, 화면 보기 입력·클립보드 행, 048·022 판정 예외 행, 048 안내 행의 둘째 줄, 그리고 이 PRD의 결정으로 더한 여섯 행(049 PRD 040의 `websockify` 인자·주소와 전환 때 다시 띄우는 것, 049 PRD 040의 `x11vnc`·`websockify`를 generation에 한 번 띄우는 것과 비밀번호를 들고 있지 않는 것과 listener 확인이 `127.0.0.1`의 `5900`만 보는 것, 049 PRD 030의 묶음 digest에 조작 판 번호를 넣는 것, 012의 외부 channel 전송 결과를 runtime에 알리는 것과 Telegram·Discord inbound의 답글 정보, 013의 멈춘 호출 재개가 `ask_user`만 보는 것, 033의 표식 없는 `injected_event`를 거절하는 것)을 구현한다. PRD 000이 더한 012 행(`/browser`는 세션의 줄을 서지 않는다)은 소비한다.
- 수용 조건 2의 마지막 줄은 이렇게 채운다. 입력은 서버가 강제한다(`user` 상태에서만 `-viewonly` 없는 `x11vnc`가 돌고 화면 보기 경로가 열린다). 보기 페이지 → 브라우저 글은 인증을 지나는 붙여넣기 길로만 간다. 둘 다 `user` 상태에서만 받는다. `x11vnc`의 selection은 두 상태 모두 꺼 둔다(`-nosel -noclipboard`). SPEC 소유 범위 4가 PRD에 맡긴 "넘겨받은 동안에만 쓰이는 UTF-8 붙여넣기 길"이 이 길이다.
- SPEC 소유 범위 6의 "사용자가 그 질문에 답하면 답의 내용과 상관없이 돌아온다"와 소유 범위 3·6의 "지난 회차의 답이나 돌려주기는 다른 회차를 끝내지 못한다"는 SPEC 결정 기록 (마)가 정한 범위대로 Scope 8의 답 맞추기와 Scope 11의 표로 채운다. 답글 정보가 있으면 지금 회차의 안내 메시지에 단 답글만 지금 회차를 끝낸다. 답글 정보가 없으면 shacs-bot에 닿은 때 열려 있고 안내가 전달된 회차의 것이다. 안내가 전달되기 전에 닿은 메시지는 그 질문의 답이 아니다. 이 범위는 SPEC이 정한 것이고 이 PRD가 따로 둔 예외가 아니다. 사용자가 (마)를 다르게 정하면 SPEC과 이 PRD를 함께 고친다.
- 049 문서는 고치지 않는다.

## Required Contract

1. `user` 상태에서 `browser`의 어떤 동작도 실행되지 않고 DevTools 명령을 만들지 않으며, 어시스턴트는 화면을 캡처하지 않는다.
2. 입력을 받는 `x11vnc`는 상태가 `user`가 된 뒤에만 뜬다. `assistant` 상태에서 도는 `x11vnc`는 `-viewonly`로 뜬 process뿐이다. `assistant`로 바꾸기 전에 입력을 받던 process가 끝났음을 확인하고, 확인하지 못하면 브라우저를 닫는다. 넘겨받기가 중간에 실패해도 같다.
3. 화면 보기 port로 입력 process나 붙여넣기 listener에 닿는 경로는 그 listener가 shacs-bot이 띄운 process의 것임을 확인한 뒤에만 열린다. `x11vnc`를 바꾸기 전에 `websockify`와 그 연결 process가 모두 끝난 것을 확인한다. 그래서 앞 상태에서 대상을 찾아 둔 연결이 바뀐 뒤의 listener나 그 port를 차지한 다른 process에 닿지 못한다. 확인하지 못하면 브라우저를 닫는다. listener를 볼 때 `x11vnc`는 `127.0.0.1:5900`, `websockify`는 `0.0.0.0:6080`으로 주소를 가려 본다.
4. `x11vnc`는 원격 제어(`-noremote`)와 X selection(`-nosel -noclipboard`)을 어느 상태에서도 열지 않는다. 브라우저 → 보기 페이지로 가는 글이 없다.
5. 붙여넣기는 `user` 상태와 그 회차, 화면 보기 비밀번호를 모두 확인한 뒤에만 들어가고, 글은 UTF-8 그대로 `Input.insertText` 한 번으로 간다. 글을 저장하거나 log에 남기지 않는다. 읽는 크기와 기다리는 시간에 한도가 있다.
6. host에 여는 port는 049의 화면 보기 port 하나 그대로다.
7. 회차는 한 번만 끝난다. 답, 돌려주기, 한도, 알림은 자기가 가리키는 회차만 끝내고 다른 회차를 바꾸지 못한다. 답글 정보가 있는 메시지는 지금 회차의 안내 메시지에 단 답글일 때만 지금 회차를 끝낸다. 답글 정보가 없는 메시지는 shacs-bot에 닿은 때 열려 있고 안내가 전달된 회차만 끝낸다(SPEC 결정 (마)). 안내가 전달됐다고 보는 것은 전송이 실제로 성공으로 끝난 때뿐이다. queue에 들어간 것과 실패한 전송은 전달이 아니다.
8. 넘겨받은 동안 049의 닫히는 시간과 답 기다리는 시간은 세지 않고, 한도가 지나면 돌아온다. 이어진 `handoff` turn이 도는 동안에도 답 기다리는 시간은 세지 않는다.
9. `handoff`의 답과 넘겨받기·돌려주기·이어 가기 inbound는 승인을 만들지 않고 classifier의 사용자 메시지가 되지 않는다. 넘겨받기 전에 준비한 묶음은 `look`·`capture`·`open`을 포함해 어느 동작이든 문서가 그대로여도 돌아온 뒤 실행되지 않는다. 돌아온 뒤 이전 번호는 쓸 수 없고, 같은 페이지는 다시 열지 않고 `look`할 수 있다.
10. `handoff`는 대화형 실행에서만 허용되고, 사용자 deny 규칙과 기억된 거절이 먼저다. `browser`의 다른 동작의 판정은 바뀌지 않는다.
11. 이어 가기 inbound는 runtime이 붙인 표식이 있을 때만 받는다. 밖에서 온 메시지는 metadata로 그것을 흉내 낼 수 없고, 표식 없는 `injected_event`의 거절은 그대로다. 이어 가기를 기다리는 회차의 기록과 답 보낼 곳은 그 이어 가기가 처리될 때까지 버리지 않는다. 넘겨받는 중에 실패한 `handoff`는 보통 도구 결과 하나로 끝나고, 되돌리다가 브라우저가 닫힌 때를 포함해 이어 가기 inbound와 이어 가기를 기다리는 기록을 만들지 않는다.
12. 화면 보기가 `open`이 아니면 넘겨받지 않는다.
13. Cargo 시험은 가짜 DevTools, 가짜 process 실행기, 가짜 provider, 시험용 시계만 쓰고 `Cargo.lock`은 바뀌지 않는다.

## Acceptance Criteria

1. `crates/shacs-cli/tests/it/spec050_browser_takeover.rs`의 `takeover_blocks_every_action_without_devtools_traffic`: 실제 `BrowserManager`, 가짜 process 실행기, 가짜 DevTools로 넘겨받은 뒤 열한 동작 모두가 `browser_user_control`을 받고 그 호출들 사이에 가짜 DevTools가 받은 명령이 늘지 않는지 확인한다.
2. 같은 파일의 `takeover_enters_user_state_before_input_viewer_and_opens_routes_last`: 가짜 process 실행기가 단계마다 상태를 적게 해서 순서가 Scope 3과 같은지 확인한다. 상태가 `user`가 되고 조작 판 번호가 오른다 → `view-routes`가 빈다 → `websockify`가 끝나고 세 확인(직접 process가 거둬짐, process group에 산 process 없음, 화면 보기 port에 listener 없음)이 끝난다 → 보기 전용 `x11vnc`가 끝난다 → 입력용 `x11vnc`가 뜬다 → listener 확인과 붙여넣기 listener가 끝난다 → `view-routes`에 두 줄이 생긴다 → 새 `websockify`가 뜨고 그 listener가 확인된다 → 단계가 `open`이 되고 기한이 그때부터다. `websockify`의 세 확인이 끝나기 전에는 어느 `x11vnc`도 끝나거나 뜨지 않는지, 입력용 `x11vnc`가 떠 있는 동안 새 `websockify`가 뜨기 전에는 화면 보기 port에 listener가 없는지도 본다.
3. 같은 파일의 `takeover_failure_after_input_spawn_turns_input_off`: 입력용 `x11vnc`를 띄운 뒤의 실패 다섯(listener가 `Foreign`, 비밀번호 파일이 남음, 붙여넣기 socket을 열지 못함, `view-routes`를 쓰지 못함, 새 `websockify`의 listener가 확인되지 않음) 각각에서 `websockify`와 입력 process가 끝난 것이 확인된 뒤에야 상태가 `assistant`인지, `view-routes`에 `paste` 줄이 없는지, 보기 전용 `x11vnc`와 `websockify`가 새로 떴고 `view-routes`가 `view` 한 줄인지, 넘겨받기가 사유와 함께 거절되는지 확인한다. 실패한 뒤 그 세션의 `look`이 평소대로 실행되는지도 본다. 다섯 경우 각각을 `handoff`로 시작한 넘겨받기(`tool_call_id`가 있음)로도 돌려, 그 회차가 까닭 `failed`로 다 처리된 기록에만 남고 `paused`가 꺼져 있으며, 알림 함수가 한 번도 불리지 않고 이어 가기를 기다리는 기록이 비어 있는지 확인한다.
4. 같은 파일의 `unconfirmed_input_shutdown_disposes_generation`: 가짜 process 실행기가 입력용 `x11vnc`를 거두지 못하게 하거나 `5900` listener가 남아 있다고 답하게 하거나, `websockify`의 process group에 연결 process가 남아 있다고 답하게 한다. `/browser return`, 한도, `handoff` 답, 입력 process를 띄운 뒤의 넘겨받기 실패(되돌리는 중에 난 실패), 보기 전용 process가 끝난 것을 확인하지 못한 넘겨받기 실패 각각에서 시험용 시계로 2초 뒤 generation 전체가 닫히는지, 회차의 까닭이 `closed`이고 PRD 000의 `browser_closed` 알림이 남는지, 보기 전용 `x11vnc`와 `websockify`를 다시 띄우지 않았는지, 연결 process가 남은 경우에는 generation이 닫히기 전에 어느 `x11vnc`도 끝내거나 띄우지 않았는지, 멈춘 `handoff`가 있으면(`paused`가 켜져 있으면) `closed` 알림 함수가 불리는지 확인한다. 넘겨받기 실패 두 경우는 `handoff`로 시작했어도(`tool_call_id`가 있어도) 알림 함수가 불리지 않고 이어 가기를 기다리는 기록이 없으며, 회차가 까닭 `closed`로 다 처리된 기록에만 남는지 확인한다. 실행기가 적어 둔 모든 시점에서 "상태가 `assistant`인데 `-viewonly` 없는 `x11vnc`가 살아 있다"가 한 번도 없는지 본다.
5. 같은 파일의 `return_invalidates_numbers_and_keeps_page_readable`: 넘겨받기 전에 `look`으로 받은 번호의 `click`과 넘겨받기 전에 `prepare`로 받은 digest의 실행이 돌려준 뒤 `browser_stale_revision`을 받는지 확인한다. 문서가 바뀌지 않은 경우를 따로 본다. 가짜 DevTools가 넘겨받은 동안과 돌려준 뒤 navigation 사건을 하나도 내지 않고 target·frame·loader·url이 그대로인 페이지에서, 넘겨받기 전에 준비한 `look`·`capture`·`scroll`·`close`와 문서 없이 준비한 `open`을 돌려준 뒤 실행하면 모두 `browser_stale_revision`이고 그 실행들이 DevTools 명령과 캡처 파일을 만들지 않는지, 돌려준 뒤 새로 준비한 같은 동작은 실행되는지, 같은 문서와 인자에서 조작 판 번호만 다른 두 digest가 서로 다른지 확인한다. 넘겨받은 동안 generation이 닫히고 새 generation이 뜬 뒤에도 넘겨받기 전에 준비한 `open`이 `browser_stale_revision`인지 본다. 같은 페이지에서 `look`의 `prepare`가 관찰 묶음을 만들고 실행이 그 페이지를 읽는지(`open`을 다시 부르지 않는다), 넘겨받은 동안 다른 `http(s)` 주소로 옮겨 갔을 때 새 주소가 읽히는지, `about:blank`에 머문 채 돌려주면 `look`이 049의 "열린 문서 없음" 결과를 받는지, 지금 문서를 읽다 실패해도 상태가 `assistant`로 돌아오는지 확인한다.
6. 같은 파일의 `viewer_arguments_follow_control_state`: `x11vnc` 인자가 두 상태에서 `-viewonly` 하나만 다르고 `-noremote -nosel -noclipboard`가 늘 있는지, `websockify` 인자가 Scope 5와 같고 고정 대상(`127.0.0.1:5900`)이 인자에 없는지, `view-routes`의 내용이 띄울 때 `view` 한 줄, 전환 중 빈 내용, 단계 `open`에서 두 줄, 돌려준 뒤 `view` 한 줄, viewer `failed`에서 빈 내용인지, 바꾸는 중에 반쯤 쓴 파일이 보이지 않는지(`rename`), 넘겨받을 때와 돌려줄 때마다 `websockify`가 새 process로 뜨고 그 사이에는 떠 있지 않은지, 비밀번호 파일이 띄울 때마다 새로 생기고 없어지는지, viewer의 `Debug`에 비밀번호가 없는지, generation root에 `:`이 있으면 화면 보기를 띄우지 않는지, 보이는 주소가 `<기준 주소>/shacs-view.html`인지 확인한다.
7. 같은 파일의 `image_carries_view_page_and_origin_check`: `Dockerfile`이 `docker/shacs-view.html`과 `docker/shacs_view_auth.py`를 Scope 5의 자리에 복사하는지, 두 파일이 있는지, 보기 페이지가 밖의 자원 주소를 담지 않고 `path=websockify%3Ftoken%3Dview`를 담는지 글로 확인한다. `SameOrigin`이 실제로 거절하는지는 Cargo 시험으로 보지 못하고 30번의 Docker 확인이 본다.
8. 같은 파일의 `takeover_limit_returns_and_pauses_browser_timers`: 시험용 시계로 넘겨받은 동안 `idleTimeoutSeconds`와 `replyWaitMinutes`가 지나도 닫히지 않는지, `takeoverMinutes`가 지나면 회차가 `limit`으로 끝나 `assistant`로 돌아오는지, 돌아온 뒤 시간을 다시 세는지, 넘겨받은 동안 입력용 `x11vnc`나 `websockify`가 죽으면 `view_failed`로 끝나고 viewer가 `failed`인지, Scope 3·4가 순서대로 끝낸 `x11vnc`·`websockify`는 `view_failed`를 만들지 않는지 확인한다. `takeoverMinutes`의 기본 15와 범위(5와 60은 받고 4와 61은 오류)는 13번의 config 시험이 본다.
9. 같은 파일의 `episode_ends_once_and_ignores_stale_endings`: 답·돌려주기·한도가 같은 때 와도 Scope 4가 한 번만 도는지, 지난 회차 번호를 가리키는 `finish_handoff`·돌려주기·한도·보기 실패가 지금 회차와 상태를 바꾸지 않는지, 다 처리된 기록이 8개까지 남고 번호·까닭·세션 key·channel·chat을 담는지, 이어 가기를 기다리는 기록은 그 8개에 들지 않고 `resume_settled`·`/stop`·`/new` 뒤에야 옮겨지는지, `user` 상태의 `/browser close`와 그 세션의 `/new`가 회차를 `closed`·`cancelled`로 끝내는지 확인한다.
10. 같은 파일의 `takeover_start_rules`: 화면 보기가 꺼졌거나, 비밀번호가 없거나, viewer가 `failed`이면 049의 view 사유와 함께 거절되고 process·상태·회차가 그대로인지(SPEC 수용 조건 2의 넷째 항목), 브라우저가 없을 때의 `/browser takeover`가 process gate를 지나 `about:blank` 한 탭으로 뜨는지, 동작이 실행 중이면 그 동작이 끝난 직후 넘겨받고 `pageTimeoutSeconds + 5초`를 넘으면 `browser_busy`인지, 다른 세션 generation의 `handoff`는 `browser_busy`이고 `/browser takeover`는 generation의 세션을 바꾸지 않고 넘겨받는지, 이미 `user` 상태면 거절되는지 확인한다. inspect와 diagnostics에 조작 상태·`takeover_minutes`·기한이 있고 세션 key·회차 번호가 없는지도 본다.
11. `crates/shacs-cli/tests/it/spec050_browser_paste.rs`의 `paste_inserts_utf8_text_once_in_user_state`: 임시 폴더의 `paste.sock`에 직접 연결해, 맞는 digest 뒤 보낸 한글 문장(받침, 띄어쓰기, 문장 부호, `\r\n` 포함)이 가짜 DevTools에 `Input.insertText` 한 번으로 가고 글이 같은지(`\r\n`만 `\n`), 비밀번호 칸에 focus가 있어도 들어가는지, 한 연결에서 둘째 `paste`를 받지 않는지, socket이 0600이고 단계 `open` 밖에서는 없는지, log·상태 파일·inspect에 글과 길이가 없는지 확인한다.
12. 같은 파일의 `paste_rejects_bad_auth_replay_and_oversize`: 틀린 digest가 1초 뒤 `error auth`를 받고 넣기가 없는지, 맞게 지난 연결의 `auth` 줄을 새 연결에 그대로 다시 보내면 `error auth`인지(nonce가 다르다), 10번 틀리면 listener가 닫히고 `view-routes`에서 `paste` 줄만 빠지며 `view` 줄과 `user` 상태는 그대로인지, 256 bytes를 넘는 줄에서 더 읽지 않고 끊는지, `paste 20001`이 글을 읽기 전에 `too_long`인지, UTF-8이 아닌 글·NUL·제어 문자가 `invalid_text`인지 확인한다.
13. 같은 파일의 `paste_connections_have_deadlines_and_a_cap`: 시험용 시계로 `auth` 줄, `paste` 줄, 글 각각이 10초 안에 오지 않으면 끊기고 틀린 횟수가 늘지 않는지, 셋째 동시 연결이 바로 끊기는지, 인증한 뒤 회차가 끝난 연결의 `paste`가 `not_user`이고 넣기가 없는지, 브라우저가 닫힌 뒤에는 `browser_closed`인지 확인한다. 그리고 `crates/shacs-config/tests/it/spec050_browser_config.rs`에 `takeover_minutes_and_view_url`(기본 15와 범위, `view.url`이 받는 모양과 받지 않는 모양, `url_ignored`, onboard가 기존 값을 보존)을, `crates/shacs-command/tests/router.rs`에 `/browser takeover`·`/browser return`·`/browser return 9f2c-3`·모양이 틀린 회차 표시의 풀이를 보탠다.
14. `crates/shacs-core/tests/it/spec050_handoff.rs`의 `handoff_pauses_turn_and_any_answer_returns_control`: 실제 `AgentLoop` turn, 외부 channel, 가짜 provider, 가짜 host로 "브라우저 넘겨줘"에 가짜 provider가 `handoff`를 부르면 turn이 멈추고, 질문에 까닭, 화면 보기 주소(`tools.browser.view.url`이 있으면 그 주소), 회차 표시, "나중에" 안내, 비밀번호를 대화로 보내지 말라는 안내, 한도 분이 있고 선택지 버튼이 없는지 확인한다. 시험은 메시지에 도착 표시를 미리 달지 않고, 가짜 host가 질문 전달을 알린 뒤에 메시지를 보낸다. 답글 없는 "다 했어"와 "나중에", 그 질문에 답글로 단 "다 했어" 각각이 `finish_handoff(세션 key, T, E)`를 부르고, 같은 turn이 이름 `browser`의 tool 메시지(`browser_handoff_answer`와 그 글)로 이어지며, 그 답이 사용자 메시지로 기록되지 않고 뒤의 classifier 요청의 사용자 메시지에 없는지 확인한다. 돌려받은 뒤의 `click`이 auto mode에서 classifier 요청을, `default`에서 승인 질문을 만드는지, 화면 보기를 쓸 수 없으면 멈추지 않고 `browser_handoff_unavailable`이 돌아오는지 확인한다(SPEC 수용 조건 3).
15. 같은 파일의 `handoff_resumes_from_other_room_return_and_limit`: 표식을 단 inbound를 bus로 넣어 까닭 `returned`·`limit`·`closed`·`view_failed` 각각에서 멈춘 turn이 Scope 9 표의 글을 도구 결과로 받아 이어지는지, 답이 host의 이어 가기를 기다리는 기록에 있는 대화방으로 가고 inbound metadata에 적어 둔 다른 대화방으로 가지 않는지 확인한다. 표식 없이 같은 metadata만 단 inbound, `tool_call_id`가 다른 것, host가 모르는 회차 번호, 세션 key가 다른 것, sender가 `browser`가 아닌 것 각각이 버려져 turn이 시작되지 않고 세션이 그대로인지 확인한다. 받은 이어 가기에서는 host가 `turn_started`와 `turn_finished`를 한 번씩 받고 버려진 것에서는 받지 않는지, 이어 가기 inbound가 classifier 요청의 사용자 메시지에 없고 승인을 만들지 않는지 확인한다.
16. 같은 파일의 `stop_and_new_cancel_paused_handoff`: 넘겨받은 세션의 `/stop`이 회차를 `cancelled`로 끝내고 멈춘 호출에 `browser_cancelled`를 붙여 닫으며 provider 요청을 만들지 않는지, `/new`가 회차를 `cancelled`로 끝내고 세션을 비우며 `close_for_session`을 부르는지, 그 뒤에 온 그 회차의 이어 가기 inbound와 답이 아무것도 바꾸지 않는지 확인한다.
17. 같은 파일의 `stale_handoff_answer_does_not_end_new_episode`: 실제 `AgentLoop`에 Telegram 모양의 가짜 외부 channel을 붙인다. 이 channel은 보낸 메시지마다 id를 돌려주고, inbound에 답글이 가리키는 id(`reply_to_message_id`)를 실으며, 사용자 메시지를 붙들었다가 늦게 넣을 수 있다. 시험은 메시지에 도착 표시를 미리 달지 않는다. 늦음은 channel이 메시지를 붙드는 것으로만 만든다. H1(호출 T1, 질문 Q1)이 한도로 끝나 이어 가기로 turn이 이어지고, 가짜 provider가 다시 `handoff`를 불러 같은 대화방에서 H2(호출 T2, 질문 Q2)가 열려 Q2가 전달된다. 그 뒤에 닿은 메시지를 차례로 본다.
    - Q1에 답글로 단 "다 했어"(H1 때 보냈으나 channel이 늦게 전한 것): `finish_handoff`가 불리지 않고, H2가 열린 채이며, Scope 8의 첫째 안내가 그 대화방에 가고, provider 요청과 세션 기록이 늘지 않는다. 다른 회차 9개가 더 끝나 H1의 기록이 모두 밀려난 뒤에도 같다.
    - 안내가 아닌 다른 메시지에 답글로 단 "다 했어": 같다.
    - Q2가 전달되기 전에 닿은 답글 없는 "다 했어": H2를 끝내지 않고 Scope 8의 둘째 안내가 간다.
    - Q2에 답글로 단 "다 했어": H2를 `answered`로 끝낸다.
    - Q2가 전달된 뒤에 닿은 답글 없는 "다 했어": H2를 `answered`로 끝낸다. H1 때 보냈으나 channel이 늦게 전한 것이어도 같다. 이것이 SPEC 결정 (마)가 정한 범위임을 그 경우의 이름과 주석에 적어 둔다.
    - 멈춘 호출의 회차가 이미 끝난 뒤에 온 사용자 메시지(이어 가기 inbound보다 먼저 닿은 것)는 `browser_takeover_over`로 이어지고 지금 회차를 건드리지 않는지, 그 뒤에 온 이어 가기 inbound는 버려지는지 확인한다.
18. 같은 파일의 `handoff_permission_follows_deny_rules_and_interactivity`: Scope 10의 표를 그대로 돈다. 규칙이 없을 때 `default`·`accept_edits`·`plan`·`auto`·`dont_ask`·`bypass_permissions` 각각이 대화형에서 `LocalUserInteraction` 허용, 비대화형에서 `ModeBaselineDeny`인지(비대화형 `bypass_permissions` 포함), `deny: ["browser"]`가 `auto`를 포함한 모든 mode에서 `UserRuleDeny`인지, 기억된 거절이 `RememberedDeny`인지, `ask: ["browser"]`가 대화형에서 질문이 되고 승인하면 허용되며 `dont_ask`와 비대화형에서 거절인지, 정적 거절이 거절인지 확인한다. auto mode에서 `handoff`가 classifier 요청을 만들지 않고 `unattended_browser_act_denial`에 닿지 않는지, 허용이 승인 cache에 들어가지 않는지, `ask_user`의 판정과 `browser`의 다른 열 동작의 판정이 이 PRD 전과 같은지, 모르는 동작이 정규화 거절인지 확인한다.
19. 같은 파일의 `takeover_commands_and_guidance`: `/browser takeover`·`/browser return`·`/browser return <회차 표시>`·`/browser status`·잘못된 인자가 provider 요청 없이 Scope 11의 글을 답하고 세션 기록이 그대로인지 확인한다. 인자 없는 `/browser return`이 Scope 11의 표 여섯 줄대로 답하는지(넘겨받은 상태가 아닐 때 닿은 것은 `넘겨받은 상태가 아닙니다.`, 지난 회차 때 닿은 것은 `그 넘겨받기는 이미 끝났습니다.`, 안내가 전달되기 전에 닿은 것과 다른 메시지에 단 답글은 각각의 글이고 지금 회차는 그대로, 지금 회차의 안내에 단 답글과 안내가 전달된 뒤 닿은 것만 끝낸다), `/browser return <회차 표시>`가 적은 회차만 끝내는지, 다른 대화방의 것도 받는지 확인한다. `browser` tool 설명과 auto mode 안내가 Scope 13의 글과 같고 PRD 000의 "직접 입력할 방법이 아직 없다" 문장이 없는지, `/help`와 사용법 글이 맞는지 확인한다.
20. `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `browser_resume_reaches_paused_turn_through_bus_and_rejects_forgeries`: 실제 CLI의 bus 경로(manager의 알림 함수 → `enqueue_inbound` → `automation_producer::enqueue_result` → durable 줄 → `durable_dispatch` → `AgentLoop`)와 실제 `BrowserManager`(가짜 process 실행기, 시험용 시계), 외부 channel adapter로 확인한다. 다른 대화방의 `/browser return`과 한도 초과 각각에서 멈춘 `handoff` turn이 이어지고 답이 넘겨받은 대화방으로 가는지, 넣은 뒤 줄을 디스크에서 다시 읽어 꺼내도(직렬화를 거친 payload) 표식이 다시 붙어 이어지는지, 같은 회차를 두 번 넣어도 한 번만 이어지는지, 이 inbound가 자동화 결과로 기록되지 않는지 확인한다. 꾸민 것은 모두 loop에 닿지 않는지 확인한다. 외부 channel이 보낸 metadata `injected_event: "browser_takeover_end"`와 회차 번호·`tool_call_id`를 단 메시지는 지금의 `automation result lacks an owner-accepted terminal boundary`로 거절되고, work id가 `work-`로 시작하는 줄 항목에 dedupe hint `browser.resume:<번호>`를 단 것과 work id `browser-resume-<번호>`에 다른 번호의 hint를 단 것은 표식이 붙지 않아 같은 거절을 받으며, 멈춘 turn과 회차가 그대로다. 다른 값의 표식 없는 `injected_event`가 지금처럼 거절되는지도 본다.
21. 같은 파일의 `takeover_and_return_bypass_busy_session_through_external_ingress`: 가짜 provider가 응답을 붙들어 세션의 turn이 도는 동안 같은 대화방에서 `/browser takeover`와 `/browser return`을 보낸다. 두 답이 그 turn이 끝나기 전에 가는지, 도는 turn과 줄에 선 메시지가 취소되지 않는지, 세션 파일과 예약이 그대로인지, 넘겨받은 사이 그 turn의 `browser` 호출이 `browser_user_control`을 받는지 확인한다.
22. 같은 파일의 `stale_answer_and_queued_return_stay_bound_to_their_episode`: 실제 `telegram_update_to_inbound`, 실제 ingress, 실제 outbound worker(`drain_outbound`)와 가짜 Telegram 전송으로 확인한다. 가짜 전송은 보낸 메시지마다 `message_id`를 돌려주고, update를 ingress에 닿기 전에 붙들 수 있다. 시험은 도착 표시를 미리 단 메시지를 쓰지 않는다. ingress가 처음 받을 때 도착 표시를 적는지, channel이 실어 온 같은 이름의 값을 덮어쓰는지, `reply_to_message.message_id`가 도착 표시의 `reply_to`가 되는지 먼저 본다. 늦음은 두 가지로 만든다(SPEC 수용 조건 2의 셋째 항목).
    - channel이 늦게 전한 것: H1 동안 Q1에 답글로 보낸 "다 했어"와 Q1에 답글로 보낸 `/browser return`의 update를 전송에 붙들어 둔다. H1을 한도로 끝내고 같은 대화방에서 H2를 열어 Q2가 전달된 뒤 내보낸다. 둘 다 H2를 끝내지 않고 Scope 8·11의 답글 안내를 답한다. 같은 방법으로 늦게 닿은 답글 없는 "다 했어"는 H2를 `answered`로 끝낸다(결정 (마)가 정한 범위). Q2에 답글로 보낸 `/browser return`은 H2를 끝낸다.
    - 줄에서 늦은 것: H1 동안 닿은 답글 없는 "다 했어"와 인자 없는 `/browser return`을 durable 줄에 붙들어 둔 채 H1을 한도로 끝내고 같은 대화방에서 H2를 연 뒤 내보낸다. 둘 다 H2를 끝내지 않고 Scope 8·11의 글을 답한다. 줄을 디스크에서 다시 읽은 뒤에도 같다.
23. 같은 파일의 `resumed_handoff_turn_stops_reply_wait_clock`: 실제 `BrowserManager`와 시험용 시계로, 멈춘 `handoff`의 회차가 한도로 끝난 뒤 이어진 turn에서 가짜 provider가 응답을 붙든다. 그동안 시계를 `replyWaitMinutes`보다 많이 넘겨도 브라우저가 열려 있는지, 그 turn이 끝난 때부터 `replyWaitMinutes`를 세어 닫히는지, 확인에서 버려진 이어 가기는 시간을 바꾸지 않는지 확인한다.
24. `crates/shacs-cli/tests/it/spec050_browser_takeover.rs`의 `late_websockify_worker_cannot_reach_swapped_listener`: 가짜 process 실행기가 `websockify`의 연결 process 하나를 "token `view`로 `127.0.0.1:5900`을 찾았고 아직 연결하지 않은 채 멈춤"으로 둔다. 실행기는 이 process가 `websockify`의 process group에 있다고 답하고, 시험이 원하는 때 그 process를 다시 움직여 연결을 시도하게 한다.
    - 넘겨받을 때: 보기 전용 `x11vnc`가 끝나기 전에 그 연결 process가 없어졌는지, 전환이 끝난 뒤 다시 움직이려 하면 "그런 process 없음"이고 입력용 `x11vnc`의 listener가 받은 연결이 새 `websockify`를 거친 것뿐인지 확인한다.
    - 돌려줄 때: 단계 `open`에서 같은 모양으로 멈춘 연결 process가 입력용 `x11vnc`가 끝나기 전에 없어졌는지, 돌려준 뒤 다시 움직일 수 없고 보기 전용 `x11vnc`의 listener에 닿지 않는지 확인한다.
    - 실행기가 그 연결 process를 죽지 않게 하면: 2초 뒤 generation이 닫히고, 그 전에 새 `x11vnc`가 뜬 적이 없다.
    - 보기 전용 `x11vnc`가 끝난 뒤 다른 process가 `5900`을 차지하면: 넘겨받기가 `Foreign`으로 실패하고, 그때 살아 있는 `websockify`와 연결 process가 없어 그 process에 닿은 연결이 없다.
    - token `paste`로 `paste.sock`을 찾고 멈춘 연결 process는 회차가 끝날 때 없어지고, 다음 회차의 붙여넣기 listener가 받은 연결에 없다.
25. `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `handoff_question_is_asked_only_after_outbound_delivery`: 실제 외부 channel adapter, 실제 outbound worker(`drain_outbound`), 가짜 전송, 실제 `BrowserManager`로 확인한다.
    - queue에 들어갔으나 worker가 아직 보내지 않은 질문: `Channel::send`는 성공으로 돌아왔지만 `asked`가 거짓이고, 그동안 닿은 답글 없는 "다 했어"는 회차를 끝내지 않고 Scope 8의 둘째 안내를 받는다.
    - 전송이 재시도 끝에 실패한 질문: delivery 기록이 `failed`이고 `episode_notice_delivered`가 불리지 않으며 `asked`가 거짓이다. `stop`으로 그만둔 전송도 같다.
    - 전송이 성공한 질문: 그 뒤에야 `asked`가 참이고, 회차의 안내 메시지 id가 가짜 전송이 돌려준 `message_id`와 같다. 성공 응답에 id가 없으면 실패로 다뤄진다.
    - 이미 끝난 회차나 모르는 회차의 전달 알림은 지금 회차의 `asked`와 id를 바꾸지 않는다.
    - WebSocket: event가 sink에 들어가기만 한 때는 `asked`가 거짓이고, 연결에 쓴 뒤에 참이며, 쓰기가 실패하면 거짓으로 남는다.
    - `/browser takeover`의 답도 같다. 전송이 성공한 뒤에야 그 회차의 `asked`가 참이고 id가 적힌다.
26. 같은 파일의 `unresumed_episode_record_survives_later_episodes`: 대화방 A의 `handoff` 회차 H1을 한도로 끝내고 그 이어 가기 inbound를 durable 줄에 붙들어 둔다. 그동안 대화방 B에서 `/browser takeover`와 `/browser return`으로 회차 9개를 열고 끝낸다. 붙든 것을 내보내면 A의 멈춘 turn이 `browser_takeover_timeout`으로 이어지고 답이 대화방 A로 가는지 확인한다. 이어진 뒤에는 H1의 기록이 다 처리된 기록으로 옮겨져 8개 한도를 따르는지, 같은 이어 가기를 다시 넣어도 turn이 시작되지 않는지, 멈춘 세션이 둘일 때 두 기록이 모두 남아 각각 이어지는지, 확인에서 버려진 꾸민 이어 가기가 기록을 옮기지 않는지 본다.
27. `crates/shacs-cli/src/spec050_browser_ingress_tests.rs`의 `handoff_opening_failure_gives_one_result_and_no_resume`: 위 3번의 실패를 실제 loop와 bus 경로로 본다. 구성은 위 20번과 같다(실제 `AgentLoop`, 실제 `BrowserManager`와 가짜 process 실행기·시험용 시계, 실제 CLI의 알림 함수 → `enqueue_inbound` → durable 줄 → `durable_dispatch`, 외부 channel adapter, 가짜 provider). 가짜 provider가 `handoff`를 부르고 가짜 실행기가 Scope 3의 순서를 실패시킨다. 경우는 셋이다. 입력용 `x11vnc`를 띄운 뒤 새 `websockify`의 listener가 확인되지 않음(까닭 `failed`), 보기 전용 `x11vnc`가 끝난 것을 확인하지 못함(Scope 4의 4번으로 generation이 닫히고 까닭 `closed`), 되돌리다가 입력용 `x11vnc`를 거두지 못함(같은 길로 까닭 `closed`). 각각 아래를 확인한다.
    - turn이 멈추지 않는다. 세션에 `pending_ask_user_id`가 없고, 그 `tool_call_id`의 tool 메시지가 정확히 하나이며 글이 `browser_handoff_unavailable: 넘겨주지 못했습니다(<사유>)`다. 가짜 provider가 같은 turn의 다음 요청에서 그 결과를 받는다.
    - 알림 함수가 불리지 않았고, durable 줄에 work id가 `browser-resume-`로 시작하는 항목이 없으며, host의 이어 가기를 기다리는 기록이 비어 있다. 그 회차는 다 처리된 기록에 그 까닭(`failed` 또는 `closed`)으로 있고 `paused`가 꺼져 있다.
    - turn이 끝난 뒤 시험용 시계를 `takeoverMinutes`보다 많이 넘기고 bus를 비워도 그 호출에 두 번째 tool 메시지가 붙지 않고, 새 turn이 시작되지 않으며, provider 요청이 늘지 않는다.
    - 시험이 그 회차 번호·`tool_call_id`·까닭으로 표식을 단 inbound를 직접 넣어도 loop가 버리고 세션이 그대로다.
    - 같은 세션에서 이어서 성공한 `handoff`는 평소대로 멈추고, 그 회차가 한도로 끝나면 이어 가기가 한 번 온다. 실패한 회차의 기록이 다음 회차를 막지 않는다.
28. `crates/shacs-cli/tests/it/spec050_browser_takeover.rs`의 `listener_check_reads_bind_address_port_and_socket_inode`: `viewer_listener_from`에 `/proc/net/tcp` 모양의 고정 글과 fd link 목록을 넣어 주소 값과 socket inode를 실제로 읽는지 확인한다. 가짜 실행기가 `OwnedByChild`를 돌려주는 것으로 대신하지 않는다.
    - `00000000:17C0` LISTEN 행(inode 4242)과 link `socket:[4242]`: `Any`·`6080`은 `OwnedByChild`다. 같은 표를 `Loopback`·`6080`으로 물으면 `Absent`다(049 함수 그대로는 `websockify`를 보지 못한다는 것).
    - `0100007F:170C` LISTEN 행과 맞는 link: `Loopback`·`5900`은 `OwnedByChild`, `Any`·`6080`은 `Absent`다.
    - `0100007F:17C0`만 있는 표(`websockify`가 loopback에만 들음): `Any`·`6080`은 link가 맞아도 `Foreign`이다.
    - `00000000:17C0` 행이 있는데 link의 inode가 다르거나(`socket:[4243]`), link 목록이 없으면 `Foreign`이다.
    - `00000000:17C0`(owner의 inode)과 `03FA1EAC:17C0`(`172.30.250.3`, 다른 inode) 두 행: `Foreign`이다.
    - 넷째 칸이 `0A`가 아닌 행(이어진 연결 `01`), port가 다른 행(`00000000:17C1`), 제목 줄만 있는 표는 `Absent`다.
    - inode는 열째 칸에서 읽는다. 여덟째·아홉째 칸(uid, timeout)의 수가 link의 inode와 같아도 맞는 것으로 보지 않는다.
    - 실제 `BrowserManager`의 가짜 process 실행기는 주소와 port를 함께 받아 답한다. `(Loopback, 5900)`에는 가짜 `x11vnc`, `(Any, 6080)`에는 가짜 `websockify`가 떠 있을 때만 `OwnedByChild`이고, `(Loopback, 6080)`과 `(Any, 5900)`에는 늘 `Absent`다. 넘겨받기와 돌려주기에서 manager가 물은 (주소, port)를 실행기가 적어 `6080`은 `Any`로만, `5900`은 `Loopback`으로만 물었는지 확인한다. `6080`을 `Loopback`으로 묻는 구현은 Scope 3의 8번에서 실패하고 "`websockify` 끝내기"의 셋째 확인을 `websockify`가 떠 있는 채 지나므로 이 기록으로 잡는다.
29. 049의 `crates/shacs-cli/tests/it/spec049_browser_viewer.rs`가 Scope 15대로 고친 인자와 주소로 통과하고, 049의 나머지 브라우저 시험이 기존 동작 검증을 그대로 둔 채 통과한다. 다만 Scope 15가 정한 가짜 실행기의 `viewer_listener` 인자·위임 변경(`spec049_browser_runtime.rs`의 가짜 실행기 포함)은 허용한다. 049의 묶음 digest를 값으로 견주는 시험이 있으면 조작 판 번호 0이 들어간 값으로 고친다(시험은 코드다. 049 문서는 고치지 않는다).
30. 사용자 승인 뒤 `scripts/spec050-takeover-check.sh --approved`가 Scope 17을 확인하고(부모 수용 조건 5의 첫 항목), `scripts/spec049-viewer-check.sh --approved`를 다시 돌린다. 미실행 결정이면 이유와 미검증 범위를 명시한다. 미검증 범위는 실제 `x11vnc`와 `websockify`를 다시 띄우는 것과 listener 확인(실제 `/proc/net/tcp`의 `0100007F:170C`·`00000000:17C0` 행과 띄운 pid의 fd), 실제 `websockify`의 연결 process가 같은 process group에 있어 함께 끝나는 것, 전환 전에 연 보기 연결이 끊기는 것, 보기 연결의 클릭·키가 페이지에 닿는 것, 한글 붙여넣기가 글자 하나까지 같은 것, `websockify`의 `TokenFile`이 바뀐 경로를 다시 읽는 것과 Unix socket 대상, `SameOrigin`의 403, 실제 경로에서의 digest 다시 보내기 거절, X selection이 바뀌지 않는 것, 돌려준 뒤 다시 열지 않은 `look`이다.
31. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 전체 workspace test는 사용자 승인 뒤에만 한다. `tests/it/`의 새 파일은 기존 `tests/it/main.rs`에만 등록한다. 사용 문서와 `Cargo.lock` 무변경을 확인한다.
32. (2026-11-12 구현 중 더함) `crates/shacs-cli/src/spec050_browser_ingress_tests/takeover.rs`의 `due_dispatch_waits_for_priority_intake_in_progress`: 받는 thread가 명령을 durable 줄에 넣고 스스로 dispatch하기까지 쥐는 lock(`DURABLE_INBOUND_ENQUEUE_LOCK`)을 시험이 쥔 동안, 외부 processor의 기한 pass(`dispatch_due_external_work`)가 줄에 있는 일을 임대하지 않고 기다리다가 lock이 풀린 뒤에 임대하는지 확인한다. pass를 도는 thread가 들어선 신호를 받은 뒤부터 200ms 동안 pass가 끝나지 않는 것을 본다(2026-11-12 독립 검토 뒤 고침. 처음에는 신호 없이 200ms를 쉬었다).
33. (2026-11-12 구현 중 더함) `crates/shacs-cli/src/browser/browser_manager.rs`의 단위 시험 `stale_ending_cannot_claim_a_newer_episode`: 끝난 회차의 번호로 온 끝내기 요청이 그 뒤에 열린 회차를 끝내지 못하고, 열린 회차는 그 번호의 첫 요청만 끝내는지 확인한다. 답·돌려주기·취소는 회차 번호를 읽은 뒤 브라우저 자리를 기다리므로 그 사이에 회차가 끝나고 새 회차가 열릴 수 있다. 두 쪽이 같은 lock을 기다려 밖에서 순서를 정할 수 없으므로 끝내기를 정하는 자리(`Episodes::claim`)를 직접 본다.
34. (2026-11-12 독립 검토 뒤 더함) `crates/shacs-cli/src/spec050_browser_ingress_tests/websocket_resume.rs`의 네 시험. 실제 WebSocket server(`serve_websocket_listener_with_timeout_and_path`)와 실제 client 연결, 실제 `BrowserManager`(가짜 process 실행기, 시험용 시계), 가짜 provider로 본다.
    - `websocket_only_gateway_continues_handoff_without_a_user_message`: 외부 channel 없이 `ExternalChannelSupervisor::start`를 부른 gateway에서, WebSocket 대화가 `handoff`로 멈춘 뒤 다른 chat의 `/browser return <회차 표시>`와 한도 초과 각각에서 이어진 답(`browser_returned`, `browser_takeover_timeout`)이 같은 연결에 오는지, 그 연결이 보낸 메시지가 `handoff`를 부른 둘뿐인지 확인한다.
    - `websocket_handoff_beside_external_channels_continues_on_its_connection`: 외부 channel과 WebSocket을 함께 켠 gateway에서 WebSocket 대화의 `handoff`가 Telegram 다른 대화방의 `/browser return <회차 표시>`와 한도 초과 각각에서 같은 연결로 이어지는지, durable 줄에 `browser-resume-` 항목이 없는지, 외부 channel 쪽에 channel `websocket`으로 보내려 한 것이 없는지 확인한다.
    - `websocket_turn_leaves_an_external_room_handoff_to_the_durable_line`: Telegram 대화방에서 멈춘 호출의 회차 번호를 WebSocket 길에 건네도 아무것도 돌지 않고 끝난 것으로 답하는지, 그 호출은 durable 줄로 이어져 답이 Telegram 대화방에 가는지 확인한다.
    - `websocket_handoff_waits_for_a_connection_and_a_free_session`: 연결이 닫힌 뒤 한도가 지나면 기록이 그대로 기다리고 호출이 멈춘 채이며 provider 요청이 없는지, 세션의 turn이 도는 동안 건넨 것은 다시 건넬 것으로 답하는지(다른 chat과 모르는 회차 번호는 끝난 것으로 답한다), 그동안 새 연결이 열려도 아무것도 오지 않는지, 세션이 풀리면 새 연결이 메시지를 보내지 않고도 `browser_takeover_timeout`으로 이어진 답을 받고 기다리는 기록이 비는지 확인한다.
35. (2026-11-12 독립 검토 뒤 더함) `crates/shacs-api/src/websocket_runtime_turns.rs`의 단위 시험: 건넨 것이 그 chat의 event를 가장 나중에 쓴 연결로 가는지(`turn_goes_to_the_connection_that_last_wrote_its_chat`), 끝날 때까지 한 번만 건네지는지(`turn_is_handed_out_once_until_its_run_ends`), 닫힌 연결의 것이 다른 연결로 가는지(`turn_of_a_closed_connection_goes_to_another`), 도는 중의 panic이 그것을 끝난 것으로 남기는지(`run_that_panics_ends_its_turn_as_done`), 맡은 chat 32개와 끝난 것 64개의 한도(`remembered_chats_and_closed_turns_are_bounded`), 실제 연결이 client frame 없이 건넨 것을 돌리는지(`offered_turn_runs_on_its_connection_without_a_client_frame`), 그 chat을 맡은 연결이 생길 때까지 기다리는지(`turn_waits_until_a_connection_serves_its_chat`) 확인한다.
36. (2026-11-12 독립 검토 뒤 더함) `crates/shacs-cli/src/spec050_browser_ingress_tests/reply_route.rs`의 `telegram_topic_handoff_continues_in_its_topic`과 `slack_thread_handoff_continues_in_its_thread`: 실제 외부 ingress(Telegram update의 `message_thread_id`, Slack envelope의 `thread_ts`)로 들어온 대화가 `handoff`로 멈춘 뒤, 다른 대화방의 `/browser return <회차 표시>`와 한도 초과 각각에서 이어진 답의 outbound가 실제 전송 함수가 읽는 자리(`telegram_message_body`의 `message_thread_id`, `slack_outbound_thread_ts`)에 넘겨받은 topic·thread를 담는지 확인한다. `crates/shacs-core/tests/it/spec050_handoff.rs`의 `handoff_resumes_from_other_room_return_and_limit`은 이어 가기 inbound가 다른 답 자리 값을 달고 와도 기록의 값이 쓰이는지 본다.
37. (2026-11-12 두 번째 독립 검토 뒤 더함) `crates/shacs-cli/src/spec050_browser_ingress_tests/websocket_resume.rs`의 `web_ui_adapter_has_no_browser_tool_and_no_browser_host`: `tools.browser.enable`을 켠 설정으로 `serve_web_ui`가 adapter를 만드는 함수(`web_ui_adapter`)를 불러, 도구 목록에 `browser`가 없고 browser host가 없으며 그 adapter의 연결이 runtime의 turn을 받지 않는지(`websocket_runtime_turns()`가 없다) 확인한다. `web`에서는 `handoff`가 멈출 수 없다는 Scope 9의 글을 붙잡아 둔다.
38. (2026-11-12 두 번째 독립 검토 뒤 더함) `crates/shacs-api/src/websocket_runtime_turns.rs`의 `frame_the_client_sent_goes_before_a_turn_handed_meanwhile`: 실제 WebSocket server와 실제 client 연결로 본다. client가 frame 둘을 한 번의 쓰기로 보내고, 첫 frame의 일이 도는 동안 adapter가 그 연결에 turn을 건넨다. 그래서 첫 frame의 일이 끝날 때 둘째 frame과 건넨 turn이 함께 준비돼 있다. 둘째 frame의 event가 turn의 event보다 먼저 오는지, turn은 다시 건넨 뒤 한 번 도는지를 16번 되풀이해 확인한다.
39. (2026-11-12 두 번째 독립 검토 뒤 더함) `websocket_resume.rs`의 `websocket_resume_that_loses_its_session_after_the_check_is_handed_out_again`: 실제 WebSocket server와 연결, 실제 `BrowserManager`로 본다. adapter와 browser host 사이에 시험용 host를 끼워, 이어 가기가 세션이 비었음을 본 뒤 자기 turn의 자리를 잡기 전(inbound에 도착 표시를 찍는 자리)에 다른 turn이 그 세션의 자리를 잡게 한다. 그 시도가 provider 요청 없이 끝나고 기록이 그대로 기다리며 호출이 멈춘 채인지, 다른 turn이 도는 동안 이어 가기가 다시 건네지는지, 다른 turn이 끝나면 `browser_takeover_timeout`으로 이어진 답이 같은 연결에 한 번 오고 provider 요청이 하나만 늘며 기다리는 기록이 비는지, 그 연결이 보낸 메시지가 `handoff`를 부른 하나뿐인지 확인한다.

## Closure Evidence

1. 넘겨받기 순서 기록(상태 `user` → 경로 비움 → `websockify`와 연결 process 끝남 확인 → `x11vnc` 바꿈 → listener 확인 → 경로 씀 → 새 `websockify`)과 실패 지점별 결과 표. 대상을 찾고 멈춘 연결 process가 전환 뒤 어느 listener에도 닿지 못한 기록. `5900`은 `Loopback`, `6080`은 `Any`로만 물은 기록과 `/proc/net/tcp` 고정 글로 본 답 표.
2. 입력 process가 끝난 것을 확인하지 못했을 때 generation이 닫힌 기록과, `assistant` 상태에 입력 process가 살아 있던 시점이 없다는 기록.
3. 두 상태의 `x11vnc`·`websockify` 인자와 상태별 `view-routes` 내용.
4. 붙여넣기: 한글 문장의 보낸 글과 `Input.insertText`가 받은 글, 인증 실패·다시 보내기·크기·시간·연결 수 결과 표.
5. 회차: 한 번만 끝난 증거. 지난 질문에 단 답글과 줄에서 늦은 답·돌려주기가 새 회차를 끝내지 않은 loop·ingress 기록, channel이 늦게 전한 답글 없는 메시지가 닿은 때의 회차에 묶인 기록(결정 (마)의 범위). queue에만 있거나 실패한 질문에서 `asked`가 거짓이던 기록. 이어 가기를 기다리는 기록이 다른 회차 9개가 끝난 뒤에도 남아 있던 기록.
6. 이어 가기: bus 경로 기록, 꾸민 inbound의 거절 표, 넘겨받은 대화방으로 간 답. 외부 channel이 없는 gateway와 함께 켠 gateway에서 WebSocket 대화가 사용자 메시지 없이 같은 연결로 이어진 기록, 연결이 없거나 세션이 바쁜 동안 기록이 기다린 기록, 세션을 본 뒤에 자리를 빼앗긴 이어 가기가 다시 건네져 한 번 끝난 기록, 이미 와 있던 client frame이 건넨 이어 가기보다 먼저 처리된 기록, `web`의 adapter에 `browser` 도구와 browser host가 없는 기록, Telegram topic과 Slack thread로 간 답의 outbound. 넘겨받는 중에 실패한 `handoff`가 도구 결과 하나로 끝나고 이어 가기 inbound와 이어 가기를 기다리는 기록이 없던 기록.
7. 판정 표(mode × 규칙 × 대화형)와 `browser`의 다른 동작이 바뀌지 않았다는 비교.
8. 시험용 시계로 본 한도, 넘겨받은 동안 멈춘 두 시간, 이어진 turn 동안 답 기다리는 시간이 서 있던 기록.
9. 돌려준 뒤 이전 번호의 거절, 바뀌지 않은 문서에서 넘겨받기 전에 준비한 `look`·`capture`·`open`이 `browser_stale_revision`을 받은 기록, 다시 열지 않고 같은 페이지를 읽은 `look` 결과.
10. `handoff` 질문, 두 답의 도구 결과, classifier 요청 transcript에 답이 없다는 증거, 돌려받은 뒤 `click`의 049 판정.
11. Docker 확인 기록 또는 미실행 결정과 미검증 범위, 049 viewer 확인을 다시 돌린 기록, Cargo 검증 기록, 사용 문서와 `Cargo.lock` 확인.

PRD 010은 로그인 유지 없이 닫힌다. 그때 넘겨받아 한 로그인은 브라우저가 닫히면 없어진다(049의 임시 프로필).

## 남은 제한

- 넘겨받을 때와 돌려줄 때 `x11vnc`와 `websockify`가 다시 떠서 이어져 있던 보기가 끊기고, 그 사이 화면 보기 페이지가 잠깐 열리지 않는다. 사용자가 다시 연결한다.
- 단계 `open`에서 입력용 `x11vnc`가 shacs-bot이 끝내지 않았는데 죽으면 `maintenance`가 알아채 회차를 `view_failed`로 끝낼 때까지 `websockify`가 떠 있다. 그 사이 같은 container의 다른 process가 `5900`을 차지하면 보기 연결이 그 process에 닿을 수 있다. 같은 container 안의 process를 막지 못하는 049의 한계와 같다.
- 붙여넣기 칸은 비밀번호를 한 번 더 적어야 하고, 글은 페이지에서 지금 focus가 있는 곳에만 들어간다. 보기 페이지에서 직접 치는 한글 조합은 깨질 수 있다.
- 붙여넣은 글은 shacs-bot의 memory를 한 번 지난다. 저장하지 않지만 지나가는 동안에는 process 안에 있다.
- 화면 보기 비밀번호를 아는 사람은 넘겨받은 동안 브라우저를 조작하고 글을 넣을 수 있다. VNC는 비밀번호의 앞 8자만 확인하고 연결은 암호화되지 않는다(049와 같다).
- 붙여넣기 인증은 비밀번호를 그대로 보내지 않게 할 뿐이다. 주고받은 nonce와 digest를 엿들은 사람은 비밀번호를 offline으로 짐작해 볼 수 있고, 붙여넣은 글을 읽거나 연결 사이에 끼어들어 바꿀 수 있다. 그래서 밖에서는 암호화된 연결(VPN, ssh 터널)로만 열어야 한다.
- Origin 확인은 사용자의 브라우저가 연 다른 웹페이지만 막는다. 화면 보기 port에 닿는 다른 프로그램은 `Origin`을 꾸밀 수 있고, 틀린 인증을 10번 보내 그 회차의 붙여넣기를 끌 수 있다. 이 방해는 막지 않는다. 그때도 VNC 입력은 되고, 다시 넘겨받으면 붙여넣기가 돌아온다.
- `Host`를 바꾸는 proxy를 거쳐 화면 보기를 열면 `Origin`과 `Host`가 달라 연결이 거절된다. 주소를 바꾸지 않는 VPN과 `ssh -L`은 된다.
- 같은 container 안의 다른 process는 `127.0.0.1:5900`과 임시 폴더에 닿을 수 있다(049와 같은 한계). 브라우저를 뚫은 페이지도 그렇다.
- 넘겨받은 동안에도 탭은 하나다. 페이지가 새 창을 열면 049가 그 창을 지금 탭으로 삼고 앞 탭을 닫으므로, 새 창이 앞 창에 결과를 돌려주는 로그인(`window.opener`에 기대는 일부 OAuth)은 끝나지 않는다. 파일 올리기와 download도 막혀 있다.
- 넘겨받기 안내가 전달되기 전에 닿은 메시지, 앞 회차 때 닿아 줄에서 기다린 메시지, 지금 안내가 아닌 메시지에 단 답글은 답으로 쓰이지 않고 처리되지도 않는다. 사용자가 다시 보내야 한다. 안내를 보내는 데 실패하면 `asked`가 거짓으로 남는다. 그때는 한도가 지나거나 `/browser return <회차 표시>`·`/stop`·`/browser close`를 보낼 때까지 그 대화방의 보통 메시지와 인자 없는 `/browser return`이 그렇게 다뤄진다. 회차 표시는 `/browser status`에 있다.
- 답글 정보가 없는 메시지는 shacs-bot에 닿은 때의 회차에 묶인다. 사용자가 보낸 때와 닿은 때 사이에 회차가 바뀌면(channel이 늦게 전한 메시지) 앞 회차를 두고 한 답이나 인자 없는 `/browser return`이 새 회차를 끝낼 수 있다. SPEC 결정 기록 (마)가 정한 보장의 범위이고 이 PRD가 따로 둔 예외가 아니다. 답글 정보는 Telegram과 Discord에서만 쓴다. Slack, Email, WhatsApp, WebSocket, API의 메시지는 모두 답글 정보가 없는 것으로 다뤄진다.
- `/browser takeover`로 넘겨받은 회차가 한도로 끝나도 대화방에 알리지 않는다. `/browser status`로 본다.
- 일회 CLI와 REPL에서는 `handoff` 답이 아닌 까닭(돌려주기, 한도, 닫힘, 화면 보기 꺼짐)으로 끝난 `handoff`가 스스로 이어지지 않고, 그 대화의 다음 사용자 메시지가 `browser_takeover_over`로 호출을 닫는다(Scope 9). `browser` 도구를 등록하는 오래 도는 실행(gateway와 `serve`)에서는 스스로 이어진다. `web`은 `browser` 도구를 등록하지 않아(049 때와 같다) `handoff`가 멈추는 일이 없다(2026-11-12 두 번째 독립 검토 뒤 고침). HTTP API는 대화형이 아니어서 `handoff`가 거절되므로 멈춘 호출이 생기지 않는다.
- WebSocket 대화의 이어 가기는 그 chat을 맡은 열린 연결이 있어야 돈다. 연결이 없으면 기록이 기다리다가 연결이 그 chat을 맡을 때 이어지거나, 그 대화의 다음 사용자 메시지에서 `browser_takeover_over`로 닫힌다. 이어진 turn이 도는 중에 연결이 끊기면 결과는 세션 기록에 남고 마지막 글의 전달만 빠진다(보통 WebSocket turn과 같다). 같은 chat을 여러 연결이 맡으면 그 chat의 event를 가장 나중에 쓴 연결 하나만 받는다.
- `serve`가 알림 함수를 넣는 한 줄은 그 줄만 보는 시험이 없다. 같은 함수를 외부 channel 없이 넣은 실제 WebSocket server의 시험은 있다(수용 조건 34). 연결에 건넨 이어 가기보다 이미 와 있는 client frame을 먼저 처리하는 순서와, 자리를 잡다가 받은 `session_busy`를 다시 건넬 것으로 바꾸는 줄은 2026-11-12 두 번째 독립 검토 뒤 시험을 더했다(수용 조건 38, 39).
- HTTP API의 streaming 응답(SSE)은 `/browser` 명령의 답을 stream으로 내보내지 않는다(PRD 000 때와 같다). 그 길로 보낸 `/browser takeover`의 안내는 전달된 것으로 세지 않아 `asked`가 거짓으로 남는다. streaming이 아닌 응답은 응답을 다 쓴 뒤에 전달된 것으로 센다. 일회 CLI(`ask`)는 답을 찍고 process가 끝나므로 전달을 알리지 않는다.
- 종료 때 줄로 돌려놓는 inbound에는 도착 표시를 달지 않는다. 회차 기록이 memory에만 있어 다시 시작한 뒤에는 묶을 회차가 없다. 그런 메시지는 회차 없이 닿은 것으로 다뤄진다.
- 회차 기록과 끝난 회차의 기록은 memory에만 있다. process가 다시 시작되면 없어진다. 줄에 남아 있던 이어 가기는 loop의 확인에서 버려지고, 멈춰 있던 `handoff`는 스스로 이어지지 않고 다음 사용자 메시지에서 `browser_takeover_over`로 닫힌다. 다시 시작한 뒤에도 스스로 이어지게 하려면 끝난 회차의 기록을 디스크에 둬야 한다. 이 PRD는 하지 않는다.
- `tools.browser.view.url`은 기준 주소만 받는다. 경로가 붙은 reverse proxy 주소는 쓸 수 없다. shacs-bot은 그 주소가 열리는지 확인하지 않는다.
- 넘겨받은 동안 다른 대화방과 051 웹 감시는 브라우저를 쓰지 못한다.
- 답이 "다 했다"인지 "나중에"인지는 어시스턴트가 읽고 판단한다. 잘못 읽으면 다시 넘기거나 넘기지 않을 수 있다.
- `SameOrigin`의 실제 거절, `TokenFile`이 바뀐 경로를 연결마다 다시 읽는 것, 실제 `websockify`의 연결 process가 같은 process group에 남아 함께 끝나는 것은 Cargo 시험이 보지 못한다. Scope 17의 Docker 확인을 돌리지 않으면 확인되지 않은 채로 남는다.