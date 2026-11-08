# PRD 020. browser tool

Status: Planned

Depends on: [PRD 000](000-tool-result-images.md)와 [PRD 010](010-container-browser-and-boundary.md)의 닫힌 결과. PRD 030·040과 050·051의 구현에 의존하지 않는다.

## Goal

일반 function tool `browser`로 주소를 열고, 보이는 요소의 번호로 조작하며, 같은 결과에 글 목록과 번호를 그린 캡처를 돌려준다. 그림을 받지 않는 모델도 같은 번호로 조작한다. 이 PRD만으로 가짜 provider의 실제 turn과 opt-in Docker 브라우저 시험을 끝낸다.

## 지금 상태

- `production_tool_registry`는 `WebFetchTool`·`WebSearchTool` 등을 등록하지만 browser host를 받거나 browser를 등록하지 않는다(`crates/shacs-cli/src/spec035_cli_media/tool_registry.rs`의 동명 함수).
- `build_subagent_tool_registry`는 file·exec·web tool을 고정 목록으로 만들고 allowed tools로 줄인다(`crates/shacs-core/src/runtime/subagent.rs`). browser를 여기에 추가하거나 부모 registry를 복사하면 안 된다.
- `infer_capabilities`는 tool 이름으로 capability를 정하고 process adapter가 있는 tool은 모두 `ProcExec`로 정한다(`crates/shacs-core/src/runtime/permission_action.rs`). 동작별 최종 판정은 아직 없다.
- `tool_images::persist`는 text/image_url content-part 배열을 풀어 media 파일과 `[image: tool-results/…]` 글로 바꾼다. `tool_images::attach`는 최근 그림 셋을 요청에만 붙인다(`crates/shacs-core/src/runtime/tool_images.rs`). browser 전용 저장기를 만들 필요가 없다.
- `AgentLoopChatCompletionAdapter::loop_config_for`는 `ToolImageSettings.attach`를 `native_image_input_supported`로 채운다. 그 판정은 adapter 생성 때 `provider_model_supports_native_image_input`으로 한다(`crates/shacs-cli/src/lib.rs`). 캡처 기본값도 이 판정을 소비해야 한다.
- `ToolCallExecutionContext`는 process gate·중단·provider invocation·숨김 경로만 담는다(`crates/shacs-core/src/tools/base.rs`의 동명 struct). runtime의 `ToolExecutionContext`에는 session key·permission mode snapshot·대화형 여부가 있지만 browser의 호출별 그림 설정은 없다. `permissioned_action_input_from_context`가 실제 session id를 해석한다(`crates/shacs-core/src/runtime/tool_execution.rs`). registry의 생성 시점 자료만으로는 실제 호출의 session·mode를 알 수 없다.
- `runtime_capability_ceiling_from_registry`는 이름별 capability를 모으지만 browser 분기는 없다(`crates/shacs-cli/src/lib.rs`). tool 등록만 더하면 실행의 상속 ceiling에 browser용 `net_outbound`가 빠질 수 있다.

## Scope

