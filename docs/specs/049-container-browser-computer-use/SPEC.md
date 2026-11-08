# 049. 컨테이너 브라우저 조작 구현 명세

Status: Planned

Consumes: 004·020 tool 실행과 tool surface, 009 context assembly, 010 host safety(network guard), 014 diagnostics, 015·023 공식 container image와 Compose, 022 permission mode·capability·승인 질문, 027·034 media 폴더, 030 process gate, 031 config schema, 048 auto mode 판정 순서·classifier·도구 결과 검사·`.env.json`과 `${이름}` 해석

## 목적과 완료 경계

지금 어시스턴트는 웹을 `web_fetch`로 내려받아 글로 읽기만 한다. 자바스크립트가 그려야 내용이 나오는 페이지, 버튼을 누르거나 칸에 입력해야 결과가 나오는 페이지(검색 조건 고르기, 날짜 바꾸기, "더 보기")는 다루지 못한다.

이 스펙은 공식 container 안에서 shacs-bot이 직접 띄우는 Chromium 하나를 어시스턴트가 조작하는 `browser` tool을 더한다(사용자 결정 2026-11-07). 모델 provider의 computer use 기능은 쓰지 않는다. 화면 조작은 shacs-bot이 구현하고 모델은 일반 tool 호출만 하므로, tool 호출을 지원하는 모델이면 어느 것이든 쓸 수 있다.

모델에 상관없이 동작하도록 조작 단위는 화면 좌표가 아니라 **번호**다. `browser`는 페이지에서 누르거나 입력할 수 있는 요소마다 번호를 붙여 글 목록으로 돌려주고, 같은 번호를 그린 화면 캡처를 함께 준다. 그림을 보지 못하는 모델은 글 목록만으로, 그림을 보는 모델은 둘을 함께 보고 "7번 클릭"으로 조작한다.

지금은 도구가 돌려준 그림이 모델에 그림으로 가지 않는다. 실행부가 도구 결과를 글로 바꾸므로(`tool_execution.rs`의 `ToolResult::Json(value) => value.to_string()`, MCP의 `content_part_text`) 모델은 base64 글자를 받는다. 화면 캡처를 쓰려면 이것부터 고쳐야 하므로 이 스펙이 함께 소유한다(사용자 결정 2026-11-07). 이 수정은 `web_fetch`의 그림 결과와 MCP 그림 결과에도 적용된다.

완료되면 사용자는 다음을 경험한다.

- 공식 Docker container에서 `tools.browser.enable`을 켜면, 대화로 "이 사이트에서 조건 바꿔서 찾아 줘" 같은 일을 맡길 수 있다. 어시스턴트가 페이지를 열고, 보고, 누르고, 입력하며 결과를 가져온다.
- 그림을 보지 못하는 모델로도 같은 일을 한다(번호 목록만으로).
- 브라우저는 내부·사설 주소와 같은 container의 shacs-bot 포트에 가지 못하고, `file:` 같은 로컬 주소의 내용은 어시스턴트에게 가지 않는다.
- 브라우저는 매번 빈 상태로 시작한다. 로그인, 쿠키, 방문 기록은 브라우저를 닫으면 사라진다. 로그인이 필요한 사이트는 이 스펙의 범위가 아니다.
- auto mode에서 조작은 048 classifier가 "어느 사이트의 어떤 버튼을 누르는지"를 보고 판단한다. 다른 mode에서 누르기·입력은 승인 질문을 거친다.
- `web_fetch`로 받은 그림과 MCP 도구가 돌려준 그림을 모델이 실제 그림으로 본다.
- 브라우저는 container 안의 가상 모니터에 뜬다. 사용자가 `tools.browser.view`를 켜고 비밀번호를 정하면, Docker가 도는 컴퓨터의 브라우저에서 `http://127.0.0.1:6080`을 열어 어시스턴트가 조작하는 화면을 주소창과 팝업까지 그대로 지켜본다. 이 화면으로는 보기만 하고 누르거나 입력하지 못한다.
- 공식 container 밖(Mac 직접 실행)에서는 `browser`가 쓸 수 없다고 이유와 함께 알린다.

이 스펙은 다른 스펙의 구현을 기다리지 않는다. 사용자가 화면을 넘겨받아 직접 조작하거나 로그인하는 일과 로그인 유지는 [050](../050-browser-takeover-and-saved-login/SPEC.md), 042 웹 감시가 브라우저로 페이지를 보는 일은 [051](../051-browser-rendered-web-watch/SPEC.md)이다.

## 소유 범위

