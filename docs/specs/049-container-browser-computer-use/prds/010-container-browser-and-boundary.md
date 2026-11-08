# PRD 010. container 브라우저와 경계

Status: Planned

Depends on: [PRD 000](000-tool-result-images.md)의 닫힌 결과. PRD 020–040과 050·051의 구현에 의존하지 않는다.

## Goal

공식 container 안에서만 가상 모니터와 임시 Chromium 하나를 안전한 연결 경계 안에 띄운다. 뒤 PRD가 쓰는 최소 host 계약과 문서를 읽는 전후 검사까지 만들고, 실제 tool이나 화면 보기 없이 가짜 실행기와 DevTools로 닫는다.

## 지금 상태

- 공식 runtime image는 `python:3.14-slim-bookworm`이고 설치 목록에 Chromium과 Xvfb가 없다(`Dockerfile`의 `runtime` stage). gateway·api의 메모리 한도는 1G, 예약은 256M이며 `user:`는 덮어쓸 수 있다(`docker-compose.yml`의 `common-config`, `shacs-gateway`, `shacs-api`).
- `ToolsConfig`에는 `browser`가 없고 SSRF 허용 목록은 `ssrf_whitelist`다. camelCase 직렬화 이름은 `tools.ssrfWhitelist`다(`crates/shacs-config/src/lib.rs`의 `ToolsConfig`). `onboard_config_defaults`와 `merge_missing_json_defaults`는 빠진 기본값만 채우므로 browser stub도 이 길을 쓴다(`crates/shacs-cli/src/lib.rs`).
- `NetworkGuard::is_private`는 주소 허용 목록을 먼저 적용한다. `validate_host_target`은 `localhost`를 거절하고 DNS의 각 주소를 검사하지만, 검사한 소켓에 연결하는 proxy는 아니다(`crates/shacs-security/src/lib.rs`). 허용 목록은 포트를 구별하지 않는다.
- `ProcessAdapterKind`에는 다섯 종류만 있다(`crates/shacs-core/src/runtime/process_envelope.rs`의 동명 enum). `mcp_startup_gate`가 시작 판정 재료를 만들고 `StdioMcpConnector::connect`가 `ProcessGate::evaluate_and_maybe_spawn` 안에서 실행한다(`crates/shacs-cli/src/lib.rs`, `crates/shacs-core/src/tools/mcp.rs`).
- `ProcessGate::evaluate_and_maybe_spawn`는 모든 adapter의 static rule 결과를 `decide_permission`에 넘기고 `Allow`와 containment proof를 모두 요구한다(`crates/shacs-core/src/runtime/process_gate.rs`). browser만의 user-local 설정 기반 시작 허락은 아직 없으므로 MCP 시작의 mode 판정을 그대로 복사해서는 부모의 시작 계약을 채우지 못한다.
- `official_runtime_package_marker_observed`는 환경 표식만 확인하고, containment는 별도의 `runtime_containment_inspect`가 만든다(`crates/shacs-cli/src/lib.rs`). 표식만으로 격리를 증명할 수 없다.
- `production_tool_registry`는 새 `ToolRegistry`를 만든다(`crates/shacs-cli/src/spec035_cli_media/tool_registry.rs`). tool 객체의 수명으로 브라우저 수명을 정하면 turn 사이에 이어 쓸 수 없다.
- `runtime_inspect_inner`는 다른 process에서도 소유 상태를 관찰한다(`crates/shacs-cli/src/spec035_cli_media/runtime_inspect.rs`). `RuntimeOwnershipMarker`, `runtime_owner_id`, `read_runtime_ownership_marker`는 PID와 시작 시각을 포함한 owner 식별자와 lease를 검증한다(`crates/shacs-cli/src/lib.rs`). process 안 static만으로 별도 inspect에 busy를 전할 수 없다.
- CLI에는 `libc`, `tungstenite`, `ureq`, `serde_json` 의존성이 있고 core에는 `libc`, `base64`, `serde_json`이 있으나 `tungstenite`는 없다(각 `Cargo.toml`의 `[dependencies]`). core에 WebSocket 의존성을 옮길 이유가 없다.

## Scope