1. **tool과 schema:** core의 leaf module에 `BrowserTool`을 만들고 010의 `BrowserHost`를 주입한다. 이름은 `browser`, `process_adapter_kind()`는 `None`, 임의 JavaScript·CDP 명령·좌표·파일 경로는 받지 않는다. object schema는 알 수 없는 key를 거절하며 다음 인자만 받는다. 동작별 불필요한 인자도 거절한다.

   | action | 인자와 의미 |
   |---|---|
   | `open` | `url: string` 필수. 절대 http(s) 주소를 연다 |
   | `look` | 추가 인자 없음. 현재 화면을 다시 읽는다 |
   | `click` | `element: integer`, `revision: string` 필수 |
   | `type` | `element`, `revision`, `text: string`(최대 4,000자), `submit: boolean`(기본 false). 기존 값을 교체하고 submit이면 끝에 Enter |
   | `press` | `key: string` 필수. `Enter`, `Tab`, `Shift+Tab`, `Escape`, `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight`, `Home`, `End`, `PageUp`, `PageDown`, `Backspace`, `Delete`만 허용 |
   | `scroll` | `direction: "up"|"down"|"left"|"right"`(기본 down), 선택적 `element`와 `revision`은 둘 다 있어야 함. 대상이 없으면 viewport, 있으면 해당 요소를 한 viewport의 80%만큼 scroll |
   | `back` | 추가 인자 없음. 이전 기록이 없으면 그 사실과 현재 결과 |
   | `wait` | `text: string`(최대 200자), `timeoutMs: integer`(기본 1,000, 범위 1–10,000). text가 있으면 그 글이 보일 때까지, 없으면 지정 시간만 대기 |
   | `close` | 추가 인자 없음. 소유 session의 browser를 닫고 확인 글만 반환 |

   - **호출 context:** runtime의 `ToolExecutionContext`와 tool 경계의 `ToolCallExecutionContext`에 `browser_context: Option<BrowserCallContext>`를 더한다. 010이 정의한 필드는 `effective_session_id`, `permission_mode_snapshot`, `permission_interactive`, `browser_screenshots: Option<bool>`, `native_image_input_supported: bool`이다. 유효 session id는 `permissioned_action_input_from_context`와 같은 해석 결과이고, mode·대화형 여부는 권한 판정에 쓴 것과 같은 run snapshot에서 온다. 그림 설정은 그 run의 `tools.browser.screenshots`와 `native_image_input_supported`를 쓴다. production adapter가 run snapshot을 만들 때 한 번 채우며 기존 권한 context와 다른 값을 따로 만들지 않는다.
   - 직접 호출·deferred bridge의 실제 tool 실행·승인 후 실행은 이 호출 context를 같은 실행 경로로 전달한다. 승인 대기 뒤에는 승인한 action에 묶인 run snapshot을 유지하며 registry의 오래된 mode나 다른 session의 설정으로 바꾸지 않는다. `BrowserTool::execute_with_context`가 이 자료를 `BrowserHost`에 호출마다 넘긴다. tool의 context 없는 `execute`와 context 누락/불일치는 `browser_context_unavailable`로 거절하고 host를 시작하지 않는다. session·mode·대화형 여부·그림 설정을 model 인자, channel 이름의 임의 추정, registry 생성 때의 값에서 가져오지 않는다. registry는 process 전체 host handle만 공유한다.