1. **도구 결과의 그림 전달:**
   - tool이 돌려준 그림 블록(`image_url`의 data URL)은 글로 바뀌지 않고 모델에 그림으로 간다. runtime은 그림 바이트를 media 폴더에 저장하고, 도구 결과에는 글과 그림 파일 참조만 남긴다. 도구 결과의 내용은 지금처럼 글이다.
   - 그림은 provider에 보내는 요청을 만들 때만, 그 assistant 호출에 대한 도구 결과들이 모두 끝난 바로 뒤에 그 결과들의 그림을 담은 메시지 하나로 붙인다. provider마다 다르게 하지 않는다. 네 요청 형식(Anthropic, Chat Completions, Responses, Codex backend)은 모두 이미 사용자 메시지의 `image_url` 블록을 그림으로 바꾼다. 도구 결과 안에 그림을 넣는 방식은 provider마다 받는지가 다르고(Responses·Codex의 `function_call_output`은 확인되지 않았다) 보태는 것이 없어 쓰지 않는다.
   - 붙인 그림 메시지는 그 요청에만 있다. run의 messages, 세션 기록, 048 classifier 입력, 도구 결과 검사 요청, 화면·channel에는 나오지 않는다. 그래서 classifier가 이것을 사용자 메시지로 읽지 않는다. 메시지 안에는 그림이 도구 결과이고 사용자의 말이 아니라는 글을 같이 넣는다.
   - provider·model이 그림 입력을 받지 않는다고 기존 판정(`native_image_input_supported`)이 정하면 그림을 붙이지 않는다(도구 결과의 참조 글만 간다). provider가 그림 때문에 요청을 거절하면 지금의 그림 빼기 재시도(`retry/fallback.rs`)가 붙인 그림도 자리 표시 글로 바꾼다.
   - 요청마다 가장 최근의 그림 N개만 붙이고, 그 전 그림은 도구 결과의 참조 글로만 남는다. N과 크기 한도는 PRD 000이 정한다. 도구 결과가 글로 남으므로 오래된 결과를 줄이는 지금의 microcompact(`runner.rs`)와 큰 결과 자르기는 그대로 동작한다.
   - 대상 tool은 `web_fetch`, MCP tool, `browser`다. 다른 tool의 결과 형식은 바꾸지 않는다.
2. **container의 브라우저:**
   - 공식 image에 Debian `chromium`, 가상 모니터(`xvfb`), 한글 글꼴을 더한다. 별도 container를 두지 않고 gateway·api container 안에서 돈다(사용자 결정 2026-11-07).
   - 브라우저는 가상 모니터 위에 일반 창으로 띄운다(headless mode가 아니다). 화면 보기(소유 범위 6)를 켜든 끄든 같은 방식으로 띄워, 지켜볼 때와 아닐 때 페이지가 다르게 동작하지 않게 한다. 어시스턴트에게 주는 캡처는 가상 모니터가 아니라 DevTools에서 찍는다.
   - shacs-bot은 Chromium DevTools Protocol로 브라우저를 조종한다. `Cargo.lock`에 이미 있는 crate(`tungstenite`, `ureq`, `base64`)만 쓴다.
   - 가상 모니터와 브라우저 시작은 030 process gate를 지난다. MCP stdio server처럼 브라우저를 띄울 때 한 번 gate를 판정하고, `browser`의 각 동작은 gate를 지나지 않는다. 사용자가 user-local config에서 `tools.browser.enable`을 켠 것을 이 시작의 허락으로 본다(사용자 결정 2026-11-09). 그래서 시작 판정은 permission mode, 대화형 여부, classifier에 기대지 않고, 030 격리 증명(공식 container)과 static deny 규칙만 본다. 이 허락은 브라우저 process를 띄우는 것에만 쓰이고 `browser`의 동작 판정(소유 범위 5)을 대신하지 않는다. workspace config나 model 인자는 이 허락이 되지 않는다. 049는 030 `ProcessAdapterKind`(지금 `ExecTool`·`PluginHook`·`PluginTool`·`PluginCommand`·`McpStdio`)에 브라우저 항목 하나를 더한다. `browser` tool 자신은 process adapter를 내세우지 않는다. 내세우면 지금 capability 추론(`permission_action.rs`)이 그 tool의 모든 동작을 `proc_exec`로 본다. 공식 container 밖에서는 시작하지 않고, `browser`는 그 이유를 결과로 돌려준다.
   - 브라우저 상태는 tool 객체가 아니라 process 전체에 하나로 둔다. tool registry는 실행마다 새로 만들어지므로 tool 안에 두면 turn이 끝날 때 브라우저도 사라진다.
   - 가상 모니터가 하나이므로 한 process에서 브라우저는 한 번에 하나, 탭도 하나다. 다른 세션이 쓰는 중이면 `browser`는 기다리지 않고 그 사실을 돌려준다. 페이지가 새 탭을 열면 그 탭으로 옮긴다. 쓰지 않으면 닫히는 시간과 페이지 하나가 쓸 수 있는 시간에는 한도가 있다(PRD 010이 정한다).
   - 가상 모니터와 화면 보기 process는 브라우저와 함께 켜지고 꺼진다. 가상 모니터나 브라우저가 죽으면 셋 다 닫고 다음 호출이 새로 띄운다. 화면 보기 process만 죽으면 브라우저는 계속 돈다(소유 범위 6). shacs-bot이 끝나면 모두 끝난다.
   - Compose의 gateway·api 메모리 한도를 브라우저가 들어갈 만큼 올린다. Docker 기본 공유 메모리(`/dev/shm` 64 MB)로는 그림이 많은 페이지에서 Chromium 탭이 죽으므로 `shm_size`를 정하거나 `--disable-dev-shm-usage`로 띄운다. 정확한 값과 방식은 PRD 010이 정한다.
   - Compose의 `user:` 덮어쓰기로 실행 UID가 1000이 아닐 수 있으므로, 브라우저가 쓰는 임시 폴더는 home이 아니라 `/tmp` 아래에 둔다.