1. **배치:** 실제 Linux process 실행기, CDP 수송, proxy, process 전체 browser slot은 `shacs-cli`에 둔다. core에는 `BrowserHost`/`BrowserSession` trait과 요청·결과 자료만 둔다. host는 세션 소유권 획득, 현재 문서 확인, 검증된 읽기, 동작, 닫기만 제공한다. 호출 자료 `BrowserCallContext`는 `effective_session_id`, `permission_mode_snapshot`, `permission_interactive`, `browser_screenshots: Option<bool>`, `native_image_input_supported: bool`을 담고 host가 호출마다 빌려 받는다. 이 PRD는 host 계약의 자료형만 만들고, tool 실행 context를 확장해 같은 run snapshot에서 채우는 일은 020이 소유한다. 이 자료는 process 시작 허락의 출처가 아니다. 가짜 host는 core 시험에 둔다. 새 crate, Cargo 의존성, `Cargo.lock` 변경은 없다. 하위 모듈 선언과 구현을 분리하고 새 Rust는 Google Rust 스타일을 따른다.
2. **image와 자원:** Debian package `chromium`, `xvfb`, `xauth`, `fonts-noto-cjk`를 runtime stage에 설치한다. gateway·api는 각각 memory limit `2G`, reservation `512M`, `shm_size: "256mb"`로 정한다. Chromium은 이 공유 메모리를 쓰며 `--disable-dev-shm-usage`를 함께 쓰지 않는다. CLI service의 한도는 바꾸지 않는다.
3. **시작:** 사용자가 user-local config의 `tools.browser.enable`을 켠 것을 browser generation을 시작할 허락으로 본다. runtime이 그 설정의 출처를 확인해 gate 입력에 묶으며 workspace config, model 인자, tool registry 구성 시 추정한 값으로 대신하지 않는다. 공식 package 표식과 기존 030 containment proof, 실행 파일·정책 파일의 준비 확인도 필요하다.
    - `ProcessAdapterKind::Browser`(직렬화 `browser`)를 더한다. `mcp_startup_gate`의 envelope·receipt·containment proof 구성은 재사용하되, `ProcessGate::evaluate_and_maybe_spawn`에 browser 전용 시작 판정을 구현한다. 유효한 user-local enable 증거가 있는 browser adapter는 030 격리 증명과 static deny 규칙만으로 시작을 판정한다. static deny가 있으면 거절하고, 없고 격리 증명이 유효하면 시작한다. permission mode·대화형 여부·classifier·사용자 질문은 시작의 조건이 아니며 `decide_permission`의 mode baseline이나 static ask가 시작을 다시 막지 않는다. envelope의 무결성·terminal precondition·receipt 검증은 유지하고 다른 adapter의 판정 경로는 바꾸지 않는다.
    - 시작 action은 기록상 `proc_exec`로 남기되 이 capability의 일반 mode 판정을 시작 허락으로 쓰지 않는다. 한 generation의 xauth·Xvfb·Chromium 및 선택적 viewer 자식 묶음은 gate가 허락한 closure 안에서 한 번 시작한다. 거절되면 이유를 돌려주고 자식을 만들지 않는다. enable은 process 시작만 허락하며 각 browser action의 허용·승인·거절을 대신하지 않는다. tool 동작은 process gate를 다시 부르지 않는다.
4. **임시 디렉터리와 실행 인자:** `/tmp/shacs-browser-<무작위>/`를 0700으로 만들고 profile, Xauthority, 임시 HOME·XDG 폴더를 그 안에 둔다. 현재 UID를 사용하며 home의 소유권을 바꾸지 않는다. `xauth`로 만든 cookie 파일로 `Xvfb :<빈 번호> -screen 0 1280x720x24 -nolisten tcp -auth <파일>`을 띄운다. `-ac`는 쓰지 않는다. Chromium은 `--display=:<번호> --user-data-dir=<임시 profile> --no-first-run --no-default-browser-check --no-sandbox --remote-debugging-pipe --proxy-server=socks5://127.0.0.1:<임시 port> --proxy-bypass-list=<-loopback> --disable-quic --force-webrtc-ip-handling-policy=disable_non_proxied_udp --disable-background-networking --disable-component-update --window-size=1280,720 about:blank`로 일반 창을 연다. headless는 쓰지 않는다. 초기 빈 탭은 읽지 않고 첫 http(s) 이동 전까지 내부 준비 상태로만 둔다.
    - generation의 모든 자식(`xauth`, Xvfb, Chromium, 040의 x11vnc·websockify)은 `env_clear`로 빈 환경에서 시작한다. allowlist는 필요한 `PATH`, `LANG`·`LC_ALL`·`LC_CTYPE`, `DISPLAY`·`XAUTHORITY`, 임시 `HOME`·`XDG_CONFIG_HOME`·`XDG_CACHE_HOME`·`XDG_DATA_HOME`·`XDG_RUNTIME_DIR`·`TMPDIR`뿐이다. 해당 자식에 필요한 key만 명시적으로 채우고 XDG/TMPDIR 경로는 generation의 0700 임시 폴더 아래로 한정한다. 부모 환경 전체나 provider credential, viewer password를 풀 때 쓴 `${NAME}` 원본 환경 변수, `.env.json` 값을 상속하지 않는다. 040도 이 공통 생성 규칙을 사용한다.