2. **결과:** `close`와 오류는 text part만, 나머지 성공은 `ToolResult::Json`의 content-part 배열이다. 첫 text part는 현재 URL·제목, opaque `revision`, 요소 목록(`번호`, `role`, `name`, `value`), 보이는 페이지 글, 잘림 여부를 담는다. URL·제목은 각 2,000자, 요소는 최대 100개·각 이름/값 200자, 페이지 글은 12,000자로 제한한다. 잘라낸 개수를 적는다. password·file input의 값은 비어 있지 않아도 내보내지 않고 `[입력 불가]`로 쓴다. 캡처가 있으면 뒤에 `{"type":"image_url","image_url":{"url":"data:image/png;base64,<CDP 결과>"}}` 한 part를 둔다. 그 배열을 000의 기존 저장·참조·붙이기에 넘긴다. tool이 media에 직접 쓰거나 base64를 별도 text에 넣지 않는다.
3. **화면 읽기:** 010의 검증된 읽기 안에서 `Accessibility.getFullAXTree`로 interactive role(button, link, textbox, searchbox, checkbox, radio, combobox, listbox, option, switch, slider, spinbutton, menuitem, tab)을 고른다. ignored·disabled·숨김 요소는 제외한다. `backendDOMNodeId`를 `DOM.getBoxModel`에 넘겨 면적이 있고 viewport와 겹치며 computed style의 display·visibility가 보이는 요소만 남긴다. 교차 문서의 non-http(s) frame 요소는 제외한다. DOM order로 1부터 번호를 매기고 내부적으로 backend node id와 묶는다. 이름과 역할은 접근성 자료, 값·password/file 판정은 DOM 속성으로 얻는다. 보이는 글은 주 문서의 `document.body.innerText`다. 요소의 좌표는 내부 클릭 구현에만 쓰고 tool 인자로 공개하지 않는다.
4. **번호 수명:** 각 결과에 browser generation·target·loader·단조 증가 snapshot 회차에 묶인 opaque revision을 낸다. 다음 결과가 발급되거나 navigation·target 전환·문서 변경이 관찰되면 이전 revision은 무효다. 실행 직전에 현재 node가 연결돼 있고 같은 role/name·종류인지 재확인한다. 달라지거나 없으면 누르지 않고 `look`하라는 결과다. 실행 시작 때 기존 번호를 무효로 해 같은 snapshot의 중복 호출이 다른 요소에 적용되지 않게 한다. 승인 질문을 거쳐 늦게 실행될 호출도 이 재확인을 통과해야 한다.
5. **캡처:** 010의 같은 문서 guard 안에서 요소 목록으로 정한 화면 위치에 번호표용 독립 overlay를 DOM에 붙이고, `Page.captureScreenshot`(PNG, 현재 viewport만) 뒤 반드시 제거한다. overlay는 `pointer-events:none`이고 페이지 상태를 고치지 않으며 script 실패·캡처 실패에도 정리한다. 번호표는 페이지가 변조할 수 있으므로 번호의 기준은 내부 요소 목록이다. PNG를 Rust에서 열거나 수정하지 않는다. 5 MiB를 넘으면 000의 `[image not kept: image larger than 5 MiB]` 처리로 가며 글 결과는 유지한다.
6. **그림 설정:** `tools.browser.screenshots: Option<bool>`를 더한다. 호출의 `BrowserCallContext.browser_screenshots`가 없으면 같은 context의 `native_image_input_supported`, false면 캡처하지 않음, true면 캡처한다. true인데 모델이 그림을 못 받아도 같은 run의 000 attach는 false이므로 저장과 참조 글만 남는다. 설정이 없고 그림을 못 받거나 false면 overlay·capture 명령도 보내지 않는다. onboard는 이 key를 강제로 false로 쓰지 않는다. 사용자 설명에는 없음/true/false 차이를 적는다.
7. **동작과 경계:** `open`은 인자를 파싱해 http(s)만 허용하고 userinfo가 든 URL도 거절한다. `file:`, `chrome:`, `devtools:`, `view-source:`, `data:`, `javascript:`, 상대 주소는 CDP 이동 전 거절한다. HTTP redirect와 script 이동 후 읽기는 010 guard를 통과해야 한다.
    - `click`은 번호 대상의 가시 영역에서 후보 점을 고르고, 입력 직전에 올바른 frame/session과 좌표계로 `DOM.getNodeForLocation` 등의 hit-test를 한다. 반환된 frame과 node가 번호 대상 또는 그 자손에 해당할 때만 Input event를 보낸다. 자식 frame에서는 viewport 좌표를 변환하고 상위 문서의 iframe이 가려지지 않았는지도 확인한다. 다른 요소에 덮였거나 다른 frame이 반환되거나 대응을 확인할 수 없으면 `browser_target_covered`와 다시 look하라는 안내를 반환하고 입력하지 않는다. 확인하지 않은 box 중앙으로 입력하지 않는다. `type`은 검증한 editable 요소에 focus한 뒤 기존 값을 교체한다. `press`는 Chromium 주소창이나 임의 shortcut이 아닌 page target의 `Input.dispatchKeyEvent`다.
    - `input[type=password]` 또는 autocomplete token에 `current-password`·`new-password`가 있는 요소의 `type`은 submit 여부와 무관하게 거절한다. focused password 요소에서의 입력 효과가 있는 `press`도 거절한다. password 값은 AX·DOM text 결과에서 지운다.
    - `input[type=file]`의 click·type은 실행 전에 거절한다. chooser·download event는 010의 host 경계가 막는다. tool 설명은 내려받기에 `web_fetch`를 쓰라고 안내한다.
    - 페이지가 연 새 page target은 auto-attach로 pause한 상태에서 download/chooser 설정을 적용하고 현재 target으로 삼으며 이전 탭을 닫는다. 복수 target이면 생성 순서대로 마지막 하나만 남긴다. popup도 새 번호가 필요한 결과다. 새 target이 내부 문서면 읽지 않고 닫는다.
    - `wait`와 이동은 010 `pageTimeoutSeconds` 남은 시간 이내다. text 대기 초과는 성공을 합성하지 않고 timeout 이유와 새로 읽을 수 있는 현재 결과를 돌려준다. 문서 guard 실패는 그 결과도 없다.