3. **브라우저의 경계:**
   - 브라우저의 모든 연결은 shacs-bot이 process 안 loopback에 여는 SOCKS5 proxy를 지난다. SOCKS5는 연결만 이어 주므로 `http:`·`https:`를 모두 HTTP 해석 없이 다루고, `std::net`만으로 만든다. Chromium은 host 이름을 넘기고(`--proxy-server=socks5://…`), proxy가 그 이름을 풀어 010 `NetworkGuard`로 확인한 주소에만, 확인한 그 주소로 연결한다. `--proxy-bypass-list=<-loopback>`으로 loopback도 proxy를 우회하지 않게 하고, QUIC(`--disable-quic`)과 proxy를 거치지 않는 WebRTC 연결(`--force-webrtc-ip-handling-policy=disable_non_proxied_udp`)은 끈다. 그래서 내부·사설 주소, container 안 shacs-bot의 API·WebSocket 포트에 가지 못한다. 정확한 실행 인자는 PRD 010이 정한다.
   - 어시스턴트에게 가는 페이지 내용은 `http:`·`https:` 문서의 것뿐이다. DevTools만으로는 모든 내부 주소(`file:`, `chrome:`, `devtools:`, `view-source:` 등) 이동을 미리 막는다고 보장할 수 없으므로 세 겹으로 막는다. (1) `open`은 `http:`·`https:` 밖의 주소를 거절한다. (2) 공식 image가 Chromium managed policy로 그 주소들을 막는다. (3) shacs-bot은 화면을 읽거나 찍기 직전과 직후에 주 문서의 주소와 문서(loader)를 확인해, `http:`·`https:`가 아니거나 그 사이에 바뀌었으면 내용을 돌려주지 않고 그 탭을 닫는다. (3)이 마지막 보장이고, (2)는 거기에 더한 방어다(Chromium 문서가 `chrome:` 차단에 예외가 있다고 밝힌다).
   - 프로필은 브라우저마다 새로 만든 임시 폴더이고 닫을 때 지운다. 이 폴더는 data 폴더와 workspace 밖에 둔다.
   - 파일 내려받기와 올리기는 거절한다. 내려받기가 필요하면 어시스턴트가 `web_fetch`를 쓴다.
   - Chromium 자체의 sandbox는 Docker 기본 설정에서 켜지지 않아 끄고 띄운다(남은 위험).
   - 가상 모니터는 TCP를 듣지 않는다(`Xvfb -nolisten tcp`). VNC 포트는 container 안 loopback에서만 열고, 브라우저의 proxy는 loopback이라 이 포트에도 연결하지 않는다.
4. **`browser` tool:**
   - 동작: `open`(주소 열기), `look`(지금 화면 보기), `click`(번호), `type`(번호, 글, 끝에 Enter 여부), `press`(키), `scroll`(방향 또는 번호), `back`, `wait`(글이 나타나거나 시간이 지날 때까지, 상한 있음), `close`.
   - `look`과, `open`·`click`·`type`·`press`·`scroll`·`back`·`wait`의 결과는 같은 형식이다. 지금 주소와 제목, 번호 붙인 요소 목록(번호, 역할, 이름, 지금 값), 페이지의 보이는 글(길이 한도 안에서), 번호를 그린 화면 캡처 한 장이다.
   - 요소 목록은 DevTools 접근성 트리(`Accessibility.getFullAXTree`)에서 누르거나 입력할 수 있는 요소를 고르고, 각 요소의 화면 위치(`DOM.getBoxModel`)로 지금 화면에 보이는 것만 남겨 만든다. 번호는 다음 결과가 나올 때까지만 유효하다. 지난 번호를 쓰면 실행하지 않고 다시 보라고 돌려준다.
   - 번호는 캡처 직전에 페이지 DOM 위에 덧붙인 번호표를 Chromium이 그리게 하고, 캡처(`Page.captureScreenshot`) 뒤 지운다. shacs-bot은 그림을 열거나 고치지 않는다. `Cargo.lock`에 그림을 다루는 crate가 없기 때문이다. 번호표는 페이지 script가 보거나 지울 수 있으므로, 번호와 요소의 대응은 캡처가 아니라 요소 목록이 정한다.
   - `tools.browser.screenshots`가 없으면 그림 입력을 받는 provider·model일 때만 캡처를 붙인다(소유 범위 1의 기존 판정). `false`면 붙이지 않고, `true`면 붙인다.
   - `type`의 대상이 비밀번호 칸(`input[type=password]`, 또는 `autocomplete`가 `current-password`·`new-password`인 칸)이면 거절한다.
   - 하위 작업(`spawn`)과 workflow 자식에는 `browser`를 주지 않는다.