5. **CDP 수송:** 권장 선택은 `--remote-debugging-pipe`다. Linux의 기존 `libc`와 표준 pipe로 자식 FD 3(입력)·4(출력)을 연결하고 NUL로 끝나는 JSON을 request id·session id로 대응한다. 별도 I/O worker가 event와 응답을 나누고 deadline과 EOF를 처리한다. HTTP discovery·WebSocket port를 열지 않으므로 페이지, proxy, 외부 client가 DevTools endpoint로 연결할 수 없다. CLI의 기존 blocking `tungstenite`는 다른 기능에 그대로 두며 browser에는 필요하지 않다. 이 선택은 새 의존성 없이 loopback DevTools port를 허용 목록에서 다시 보호해야 하는 문제를 없앤다. transport가 달라도 같은 가짜 DevTools peer로 시험한다.
6. **SOCKS5:** `std::net`의 loopback 임시 listener에서 인증 없는 CONNECT/TCP만 받는다. UDP ASSOCIATE와 BIND는 거절한다. hostname을 소문자화하고 끝의 점을 없애 `localhost`·그 하위 이름을 거절한다. 주소 literal은 `NetworkGuard::is_private`로 검사한다. hostname은 한 번만 resolve하고, 비어 있거나 하나라도 `is_private`면 연결 전체를 거절한다. 통과한 `SocketAddr` 중 하나에 직접 연결하며 두 번째 DNS 조회를 하지 않는다. 재연결 때는 다시 resolve·검사한다. IPv4·IPv6 모두 같다. `tools.ssrfWhitelist`를 그대로 소비하고 주소 단위 허용 의미를 바꾸지 않는다. loopback·같은 container 주소는 기본 guard에서 막히며, 사용자가 그 주소를 허용하면 그 주소의 모든 port가 열릴 수 있음을 문서에 쓴다. DevTools는 port가 없으므로 이 허용에도 노출되지 않는다.
7. **내부 주소 정책:** image의 Debian Chromium managed policy 경로 `/etc/chromium/policies/managed/shacs-browser.json`에 다음 JSON을 root 소유 0644로 둔다. Chromium 원본 `components/policy/core/common/policy_paths.cc`의 `policy::kPolicyPath`는 비 Chrome Linux build에서 `/etc/chromium/policies`다. Debian bookworm image에서 managed 하위 배치와 실제 적용 여부는 PRD 020 Docker 확인에 포함한다.

   ```json
   {
     "URLBlocklist": ["*"],
     "URLAllowlist": ["http://*", "https://*"],
     "DownloadRestrictions": 3,
     "DefaultFileSystemReadGuardSetting": 2,
     "DefaultFileSystemWriteGuardSetting": 2
   }
   ```

   정책은 추가 방어다. `chrome:`의 정책 예외와 navigation notification의 한계 때문에 이것을 유일한 차단으로 설명하지 않는다. 초기 `about:blank`는 읽기 대상이 아니며, 정책 적용을 위한 예외 allowlist를 더하지 않는다.