8. **030 이전의 판정:** 이 PRD는 `browser`의 모든 동작을 임시 `net_outbound`로 알려 기존 `web_fetch` 판정에 연결한다. `click`·`type`·`press`는 호출의 `BrowserCallContext.permission_mode_snapshot.mode`가 명시적 `bypass_permissions`일 때 대화형·비대화형 모두 실행한다. 그 밖에는 `browser_action_policy_unavailable: 페이지 조작 판정이 아직 연결되지 않았습니다`로 거절하고 CDP 동작을 보내지 않는다. 기존 사용자 deny/ask와 상속 한도를 우회하지 않는다. 임시로 auto classifier의 net 판정만으로 페이지 변경을 허용하지 않기 위한 제한이다. 시작 허락은 010의 user-local enable 계약을 따르며 이 bypass 제한을 process 시작에 적용하지 않는다. 030이 동작별 capability·plan 예외를 완성하며 임시 제한을 제거한다. 이 PRD의 조작 turn·Docker 시험은 명시적 bypass 시험 설정에서 수행하므로 뒤 PRD 없이 닫힌다.
9. **production 등록:** CLI 공용 `production_tool_registry`에서 process 전체 host handle로 browser를 등록한다. 같은 공용 adapter를 쓰는 일반 대화·외부 channel·local API·cron/heartbeat에도 같은 tool surface와 unavailable 이유를 제공한다. 010의 user-local enable·공식 container 시작 조건을 유지하며 실제 session·mode·그림 조건은 Scope 1의 호출 context를 쓴다. `runtime_capability_ceiling_from_registry`(`crates/shacs-cli/src/lib.rs`)에 browser 분기를 더해 이 PRD에서는 `net_outbound`를 광고한다. 030은 같은 분기에 `browser_act`도 더한다. side-effect tool이 금지된 run에서는 등록하지 않는다. deferred discovery에도 같은 schema를 제공한다. `build_subagent_tool_registry`와 workflow 자식의 허용 목록에는 추가하지 않고 부모 host handle도 넘기지 않는다. registry를 다시 만들어도 host는 유지한다.
10. **실제 Docker 확인:** opt-in `scripts/spec049-browser-check.sh`를 만든다. 기본 Cargo test와 CI에서 실행하지 않는다. 실행 전 사용자 승인을 받고, disposable data·workspace·config와 별도 Compose project/network를 만든다. 정적 test-page container를 따로 띄워 JavaScript 입력·클릭·popup·큰 그림·download/file/password·내부 주소 이동 fixture를 제공한다. 그 container의 고정 IPv4 하나만 시험 config `tools.ssrfWhitelist`에 `/32`로 넣는다. shacs-bot과 같은 container에서 페이지를 제공하거나 loopback/container 전체 CIDR을 허용하지 않는다.
    - 일반 창과 Xvfb, 입력→클릭→결과, turn 간 이어짐, popup 전환, 많은 그림에서도 탭 유지, profile 삭제를 확인한다. managed policy 파일의 경로·내용과 Chromium의 적용 상태도 기록한다.
    - 별도 test page 주소만 열리고 shacs-bot container의 API·WebSocket·VNC 주소는 열리지 않는지 proxy와 CDP 실패로 확인한다. viewer 구현을 요구하지 않도록 loopback의 가짜 VNC listener로도 확인할 수 있다. DevTools listener가 없는지도 확인한다.
    - 종료 시 해당 project와 임시 자료만 정리한다. 사용자가 실행을 거절하면 결정·이유·확인하지 못한 Docker 동작을 Closure Evidence에 적어 `Complete (Scoped)`로 닫을 수 있다. 실행 실패를 거절 결정으로 바꾸지는 않는다.