5. **판정:**
   - capability는 tool 이름이 아니라 동작마다 정한다. 지금 capability 추론(`permission_action.rs`의 `infer_capabilities`)은 tool 이름만 보므로 `browser`는 동작 인자를 본다.
   - `open`·`look`·`scroll`·`back`·`wait`·`close`는 `net_outbound`이고 `web_fetch`와 같은 판정을 받는다. 단 `plan` mode에서는 `web_fetch`와 달리 이 동작들을 허용한다(사용자 결정 2026-11-09). `web_fetch`의 `plan` 판정은 그대로다.
   - `click`·`type`·`press`는 새 capability `browser_act`(페이지에 영향을 주는 동작)다. 049가 022 `SafetyCapability`와 022 판정 표에 이 한 항목을 더한다. auto mode에서는 048 4단계 classifier가 보고, `default`·`accept_edits`·`plan`에서는 승인 질문을 거친다. `bypass_permissions`에서는 대화형이든 비대화형이든 실행한다(사용자 결정 2026-11-09). 그 밖의 mode의 비대화형 실행과 `dont_ask`에서는 거절한다.
   - classifier와 승인 질문이 보는 action 설명은 번호가 아니라 내용이다. 사이트(origin), 요소의 역할과 이름, 입력할 글, 그 요소가 속한 양식의 이름과 제출 주소를 적는다. 이 설명은 022 redaction과 048의 `.env.json` 값 가리기를 거친다.
   - auto mode 기본 `softDeny`에 "브라우저에서 구매·예약·결제·발송·게시·계정 설정 변경을 확정하는 동작"(`[Browser Commitment]`, 글은 [앞선 스펙에서 바꾸는 것](#앞선-스펙에서-바꾸는-것))을 더한다. 사용자가 그 동작을 직접 말하면 풀린다(048 소유 범위 9).
   - `browser` 결과의 글은 048 도구 결과 검사를 받는다. 캡처 그림은 검사 요청에 넣지 않는다.
   - 사용자 규칙(048 소유 범위 2)에서 `browser`와 `browser(domain:<host pattern>)`를 쓸 수 있다. 지금 규칙 문법은 `domain:`을 `web_fetch`에만 받으므로(`permission_rule.rs`) 049가 048 문법을 이 한 꼴로 넓힌다. host pattern의 뜻은 `web_fetch`와 같고, 동작이 가는 사이트(`open`은 열 주소, 나머지는 지금 페이지의 origin)에 맞춘다.
6. **화면 보기(보기 전용, 사용자 결정 2026-11-07):**
   - 공식 image에 `x11vnc`, `novnc`, `websockify`를 더한다. 브라우저가 뜨면 `x11vnc`가 가상 모니터를 내보내고, `websockify`가 noVNC 보기 페이지와 그 연결을 container 포트 6080에서 제공한다. Dockerfile `EXPOSE`에 6080을 더하고, Compose는 gateway의 이 포트를 `127.0.0.1:6080`에만, api는 `127.0.0.1:6081`에만 연다.
   - 보기 전용은 서버에서 강제한다. `x11vnc`를 보기 전용으로 띄워, 보기 페이지를 고친 client가 보낸 클릭·키도 브라우저에 가지 않는다. 클립보드 주고받기도 끈다.
   - 설정은 `tools.browser.view.enable`(기본 꺼짐)과 `tools.browser.view.password`다. 비밀번호는 048 `.env.json`에 두고 config에는 `${이름}`을 쓴다. 켰는데 비밀번호가 없거나 8자보다 짧으면 화면 보기를 띄우지 않고 diagnostics에 이유를 남긴다. 브라우저는 그대로 쓴다.
   - 비밀번호는 `x11vnc`에 넘길 때만 임시 파일로 쓰고, `x11vnc`가 읽은 뒤 남지 않는다. 명령줄 인자, 환경 변수, 로그, 어시스턴트에게 가는 글, channel 메시지에 나오지 않는다. 정확한 방식은 PRD 040이 정한다.
   - VNC 포트는 container 안 loopback에서만 열어 `websockify`만 닿는다. 밖에 여는 것은 6080(api는 6081) 하나다.
   - 화면 보기는 브라우저가 떠 있는 동안만 열린다. 화면 보기 process가 죽어도 브라우저는 계속 돌고, 다음에 브라우저가 뜨면 다시 켜진다.
   - 보는 사람이 있든 없든 어시스턴트의 동작과 판정은 같다. 화면을 보고 있다는 것이 승인을 대신하지 않는다.
7. **설정:** `tools.browser.enable`(기본 꺼짐), `tools.browser.screenshots`(기본 없음, 소유 범위 4), `tools.browser.view.enable`·`tools.browser.view.password`, 한도 값. `onboard`가 기본 stub을 만들고, 비밀번호 칸은 빈 글로 둔다(048 소유 범위 3).
8. **표면과 문서:** `runtime inspect`와 diagnostics에 브라우저 사용 가능 여부와 쓸 수 없는 이유(container 밖, 꺼짐, Chromium 없음, 다른 세션이 쓰는 중), 화면 보기 상태(꺼짐, 비밀번호 없음, 열림과 보기 주소)를 보인다. 비밀번호는 보이지 않는다. 각 PRD는 자기가 바꾼 동작을 `docs/USAGE.md`, README의 도구·Docker 설명에 함께 적는다.

## 기존 owner와의 경계

| 기존 owner | 049가 소비하는 것 | 049가 소유하지 않는 것 |
|---|---|---|
| 004·020 | tool 실행과 등록, deferred bridge | 다른 tool의 동작 |
| 009 | context assembly | 그림 결과를 고르는 것 말고 context를 짜는 방식 |
| 010 | `NetworkGuard`의 판정 | 판정 규칙과 SSRF 허용 목록 |
| 015·023 | 공식 image, Compose, containment 증거 | image의 다른 구성, 공식 container 판정 방식 |
| 022 | permission mode, capability, 승인 질문, redaction | 049가 더하는 capability `browser_act`와 그 판정 표 자리 말고 capability와 판정 |
| 027·034 | media 폴더 위치 | 첨부·생성 media의 저장 방식 |
| 030 | process gate | 049가 더하는 `ProcessAdapterKind` 브라우저 항목 말고 gate의 판정과 격리 증명 |
| 048 | 판정 순서, classifier, 기본 slot, 도구 결과 검사, 사용자 규칙 문법, `.env.json`과 `${이름}` 해석 | 049가 더하는 것(`softDeny` 한 항목, `browser(domain:…)` 규칙 꼴) 말고 auto mode의 판정 |

## 앞선 스펙에서 바꾸는 것

049는 아래 앞선 스펙의 계약을 바꾼다. 앞선 스펙 문서는 고치지 않고 닫힐 때의 기록으로 두며, 049가 닫힌 뒤에는 이 표의 "바뀐 뒤"가 지금 계약이다.

| 스펙 | 지금 계약(위치) | 바뀐 뒤 | PRD |
|---|---|---|---|
| 004 | `ToolResult`를 `RuntimeToolMessage`로 바꾼다(004 SPEC "tool output" 표와 `ToolResult` 변환). 코드의 `RuntimeToolMessage.content`는 글이라 `ToolResult::Json`의 그림 블록도 base64 글로 바뀐다 | 그림 블록은 media 폴더에 저장되고 `RuntimeToolMessage`는 글과 그림 참조를 담는다. `web_fetch`·MCP·`browser`의 그림은 provider 요청에서 그림으로 간다(소유 범위 1). 다른 tool의 결과는 그대로다 | 000 |
| 022 | 최소 capability set 10개(022 SPEC `Capability` 절의 `fs_read`…`self_modification`) | 11번째 `browser_act`(브라우저에서 페이지에 영향을 주는 누르기·입력·키 누르기)를 더한다. 판정은 소유 범위 5다 | 030 |
| 030 | process gate가 받는 실행 종류. 문서에는 목록이 없고 코드의 `ProcessAdapterKind`가 `ExecTool`·`PluginHook`·`PluginTool`·`PluginCommand`·`McpStdio`다 | 브라우저 항목을 더한다. 가상 모니터·브라우저·화면 보기 process를 띄울 때 한 번 판정한다(소유 범위 2) | 010 |
| 030·022 | process gate의 시작 판정: gate가 permission 판정(`decide_permission`)으로 `Allow`를 받아야 process를 시작한다(코드 `process_gate.rs`, MCP 시작은 설정한 mode의 비대화형 판정을 받는다) | `ProcessAdapterKind`의 브라우저 항목은 user-local `tools.browser.enable`을 허락으로 보고, 격리 증명과 static deny만 확인한다. mode·대화형 여부·classifier는 보지 않는다. 다른 adapter의 판정은 그대로다(소유 범위 2) | 010 |
| 022 | `plan` mode는 `fs_read`만 허용하고 나머지 capability를 거절한다(코드 `permission_policy.rs`의 `decide_plan`) | `browser`의 `open`·`look`·`scroll`·`back`·`wait`·`close`(`net_outbound`)를 허용하고, `browser_act`는 대화형이면 승인 질문, 비대화형이면 거절한다. `web_fetch`와 다른 capability의 `plan` 판정은 그대로다(소유 범위 5) | 030 |
| 022 | 비대화형 실행에서 묻는 판정은 거절이 된다 | `browser_act`는 `bypass_permissions`이면 비대화형이어도 실행한다(지금 `bypass_permissions`의 다른 capability와 같다). 다른 mode의 비대화형은 거절이다(소유 범위 5) | 030 |
| 048 | PRD 000 범위 4의 규칙 표: `domain:` 지정은 `web_fetch(domain:<host pattern>)`뿐이다 | `browser(domain:<host pattern>)`을 더한다. host pattern의 뜻은 `web_fetch`와 같다(소유 범위 5) | 030 |
| 048 | PRD 000 범위 13의 기본 `SOFT DENY` 목록 7개 | 끝에 한 항목을 더한다: `- [Browser Commitment] In the browser, submitting or confirming a purchase, booking, payment, message, post, or account-setting change the user did not ask for.` | 030 |

## 반드시 지킬 것

- 모델 provider의 computer use 기능이나 provider 전용 tool 형식을 쓰지 않는다. `browser`는 일반 function tool이다.
- 그림을 받지 못하는 모델도 `browser`의 모든 동작을 쓸 수 있다.
- 붙인 그림 메시지는 classifier와 도구 결과 검사의 사용자 메시지가 되지 않는다.
- 브라우저의 연결은 `NetworkGuard`를 지나지 않고 나가지 않는다. 어시스턴트에게는 `http:`·`https:` 문서의 내용만 간다.
- 브라우저는 data 폴더와 workspace를 프로필·내려받기 위치로 쓰지 않는다. 브라우저를 닫으면 프로필이 남지 않는다.
- 공식 container 밖에서는 브라우저를 시작하지 않는다.
- 화면 보기는 보기 전용이고, 비밀번호 없이는 열리지 않으며, host의 `127.0.0.1` 밖에 열리지 않는다. 비밀번호는 어시스턴트와 AI provider에게 가지 않는다.
- classifier와 승인 질문은 번호가 아니라 요소의 내용으로 판단한다.
- `Cargo.lock`에 새 crate를 더하지 않는다. 필요해지면 멈추고 사용자에게 묻는다.
- Cargo 테스트는 가짜 DevTools 서버와 가짜 provider로 돈다. Chromium이 없는 환경에서도 통과한다.

## 수용 조건

1. 그림 전달:
   - 가짜 provider 요청을 가로채 보면 `web_fetch`·MCP·`browser`의 그림이 provider 형식마다 그림 블록으로 들어 있고, 글에 base64가 없다.
   - 그림을 거절하는 가짜 provider에서는 그림이 자리 표시 글로 바뀐 재시도로 turn이 끝난다.
   - run의 messages, 세션 파일, classifier와 도구 결과 검사 요청에 붙인 그림 메시지가 없다.
   - 그림 입력을 받지 않는다고 판정된 model에는 그림이 처음부터 자리 표시 글로 간다.
   - 세션 파일에 그림 바이트가 없고, 최근 N개를 넘는 그림은 붙지 않는다.
2. 가짜 DevTools 서버로 `open` → `look` → `type` → `click` → `look` 흐름이 실제 `AgentLoop` turn(가짜 provider)에서 끝난다. 캡처 전에 번호표를 덧붙이고 캡처 뒤 지우는 DevTools 명령이 오간다. 지난 번호의 `click`은 실행되지 않는다. `screenshots: false`나 그림 입력을 받지 않는 model이면 그림 없이 같은 흐름이 끝난다. 브라우저가 turn을 넘어 같은 process에서 이어진다.
3. 경계:
   - proxy가 사설·loopback 주소, container 안 shacs-bot 포트, 이름이 사설 주소로 풀리는 host를 거절한다.
   - `open`의 `file:`·`chrome:` 주소, 내려받기, 올리기, 비밀번호 칸 `type`이 거절된다. 주 문서가 `http:`·`https:`가 아닌 상태에서나 읽는 사이 문서가 바뀐 때의 `look`은 내용을 돌려주지 않고 탭을 닫는다. 공식 image에 내부 주소를 막는 managed policy 파일이 있다.
   - 브라우저를 닫은 뒤 프로필 폴더가 없다.
   - 공식 container 밖 설정에서는 브라우저 process가 시작되지 않고 이유가 돌아온다. gate는 브라우저를 띄울 때 한 번 판정되고 `browser` 동작마다 판정되지 않는다.
   - 실행 인자에 SOCKS5 proxy, loopback 우회 금지, QUIC 끔, WebRTC 제한, 공유 메모리 처리가 있고, 가상 모니터가 TCP를 듣지 않는다.
4. 판정:
   - mode × 동작 종류 × classifier 응답(허용, 막음, 판정 없음) × 대화형 여부가 소유 범위 5와 같다. `look`·`scroll`의 capability는 `net_outbound`, `click`·`type`·`press`는 `browser_act`이고 `proc_exec`가 아니다.
   - `browser(domain:<host pattern>)` 규칙이 읽히고, 그 사이트의 동작에만 맞는다.
   - classifier 요청과 승인 질문의 action 설명에 origin, 요소 역할·이름, 입력 글이 있고 요소 번호만 있지 않다.
   - 기본 `softDeny`의 구매·발송 항목에 걸린 `click`이 막히고, 사용자가 그 동작을 직접 말한 대화에서는 풀린다.
5. 화면 보기(Cargo, 가짜 process 실행기):
   - 켰는데 비밀번호가 없거나 짧으면 화면 보기 process가 시작되지 않고, 브라우저는 뜨며, diagnostics에 이유가 있다.
   - `x11vnc`가 보기 전용·loopback·클립보드 끔으로 뜨고, 명령줄 인자와 환경 변수에 비밀번호가 없으며, 시작 뒤 비밀번호 임시 파일이 없다.
   - 브라우저를 닫으면 화면 보기 process가 끝나고, 화면 보기 process가 죽어도 `browser` 호출은 이어진다.
   - inspect·diagnostics·로그·모델 요청에 비밀번호가 없다.
6. 실제 Docker 브라우저(opt-in 스크립트, 사용자 승인 뒤). 사용자의 data directory는 쓰지 않는다.
   - 공식 image에서 가상 모니터와 Chromium이 뜨고, 고정 로컬 test 페이지에서 입력·클릭·결과 읽기가 끝난다. test 페이지는 시험 스크립트가 같은 Compose 내부망에 따로 띄우는 정적 파일 container이고, 시험용 config의 `tools.ssrfWhitelist`에 그 container 주소 하나(`/32`)만 넣어 연다. `ssrf_whitelist`는 포트를 보지 않는 주소 단위이므로 test 페이지를 shacs-bot과 같은 container(loopback이나 container 주소)에 두면 shacs-bot 포트도 함께 열린다. proxy가 사설 주소를 여는 다른 길은 없다.
   - 같은 container의 shacs-bot API·WebSocket 포트와 VNC 포트에는 브라우저가 가지 못한다.
   - 이미지가 많은 test 페이지에서 탭이 죽지 않는다.
7. 실제 Docker 화면 보기(opt-in 스크립트, 사용자 승인 뒤). 사용자의 data directory는 쓰지 않는다.
   - host에서 `127.0.0.1:6080`으로 올바른 비밀번호를 주면 화면이 보이고, 틀린 비밀번호는 거절된다. 보기 연결로 보낸 클릭·키는 페이지를 바꾸지 않는다. VNC 포트는 host에서 닿지 않는다.
8. fmt, workspace clippy `-D warnings`, 변경한 package 테스트가 통과한다. 전체 workspace 테스트는 사용자 승인을 받아 실행한다.

위 조건은 049의 단독 종료 기준이다. 수용 조건 6·7은 사용자가 돌리지 않기로 결정할 수 있다. 그러면 그 PRD의 종료 확인에 결정과 이유, 그 때문에 확인하지 못한 것을 적고 scoped로 닫는다(048 수용 조건 10과 같다).

## 남은 위험과 한계

- **같은 container라서 브라우저가 뚫리면 data 폴더에 닿는다.** Chromium sandbox가 꺼져 있으므로, 악성 페이지가 브라우저의 보안 결함을 쓰면 shacs-bot과 같은 사용자로 container 안의 `auth.json`, `.env.json`, 세션 기록을 읽을 수 있다. 별도 container에 두면 막을 수 있지만 사용자 결정(2026-11-07)으로 같은 container를 쓴다. Chromium을 image 빌드 때마다 최신으로 받는 것이 주된 완화다. Compose에 Chromium용 seccomp profile(`security_opt`)을 두면 권한을 더하지 않고 Chromium sandbox를 켤 수 있어 위험을 일부 줄인다. 이 스펙은 하지 않고 나중 선택지로 남긴다.
- image가 Chromium, 가상 모니터, 화면 보기 프로그램, 글꼴만큼 커지고(수백 MB), 브라우저가 떠 있는 동안 메모리를 많이 쓴다. headless보다 메모리를 조금 더 쓴다.
- 가상 모니터가 하나라 한 번에 한 대화만 브라우저를 쓴다. 다른 대화방의 요청은 그동안 브라우저를 쓰지 못한다.
- VNC 비밀번호는 방식의 한계로 앞 8자만 확인된다. host와 container 사이의 보기 연결은 암호화하지 않는다. 그래서 host의 `127.0.0.1`에만 열며, 그 컴퓨터에서 비밀번호를 아는 사람은 화면을 볼 수 있다. 화면에는 어시스턴트가 연 페이지의 내용이 그대로 보인다.
- 화면 보기는 Docker가 도는 그 컴퓨터에서만 열린다. 밖에서 channel로 대화하는 중에는 볼 수 없다.
- 요소 목록은 접근성 정보에 기댄다. 그림으로만 그린 페이지(canvas, 지도, 일부 게임·차트)는 번호를 붙일 요소가 없어 다루지 못한다. 좌표 클릭은 두지 않는다.
- 캡처의 번호표는 페이지 DOM에 그리므로 페이지 script가 가리거나 옮길 수 있다. 그때 그림의 번호는 틀릴 수 있지만, 동작은 요소 목록의 번호로 하므로 다른 요소를 누르지는 않는다.
- 요소의 이름은 페이지가 정한다. 악성 페이지가 "취소"라는 이름의 결제 버튼을 만들면 classifier와 승인 질문도 그 이름을 본다.
- 페이지 글은 도구 결과 검사가 경고만 붙인다. 화면 캡처 안의 글은 검사하지 않는다.
- 브라우저는 `web_fetch`와 같은 `NetworkGuard`를 쓰므로, 사용자가 `tools.ssrfWhitelist`에 넣은 주소(집 안 서버 등)에는 브라우저도 간다. 이 목록은 포트를 보지 않는다.
- 많은 사이트가 자동화된 브라우저를 막거나 확인 문제(캡차)를 낸다. 이 스펙은 이를 우회하지 않는다.
- 로그인이 없으므로 회원 전용 페이지는 볼 수 없다.
- 화면 캡처는 그대로 AI provider로 가고 media 폴더에 남는다.
- auto mode에서 `click`·`type`·`press`마다 classifier를 부르므로 model 호출과 지연이 늘어난다. `default` mode에서는 누를 때마다 묻는다.
- 그림 전달 수정으로 `web_fetch`·MCP의 그림 결과가 지금보다 많은 token을 쓸 수 있다(지금은 base64 글자로 가서 그림으로 보이지 않았다).

## 실행 PRD와 독립 종료

각 PRD는 앞 번호의 닫힌 결과만 소비한다. 구현 전 상태는 모두 `Planned`다. 번호 순서대로 구현한다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 도구 결과의 그림 전달](prds/000-tool-result-images.md) | 소유 범위 1, 8 중 자기 변경의 문서 | 없음 | 수용 조건 1, 8 |
| [010 container 브라우저와 경계](prds/010-container-browser-and-boundary.md) | 소유 범위 2(`ProcessAdapterKind` 항목 포함), 3, 7 중 `enable`과 한도, 8 중 브라우저 상태 | 000 | 수용 조건 3, 8 |
| [020 `browser` tool](prds/020-browser-tool.md) | 소유 범위 4, 7 중 `screenshots` | 000, 010 | 수용 조건 2, 6, 8 |
| [030 판정 연결](prds/030-decision-wiring.md) | 소유 범위 5(`browser_act`, 규칙 문법 포함) | 000–020 | 수용 조건 4, 8 |
| [040 화면 보기](prds/040-screen-viewer.md) | 소유 범위 6, 7 중 `view`, 8 중 화면 보기 상태 | 000–030 | 수용 조건 5, 7, 8 |

2026-11-08: PRD 000은 `Complete (Scoped)`다(독립 검토 2차 PASS). 2026-11-09: PRD 010–040 문서를 썼고 독립 검토 2차 PASS를 받았다. 구현 전이라 `Planned`다.

수용 조건 8은 각 PRD가 자기 변경에 대해 확인한다. PRD 010의 경계는 가짜 시험(수용 조건 3)으로 닫고, 실제 Docker의 경계 확인은 브라우저 조작이 생기는 PRD 020의 수용 조건 6에서 한다.