8. **검증된 읽기 primitive:** 활성 target/session과 주 frame id, URL, loader id를 `Page.getFrameTree`로 읽고 URL을 파싱해 http(s)인지 먼저 확인한다. event를 받으면서 같은 문서의 접근성·보이는 글·요소 위치·선택적 캡처를 수집한다. 직후 다시 frame tree를 읽는다. target/frame/loader/URL이 달라졌거나 `frameNavigated`·detach로 문서 변경이 관찰됐거나 한 번이라도 비 http(s)면 전체 payload를 버리고 탭을 닫는다. 실패 결과에 URL 원문·제목·요소·글·그림을 넣지 않는다. 오류 이름과 다시 열라는 안내만 반환한다. screenshot·AX 결과를 먼저 event나 로그로 내보내지 않는다. PRD 020은 모든 화면 결과를 이 primitive 안에서만 만든다. 탭이 닫히면 요소 번호도 무효다.
9. **파일 이동 거절:** Browser session 설정 시 `Browser.setDownloadBehavior`의 `behavior: "deny"`를 모든 context에 적용하고 새 target이 열릴 때도 확인한다. managed `DownloadRestrictions`는 이를 보강한다. `Page.setInterceptFileChooserDialog`를 켜고 chooser event에는 파일을 넘기지 않으며 페이지를 닫고 거절 결과를 만든다. host 계약에 파일 경로나 `DOM.setFileInputFiles` 동작을 두지 않는다. File System Access read/write는 정책으로 거절한다. PRD 020은 file input 요소의 click·type도 실행 전에 거절한다.
10. **한 자리와 시간:** process 전체 `OnceLock`으로 관리자를 하나 두고, manager가 browser generation·session key·활성 target·마지막 사용 시각을 소유한다. 다른 session은 기다리지 않고 `browser_busy: 다른 세션이 브라우저를 쓰는 중입니다`를 받는다. session 식별자는 모델 결과에 노출하지 않는다. 같은 session의 동시에 온 동작도 겹쳐 실행하지 않는다. 시작 timeout은 기본 20초(허용 5–60초), idle close는 300초(60–1800초), 페이지 한 동작의 전체 한도는 30초(5–120초)다. 초과하면 해당 generation을 닫고 이유를 반환한다. idle 시계는 tool 호출 시작·완료에 갱신하며 background timer가 닫는다. turn 종료만으로 닫지 않는다.
11. **수명:** proxy → Xvfb → Chromium 순서로 준비하고 부분 실패면 시작한 것을 역순 정리한다. 정상 close·idle·timeout·종료 신호·Chromium 또는 Xvfb EOF/사망에서 viewer, Chromium, Xvfb, proxy를 종료하고 wait/reap 후 임시 폴더를 지운다. 소유 process 종료에 Linux parent-death 처리를 적용해 자식이 남지 않게 한다. viewer만 죽으면 browser를 유지한다. 다음 호출의 lazy start는 새 generation이다. viewer용 최소 lifecycle participant는 승인 closure 안 start, 종료 시 stop, viewer-only failure 보고 세 가지이고 기본은 아무 participant도 없다. 040이 이것을 채우며 이 PRD는 viewer를 실행하지 않는다. 050의 takeover와 051 watcher는 구현하지 않는다.
12. **설정과 상태:** `tools.browser`에 `enable: false`, `startupTimeoutSeconds: 20`, `idleTimeoutSeconds: 300`, `pageTimeoutSeconds: 30`을 더한다. 숫자는 범위 밖이면 config 오류다. onboard는 기존 값을 보존하며 이 stub을 채운다. `screenshots`와 `view`의 자료형·stub은 각 owner인 020·040에서 더한다.
    - inspect·diagnostics 공통 browser 필드는 `enabled`, `available`, `reason`, `state`(`disabled`, `unavailable`, `idle`, `starting`, `running`, `busy`, `failed`, `unknown`), `ownerObserved`, `updatedAtMs`다. URL·페이지 내용·profile 경로는 담지 않는다. 실제 호출이 다른 session에 막힌 경우 busy를 결과와 diagnostics에 남긴다.
    - 별도 inspect는 data 폴더 `runtime/browser-state.json`의 0600 원자적 snapshot을 읽는다. 현재 유효한 `RuntimeOwnershipMarker`의 `owner_id`, PID, 시작 시각, config/workspace와 일치하는 generation의 snapshot만 받는다. owner가 바뀌거나 lease가 끝나면 `unknown`으로 보고 이전 busy를 재사용하지 않는다. snapshot의 holder 존재는 busy 사실이며, 보는 caller에 따라 실제 접근 가능 여부는 host가 판정한다. owner marker 없는 일회 실행은 process 내 diagnostics만 제공하고 별도 inspect는 `ownerObserved: false`, `state: unknown`이다. snapshot 읽기 실패를 idle로 바꾸지 않는다.