11. **문서:** `docs/USAGE.md`, README의 도구·Docker 문단에 schema 사용 예, revision 수명, 그림 설정, 임시 판정 제한, 그림 보관, password/file 불가, canvas·자동화 차단 한계와 Docker opt-in 명령을 적는다.

## Non Scope

- 최종 `browser_act`·classifier 설명·domain 규칙(PRD 030), 화면 보기(PRD 040), takeover·로그인·capture 별도 동작(050), 웹 감시(051).
- 좌표 tool, provider computer use, 임의 JS 실행, 업로드·다운로드·로그인·캡차 우회.
- 그림 변환·압축·보관 정리와 기존 000 저장 계약 변경.

## Parent Requirement Mapping

- [049 SPEC](../SPEC.md) 소유 범위 4, 7 중 `screenshots`, 8 중 자기 문서, 수용 조건 2·6·8을 소유한다.
- 수용 조건 3 중 `open`의 non-http 거절·password/file 요소 거절과 010 primitive를 실제 tool 결과에 적용하는 것을 이 PRD에서 확인한다. 세 겹 경계의 image policy·읽기 guard 자체는 010 소유다.
- SPEC "앞선 스펙에서 바꾸는 것"의 추가 행은 소유하지 않는다. 010의 030·022 시작 허락을 소비하며 030이 소유한 022 plan·비대화형 bypass 행과 capability·048 확장은 아직 하지 않는다. 이 PRD의 임시 bypass guard도 대화형·비대화형을 구분하지 않는다.

## Required Contract

1. 번호 목록이 조작 대상의 기준이며 유효 revision 없는 요소 동작은 실행되지 않는다. click은 올바른 frame의 hit-test에서 그 요소 또는 자손이 확인된 점에만 입력한다.
2. 모든 화면 결과는 010의 전후 검사 뒤 반환한다. text/image_url 배열만 000에 넘긴다.
3. 그림을 받지 않는 모델도 같은 글 목록으로 조작하며 기본값에서는 캡처 비용을 내지 않는다.
4. password·file 입력과 non-http open은 거절하고 하위 작업에는 tool을 주지 않는다.
5. 030 이전 페이지 변경은 명시적 bypass 시험·실행에서 대화형·비대화형 모두 가능하다. 종료에 030·040을 요구하지 않는다.
6. `Cargo.lock`은 바뀌지 않으며 Cargo test는 Chromium·Docker 없이 돈다.
7. session·permission snapshot·대화형 여부·그림 조건은 실제 호출의 `BrowserCallContext`로 host에 전달한다. 직접·deferred·승인 실행이 같고 registry나 model 인자로 대신하지 않는다. production ceiling에는 `net_outbound`가 있다.

## Acceptance Criteria