13. **문서:** `docs/USAGE.md`와 README의 Docker·도구 설명에 opt-in, 공식 container 조건, 한 session, 시간 한도, 임시 profile, 주소 단위 whitelist, sandbox 꺼짐과 같은 container의 위험을 적는다. 아직 tool·viewer가 제공되지 않는 종료 표면도 명시한다.

## Non Scope

- `browser` 등록·요소 번호·`open`의 URL 입력 거절(PRD 020), `browser_act`와 승인 판정(PRD 030), 실제 viewer와 비밀번호(PRD 040).
- 실제 Docker 종료 시험(PRD 020), 로그인·takeover·영구 profile(050), 감시 전용 브라우저 정책(051).
- Chromium sandbox 활성화, 별도 browser container, 임의 CDP 명령 공개.

## Parent Requirement Mapping

- [049 SPEC](../SPEC.md) 소유 범위 2, 3, 7 중 `enable`·한도, 8 중 browser 상태·자기 문서를 소유한다. 수용 조건 3의 runtime 경계·실행·정리와 8을 확인한다.
- 수용 조건 3의 tool 입력 URL·비밀번호 칸·file input 거절은 020 몫이다. 이 PRD에서는 그 아래의 policy·검증된 읽기·파일 이동 거절 primitive를 닫는다. 실제 Docker 경계는 020의 수용 조건 6이다.
- 바꾸는 앞선 계약: SPEC "앞선 스펙에서 바꾸는 것"의 030 실행 종류 행과 030·022 시작 판정 행을 소유한다. browser adapter를 더하고, 그 adapter의 시작만 user-local enable 허락·030 격리 증명·static deny로 판정한다. 다른 adapter는 기존 permission 판정을 유지하며 각 browser action의 판정은 030 PRD 소유다.

## Required Contract

1. user-local enable이 시작 허락이며 공식 package 표식·030 containment proof·static deny 검사를 통과해야 시작한다. mode·대화형 여부·classifier에 기대지 않고 한 generation을 한 번 판정한다. workspace config와 model 인자는 허락이 아니다.
2. browser tool은 process adapter를 보고하지 않는다. host의 시작 action만 `proc_exec`다.
3. 연결은 한 번 resolve한 주소 전부를 검사하고, 검사한 주소로만 이어 준다. DevTools TCP endpoint는 없다.
4. 페이지 내용은 같은 http(s) 주 문서의 읽기 전후 검사를 모두 통과한 뒤에만 외부로 나온다.
5. profile은 `/tmp`에만 만들고 정상 정리 뒤 남지 않는다. viewer가 없어도 수명이 완결된다.
6. 현재 owner와 맞지 않는 상태 파일로 running·busy를 주장하지 않는다. `Cargo.lock`은 바뀌지 않는다.
7. 모든 generation 자식은 Scope 4의 빈 환경·allowlist 규칙을 따른다. provider credential과 viewer password source 변수는 어떤 자식에도 상속하지 않는다.

## Acceptance Criteria