1. `crates/shacs-core/tests/it/spec049_browser_tool.rs`의 `agent_loop_operates_numbered_browser_across_turns`: 가짜 provider·010 가짜 DevTools host, bypass 시험 설정으로 실제 `AgentLoop`에서 open→look→type→click→look을 수행한다. 다음 turn도 같은 generation이고 새 revision 뒤 지난 click은 실행되지 않는다. 글과 000의 저장 참조가 세션에 남고 provider에 그림이 붙는다.
2. 같은 파일의 `screenshots_and_overlay_follow_run_settings`: 그림 가능/불가·설정 없음/false/true 조합을 표로 확인한다. 캡처 때만 overlay 삽입→capture→제거가 순서대로 오고 실패에도 제거한다. 그림이 없어도 같은 조작 흐름이 완료된다.
3. 같은 파일의 `element_visibility_and_input_boundaries`: AX/DOM fixture에서 숨김·viewport 밖·disabled 제외, 번호/역할/이름/값/잘림, DOM 변경과 stale revision, password autocomplete·file input·금지 URL·금지 key·잘못된 인자를 확인한다. 번호 대상 위에 다른 요소가 고정돼 겹치는 fixture에서는 box가 보여도 hit-test가 `browser_target_covered`를 돌려주고 CDP 입력이 0개다. 대상의 자손 hit는 허용하고 잘못된 frame·자식 frame을 덮은 상위 요소는 거절한다. 읽는 사이 loader 변경은 내용 없는 오류와 tab close다.
4. 같은 파일의 `navigation_popup_wait_and_temporary_policy`: popup이 이전 탭을 닫고 번호를 갱신하며 back·scroll·wait·close가 schema대로 동작한다. auto/default/plan/dont_ask의 임시 페이지 변경 거절과 대화형 bypass·비대화형 bypass의 실행을 확인한다. 비대화형의 non-bypass는 거절하고 context 누락/불일치는 host 호출 전에 거절한다.
5. `crates/shacs-cli/tests/it/spec049_browser_registry.rs`의 `production_surface_excludes_children`: 공용 production registry·deferred schema와 disabled/native unavailable 결과, registry 재생성 후 공유 host, subagent·workflow 자식의 browser 부재를 확인한다. `crates/shacs-config/tests/it/spec049_browser_config.rs`에 screenshots 없음/true/false 저장 형식 검사를 보탠다. 각각 기존 `tests/it/main.rs`에 등록하고 새 test binary는 만들지 않는다.
   - 같은 CLI 시험 모듈의 `production_adapter_passes_per_call_browser_context`는 실제 production adapter·가짜 provider/host로 같은 host를 쓰는 두 session을 돌린다. A는 대화형 auto와 그림 불가, B는 비대화형 bypass와 그림 가능으로 서로 다른 effective run snapshot을 갖는다. A의 open/close, B의 busy 거절 후 open/type을 확인하고 host가 각 호출의 session·mode·대화형 여부·그림 조건을 받는지 검사한다. 직접·deferred·승인 후 실행 경로를 포함하며 승인 실행은 원래 run snapshot을 보존한다. registry 생성 시 mode와 실제 run mode를 다르게 두어도 임시 act guard·그림 선택이 실제 run을 따르고 session 소유권이 섞이지 않는다. 같은 시험에서 `runtime_capability_ceiling_from_registry`의 browser `net_outbound` 포함을 확인한다.
6. 사용자 승인 후 `scripts/spec049-browser-check.sh`가 Scope 10을 확인한다(부모 수용 조건 6). 거절하면 결정과 미검증 범위를 명시해 scoped로 닫는다.
7. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 전체 workspace test는 사용자 승인 때만 한다. 사용 문서와 `Cargo.lock` 무변경을 확인한다.

## Closure Evidence

1. 실제 AgentLoop turn의 가짜 provider 호출·DevTools 명령 순서·세션의 참조 글 예. base64 원문은 기록하지 않는다.
2. 겹친 요소의 입력 거절 fixture, 두 session의 production 호출 context와 ceiling 전달, production/child 등록, 대화형·비대화형 bypass와 그 밖의 임시 판정 제한의 시험 증거.
3. Docker script 결과와 격리된 시험 주소·정리 기록, 또는 사용자의 미실행 결정과 그 이유·미검증 범위.
4. 문서 diff, Cargo 검증 기록과 `Cargo.lock` 확인.

PRD 020은 bypass에서의 조작과 다른 mode의 명시적 거절까지로 독립 종료한다. 030이 닫히기 전 기능 제한을 최종 권한 계약이라고 설명하지 않는다.

## 남은 제한

- 030 전 auto/default/plan에서는 클릭·입력·키 조작을 쓸 수 없다.
- 접근성 정보 없는 canvas와 가려진 요소는 다루지 못한다. 페이지가 번호표를 바꿀 수 있고 DOM은 확인 직후에도 바뀔 수 있다.
- press는 page key event만 지원하고 주소창·브라우저 메뉴 조작은 없다. password 값 마스킹은 페이지가 별도 일반 글로 공개한 비밀까지 찾아내지 못한다.
- 캡처는 AI provider로 가고 media에 남는다. 본 PRD는 오래된 캡처를 지우지 않는다.
- Docker 확인을 거절한 scoped 종료는 실제 Chromium·policy 적용·메모리 사용의 보장이 아니다.