1. `crates/shacs-cli/tests/it/spec049_browser_runtime.rs`의 `startup_gate_and_process_lifecycle`: 실제 gate와 가짜 process runner로 browser가 없는 상태에서 user-local enable을 켜고 공식 container 증명을 제공한다. 대화형 auto·default, 비대화형 auto·default·bypass 각각에서 classifier·승인 질문 없이 xauth·Xvfb·Chromium이 한 번의 gate 안에서 시작되는지 확인한다. disabled·workspace-only enable·model의 enable 주장·container 밖·proof 불충족·static deny에서는 자식 spawn이 0회다. browser static ask나 mode baseline이 시작을 막지 않으며 다른 adapter는 기존 mode 판정을 유지한다. 두 호출과 registry 재생성 뒤 같은 generation이고 다른 session은 즉시 busy다. argv·0700 profile·비 TCP Xvfb·shm 설정도 확인한다. 부모 환경에 provider credential·viewer password source 비밀 표식을 넣고 xauth·Xvfb·Chromium·fake viewer participant가 시작한 자식 모두의 환경이 Scope 4 allowlist뿐이며 표식이 없는지 검사한다.
2. 같은 파일의 `proxy_checks_all_resolved_addresses_without_relookup`: 가짜 resolver/connector로 공개 주소, 혼합 공개·사설 주소, localhost 계열, IPv4·IPv6 loopback, container API·WebSocket 주소, whitelist `/32`를 시험한다. DNS 1회와 검사한 `SocketAddr`로의 연결, 주소 허용이 port에 한정되지 않음을 확인한다. SOCKS BIND·UDP는 거절된다.
3. 같은 파일의 `document_guard_discards_changed_and_internal_documents`: 가짜 DevTools peer로 정상 읽기와 loader·URL·target 변경, 내부 주소 이동, 읽기 중 navigation event를 넣는다. 실패 payload·event·로그에 읽은 글과 캡처가 없고 탭이 닫힌다. deny download·intercept chooser 설정도 같은 peer transcript에서 확인한다.
4. 같은 파일의 `timeouts_cleanup_and_owner_snapshot`: 시험용 시계로 시작·idle·페이지 한도, 부분 시작 실패, browser/Xvfb 사망과 정상 종료의 자식 정리·profile 삭제를 확인한다. fake viewer participant 사망은 browser를 유지한다. 별도 reader가 유효 owner snapshot을 읽고 이전 owner·만료·읽기 오류를 unknown으로 보는지 확인한다.
5. `crates/shacs-config/tests/it/spec049_browser_config.rs`의 `browser_defaults_and_limits`, `crates/shacs-cli/tests/it/spec049_browser_runtime.rs`의 `onboard_and_inspect_browser`: 기본값·범위 오류·기존 설정 보존, 상태의 JSON/글 형식과 diagnostics 대응, image policy의 경로·JSON과 Compose 2G/512M/256mb를 확인한다. 새 test binary 없이 각 `tests/it/main.rs`에 등록한다.
6. fmt → workspace clippy `-D warnings` → 변경한 package(`shacs-config`, `shacs-core`, `shacs-cli`, 상태 자료형 변경 시 해당 package)의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 실행한다. `--locked`를 사용하고 전체 workspace test는 별도 사용자 승인 없이는 실행하지 않는다. 문서와 `Cargo.lock` 무변경을 확인한다.

## Closure Evidence

1. fake process runner·DevTools peer·resolver의 시험 경로와 결과, auto·default·비대화형 최초 시작의 gate receipt와 classifier/질문 부재, static deny 거절, 전체 자식 환경의 비밀 표식 부재, 내용 폐기와 정리 증거. bypass에서만 시작한 증거로는 이 PRD를 닫지 않는다.
2. 현재 owner와 이전 owner의 inspect 예, 비밀·페이지 내용 없는 diagnostics 예.
3. image policy·실행 인자·Compose·사용 문서 diff와 Cargo 검증 기록. Debian policy 실제 적용과 실제 메모리 사용은 020 Docker 증거로 남긴다.

PRD 010은 `BrowserHost`를 직접 쓰는 가짜 시험으로 닫는다. PRD 020–040의 구현을 종료 조건에 넣지 않는다.

## 남은 제한

- Chromium sandbox는 꺼지고 같은 UID가 data 폴더에 닿으므로 브라우저 취약점이 container 안 비밀을 읽을 수 있다.
- policy만으로 내부 주소 차단을 보장하지 않는다. 내용 반환의 보장은 주 문서 전후 검사다. chooser는 열리기 전에 취소할 CDP 응답이 없어 event 수신 시 탭을 닫는다.
- whitelist는 주소 전체를 허용한다. 사용자가 같은 container 주소를 넣으면 그 port들을 보호한다는 기본 보장도 풀릴 수 있다.
- 강제 SIGKILL·host 전원 중단에서는 임시 폴더가 남을 수 있다. 다음 시작이 다른 실행의 폴더를 임의로 지우지는 않는다.
- 한 process의 한 자리이고 CLI 일회 실행의 live 상태는 별도 inspect로 보장하지 않는다.
