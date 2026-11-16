# PRD 000. 브라우저로 읽는 관측

Status: Implemented (independent review approved 2026-11-12)

Depends on: [042](../../042-conversational-web-watch-and-trustworthy-silence/SPEC.md)의 닫힌 결과(관측 계획·미리보기·확인·판정·실패 구간·알림), [049](../../049-container-browser-computer-use/SPEC.md)의 container 브라우저·process gate·SOCKS5 proxy, [050](../../050-browser-takeover-and-saved-login/SPEC.md)의 process 전체 자리·조작 상태·공통 잠금과 닫기를 소비한다. 010 network guard, 029 durable wake·lease, 036 관측 fact를 그대로 쓴다. 이 스펙의 PRD 010에 의존하지 않는다.

## Goal

사용자가 CLI/API에서 읽기 방식 `browser`로 미리보고 확인한 페이지를, watcher가 브라우저로 열어 그려진 글에서 읽는다. 조건·실패 구간·알림은 042 그대로이고, 대화가 브라우저를 쓰는 동안의 확인은 `browser_busy`로 남는다. 이 PRD만으로 로그인하지 않는 감시의 등록·관측·조회·실패와 회복 알림까지 끝난다.

## 지금 상태

- `crates/shacs-core/src/runtime/web_watch.rs`의 `WebWatchPlan`에는 URL·`WebExtraction`·조건·주기·첫 wake만 있고 `digest`는 그 구조의 JSON을 SHA-256으로 만든다. `WebExtraction.version`은 `normalized_static_text_v1` 하나다. `web_watch_surface.rs`의 `CandidateToken`, `decode_token`, `confirmation_candidate`, `store_confirmed_watch`도 이 전제다.
- `web_watch_fetch.rs`의 `read_page`는 `ureq`로 redirect 한 경로를 읽고 `WebFetchLimits::PRODUCTION`(20초·1 MiB)을 적용한다. `check_plan`이 HTTP 분류와 추출을 한다. `web_watch_text.rs`의 `normalize_fragment`가 NFC·공백 정규화이고, HTML 제거·entity 해석은 `normalized_static_text_v1`에 따로 있다.
- `web_watch_dispatch.rs`의 `DurableWorkDispatcher::process_web_watches`가 `WebWatchExecutor::prepare` 뒤 임대하고 `check_plan`을 직접 부른다. 읽는 동안에는 세션 mutation lock을 놓고, 결과 저장 전 다시 lineage·취소를 확인한다. `WebWatchRuntime`에는 브라우저 reader가 없다.
- `web_watch_decision.rs`의 `build_record`는 `text_version`을 정적 버전으로 고정한다. `web_watch_notify_render.rs`의 `render_web_notification`은 저장된 `WebNotifyIntent`만 읽지만, 확인 한계는 상수 `LIMITS` 하나다. 읽기 방식은 두 곳 모두에 없다.
- 실제 브라우저는 `crates/shacs-cli/src/browser/browser_manager.rs`의 `process_browser_host`가 `OnceLock<ManagedBrowserHost>` 하나로 만든다. `BrowserManager::start`는 설정의 `persist_login`과 `ConfigBrowserStartup::network_guard`를 사용한다. 그대로 호출하면 로그인하지 않는 감시까지 저장 프로필과 사용자의 `ssrfWhitelist`를 쓰게 된다.
- `CdpDocument::read_transaction`은 주 문서를 전후로 확인하지만, 요소 목록·접근성 트리와 선택적인 그림을 만드는 대화용 읽기다. load 이후 network idle, 주 문서 HTTP 응답·`Retry-After` 기록은 없다. 새 page target은 `apply_event`에서 현재 탭으로 바뀌며 앞 탭을 닫는다. `cdp_connection.rs`의 `PausedTargetAdmission`은 응답 대기 중에도 새 target을 처리하므로, 마지막 `apply_event`만 바꿔서는 감시의 새 탭 금지를 지키지 못한다.
- `BrowserManager::hold`는 기다리지 않지만 `reserve`는 takeover를 위해 page timeout + 5초까지 기다린다. `maintenance`는 100 ms마다 돌고, 그 안의 `watch_locks`는 1초마다 forget·경로 교체를 확인한다. slot을 못 잡으면 `Deferred.live.close`를 세울 뿐 진행 중인 DevTools 요청을 끊지는 않는다.

## 설계 결정과 근거 (2026-11-12)

1. **브라우저 읽기의 연결점.** core에 읽기 전용 경계 `RenderedWebReader`와 제한된 응답 `RenderedWebPage`를 더하고, CLI의 `ManagedBrowserHost`가 구현한다(이 이름들은 이 PRD에서 신설할 계약이다). `WebWatchRuntime`과 미리보기·확인에 같은 reader를 전달한다.
    - 다른 길: watcher가 `browser` tool을 호출하거나 별도 manager를 만든다. 전자는 provider·도구 판정을 끌어들이고, 후자는 한 process 한 브라우저와 화면 보기 자리를 깨뜨린다.
    - core는 Chromium을 모르고 기존 추출·판정·저장을 소유한다. CLI reader는 한 번의 시작·읽기·닫기와 그때의 공개 주소 정책만 소유한다. HTTP 읽기는 기존 함수를 그대로 쓴다.
2. **시작의 목적을 고정한다.** manager의 generation 목적을 대화와 감시로 구분하고, 감시는 시작 전에 프로필·proxy·새 탭 정책을 확정한다. 이번 PRD의 감시는 항상 `ProfileUse::Temporary`다. 설정의 `persistLogin`이 켜져 있어도 저장 프로필을 열지 않는다.
    - 다른 길: 전역 config를 잠시 바꾸거나 열린 대화 탭을 빌린다. 다른 thread와 다음 generation에 설정이 새고, 확인한 페이지와 계정의 경계가 사라진다.
    - SPEC의 2026-11-12 저장 예외 결정을 소비한다. 임시 프로필의 익명 cookie 생성·저장·전송과 사이트 자료, generation HTTP cache는 042 무저장 계약의 명시적 예외다. 브라우저 감시를 처음 시작하는 이 PRD가 임시 프로필과 HTTP cache 예외를 소유하며, 저장 프로필 예외는 PRD 010이 소유한다. 049·050보다 좁은 감시 전용 cookie/cache 제한은 두지 않는다.
3. **완료 대기의 뜻.** 처음 연 탭의 현재 주 문서에서 load를 확인한 뒤, 그 탭에 속한 진행 중 HTTP(S) 요청이 0인 상태가 연속 500 ms 유지되어야 읽는다. 전체 실행 예산과 본문 한도는 Scope 4에서 고정한다. DOM이 두 번 같다는 검사나 사이트별 selector는 쓰지 않는다.
    - load만 기다리면 load 뒤 fetch로 그리는 값을 놓친다. network idle도 모든 사이트의 최종 값을 증명하지는 못한다. 계속 요청하는 페이지는 한도에 닿아 실패하며, 조용해진 뒤 예약된 script는 남은 제한이다.
4. **점유와 조작 상태는 다른 값이다.** 감시는 `assistant` 조작 상태의 감시 소유 generation이다. `user`를 흉내 내지 않는다. slot 밖에서도 generation 번호와 감시 점유를 확인할 수 있게 하고, 그 표식은 시작 전부터 정리 완료까지 유지한다.
    - takeover는 감시 점유를 보면 기다리거나 예약하지 않고 `browser_busy`다. 대화 동작에 대한 050의 기다림은 그대로다. SPEC 소유 범위 4의 차이를 050 소비 표에 명시한다.
5. **기존 저장 자료의 뜻을 유지한다.** `fetch` 기본값은 읽을 때 보충하고 기존 계획 JSON에서는 생략한다. 그러면 이미 예약된 fetch wake의 `plan_digest`가 업그레이드만으로 달라지지 않는다. 새 browser 값은 직렬화하므로 digest가 다르다. 기존 token·알림 intent도 누락값을 fetch로 읽는다. 이미 저장된 자료를 모두 고치는 migration은 필요 없다.

## Scope

1. **계획과 외부 입력:** `WebWatchPlan`, `WebWatchPlanView`, `WebPreviewRequest`, `ConfirmWebWatchRequest`, `WebPreview`, `CandidateToken`에 `reading_mode: fetch | browser`를 더한다. 내부 타입은 enum이며 외부 문자열을 경계에서 검사한다. 누락은 fetch다.
    - `WebExtraction.version`은 fetch면 `normalized_static_text_v1`, browser면 `rendered_text_v1`이다. token의 방식·버전·URL digest와 확인 요청이 모두 맞아야 한다. 모르는 방식·버전, fetch token을 browser 확인에 쓰는 요청은 읽기 전에 거절한다. 기존 version 1 token은 누락된 방식을 fetch로만 읽고, browser token은 새 version 2로 만든다.
    - CLI의 `crates/shacs-cli/src/spec042_web_watch_cli.rs`에 `work web-preview`·`work web-watch`의 `--reading-mode fetch|browser`를 더한다. API의 `crates/shacs-api/src/spec042_web_watch.rs` preview·confirm body에도 같은 `reading_mode`를 더한다. 기존 인자와 기본 실행은 fetch다. 로그인 선택은 PRD 010 전까지 없다.
    - `preview_web_watch`, `confirm_web_watch`, `preview_page`, `read_confirming`, `read_value`가 한 읽기 선택 경계를 소비한다. browser reader가 없으면 `browser_unavailable`이며 fetch로 조용히 바꾸지 않는다.
    - 방식 변경도 `store_confirmed_watch`의 revision 증가·확인 순간 재읽기·새 baseline 경로를 탄다. 기존 wake는 `check_web_wake`의 digest·revision 확인에서 오래된 것으로 끝난다. 000의 사용자 확인은 CLI/API의 preview token 확인이다. 대화로 바꾸고 새 요약을 확인하는 표면은 PRD 010이 소유한다.
2. **운영 배선:** `RenderedWebReader`는 URL·읽기 제한을 받아 provider 없이 제한된 문서 응답 또는 분류된 실패를 돌려주는 경계다. 원문은 추출할 동안의 메모리에만 있고 serializable 관측 기록이나 tool 결과가 아니다. `RenderedWebPage`에는 잘린 글, 확인된 최종 URL·HTTP 상태·`Retry-After`, 잘림 여부만 둔다. request/response header 전체·cookie·화면·DevTools 원본 event는 돌려주지 않는다.
    - `spec035_cli_media/tool_registry.rs`의 `production_tool_registry`가 받는 `process_browser_host`의 같은 인스턴스를 `BrowserTool`과 reader 두 경계로 전달한다. `ProductionTooling` → `AgentLoopChatCompletionAdapter::from_bundle_with_trusted_extensions` → adapter에 reader를 보관한다. `allow_side_effect_tools`가 false이면 browser reader도 없다. 시험 전용 `with_browser_clients`만 고쳐서는 완료가 아니다.
    - 실제 gateway 경로는 `crates/shacs-cli/src/lib.rs`의 runtime ownership 획득 → `AgentLoopChatCompletionAdapter::from_bundle` → `GatewayDurableWork::new` → `GatewayDurableWork::start_worker` → `surface_approval_worker.rs::start_surface_approval_worker` → `automation_worker.rs::process_due_automation` → `AutomationOwnerAdapters::process_observations` → `process_web_watches`다. `process_observations`가 만드는 `WebWatchRuntime`에 adapter의 같은 reader를 넘긴다.
    - `loop_config_for`가 만드는 `WebWatchConversationConfig`에도 같은 reader를 전달할 자리를 마련한다. 000에서는 기존 fetch 대화 동작만 유지하며 자동 제안은 하지 않는다. API는 `crates/shacs-api/src/media_api/adapter.rs`의 `ChatCompletionAdapter`에 optional reader 접근 경계를 추가하고 CLI 구현이 같은 reader를 제공한다. 일회 CLI preview·confirm도 config로 `process_browser_host`를 받아 같은 경로를 탄다.
    - `observation_worker.rs`의 `WebWatchExecutor::prepare`와 `watcher_preflight`는 snapshot provenance·hook veto다. `automation_snapshot`이 `ProviderRequest` 값을 만들지만 provider를 호출하지 않는 현재 경계를 유지한다. 이 단계가 허용됐다는 것만으로 process gate를 통과한 것으로 보지 않는다.
    - worker와 ingress는 050의 `GatewayDurableWork`가 정한 같은 lease owner를 쓴다. `runnable_work_ids`·`lease_work`·결과 전 lineage 재확인·`record_web_watch_terminal`·다음 wake 예약을 유지한다. 외부 inbound loop에 자동화 pass를 다시 넣거나 새 queue·scheduler·임대 owner를 만들지 않는다.
3. **시작과 네트워크:** 감시도 `ConfigBrowserStartup::gate` → `startup.rs::browser_start_input` → `ProcessGate::evaluate_and_maybe_spawn`을 탄다. `UserLocalBrowserEnable::read`로 고른 user-local 파일의 enable과 permission rules를 다시 읽고, 공식 container·containment proof·static deny를 확인한다.
    - `browser_startup` envelope, `BrowserStartEvidence`, receipt의 adapter·identity·snapshot ref·dispatch count·terminal outcome 확인은 `BrowserManager::start`와 같다. 이 경계 안에서만 `BrowserProcesses::start_on`을 호출한다. mode를 bypass로 바꾸거나 allow receipt를 합성하지 않는다. 거절은 `browser_unavailable`와 고정 권한 사유로 기록하고, child spawn·classifier·승인 질문·provider 호출은 0이다.
    - 감시용 proxy에는 `web_watch_profile::web_watch_guard()`의 production `NetworkGuard::default()`를 준다. `ConfigBrowserStartup::network_guard`의 사용자 whitelist를 재사용하지 않는다. guard는 generation 수명 동안 고정하고, 대화 generation의 guard는 그대로다.
    - `browser_proxy.rs::guarded_connect`의 한 번 DNS 해석 → 결과 주소 전부 검사 → 검사한 주소에만 연결을 유지한다. redirect·하위 자원·다시 연결에도 같은 proxy를 쓴다. `--proxy-bypass-list=<-loopback>`·QUIC 및 비 proxy UDP 제한·managed policy를 그대로 소비한다. URL userinfo와 비 HTTP(S)는 042의 `validate_watch_url`에서 먼저 거절한다.
    - 같은 process에 열린 generation이 있거나 시작·정리가 진행 중이면 감시가 기다리거나 닫지 않고 `browser_busy`다. 아직 살아 있는 대화의 idle/reply-wait 만료를 감시가 대신 처리해 자리를 빼앗지 않는다. maintenance가 닫은 뒤의 다음 정규 확인에서만 시작한다.
    - **프로필·cache 저장과 보호:** 비로그인 감시는 049처럼 `<generation root>/profile`에서 익명 cookie를 생성·저장·전송하고 방문 기록·사이트 자료를 가질 수 있다. HTTP cache도 generation 임시 폴더 안에 저장된다. 비로그인 `BrowserEnvironment::chromium`은 cache 경로를 별도 지정하지 않는 기존 인자를 유지한다. PRD 010의 저장 프로필 감시는 050 결정 (라)대로 `chromium_on`이 저장 프로필 밖 `<generation root>/cache`를 지정한다. HTTP cache를 끄지 않는다. 두 mode 모두 정상 종료 때 임시 프로필/cache를 generation과 함께 지우지만 강제 종료 뒤에는 남을 수 있다. 임시 프로필과 임시 cache는 0700 generation 폴더 안에 있으나 050의 비밀 파일 보호 대상은 아니다. 그 보호는 `<data>/browser/profile/` 전체에만 적용된다. 이 예외는 브라우저 자체의 저장에만 적용하고 관측 fact·알림·조회·진단에는 cookie·원문을 추가하지 않는다.
4. **그려진 글의 대기와 한도:** `CdpDocument`에 감시 전용 읽기를 더한다. 대화용 `read_transaction`의 AX·요소 snapshot·캡처를 호출하지 않는다. 열기는 `Page.navigate`이고 그 뒤 관찰 명령만 쓴다.
    - 전체 한도는 자리 획득 뒤 시작부터 결과를 받을 때까지 **60초**다. 브라우저 core 시작은 기존 `startupTimeoutSeconds`와 남은 전체 예산 중 작은 값, viewer 시작도 기존 viewer 예산과 남은 전체 예산 중 작은 값이다. navigation·load·idle·본문 추출을 합친 한도는 **20초**와 남은 전체 예산 중 작은 값이다. 성공·실패를 정한 뒤 process·proxy·임시 폴더 정리는 항상 수행한다. 정리는 이 읽기 예산을 넘더라도 자원을 남긴 채 반환하지 않는다.
    - navigation 전에 `Network.enable`과 loader에 결속된 Page lifecycle event를 켠다. request id·frame id·loader id·target session으로 주 문서 응답과 하위 요청을 구분한다. redirect 때 마지막 주 문서 응답만 채택한다. 전 loader의 load나 subframe의 load로 완료를 선언하지 않는다.
    - 현재 주 문서 load 후 진행 중 HTTP(S) 요청이 0인 시간이 500 ms 이어져야 한다. `requestWillBeSent`에서 더하고 `loadingFinished`·`loadingFailed`에서 뺀다. cache 응답도 event에 따라 정리한다. redirect의 request id 재사용을 중복으로 세지 않는다. 이 idle 판정은 최초 page target의 DevTools session이 보고한 요청(그 session의 하위 frame 요청 포함)을 대상으로 한다. 종료되지 않는 HTTP 요청은 한도까지 기다린 뒤 실패한다. 별도 target인 cross-process iframe·service worker의 모든 내부 작업이나 WebSocket 지속 연결까지 완료됐다고 주장하지 않는다.
    - 최종 주 문서가 429 또는 오류 HTTP 상태면 그 응답으로 실패를 먼저 분류한다. 오류 페이지의 script가 계속 요청해도 `rate_limited`·`http_error`를 timeout으로 덮지 않는다. 주 문서의 429·503 `Retry-After`만 `web_watch_fetch.rs::retry_after_ms`와 기존 backoff에 전달한다. 하위 자원의 429나 앞 redirect의 header는 쓰지 않는다.
    - 글은 `document.body.innerText`에서 읽는다. 1 MiB의 UTF-8 byte 한도로 DevTools 응답 자체를 제한하고 code point 중간을 자르지 않는다. 경계의 expression은 고정이며 페이지가 제공한 code를 실행하지 않는다. 잘림은 별도 bool로 반환한다. body가 없으면 빈 글이다. 전체 글을 먼저 Rust로 전송한 뒤 자르는 구현은 한도를 지킨 것이 아니다.
    - 받은 글에 `normalize_fragment`만 적용한다. HTML 태그 제거·entity 재해석을 하지 않는다. 정규화 뒤 기존 `extract`·`extract_shown`·숫자/문구 조건을 쓴다. 고정 문구·occurrence·match count가 맞지 않을 때 다른 위치를 찾지 않는다.
    - 잘린 글에서 완전한 앞뒤 고정 문구가 모두 있는 값은 042처럼 읽을 수 있지만 `WebResponseFacts.truncated = true`를 남긴다. 고정 문구나 값이 잘려 없어졌으면 성공하지 않는다. `text_digest`는 실제 추출에 쓴 제한된 정규화 글의 digest다.
    - 추출 직전·직후 `Page.getFrameTree`의 `BrowserDocument`(target·session·frame·loader·URL)와 `CdpReply.document_epoch`·sequence를 확인한다. URL과 loader가 같은 HTTP(S) 문서이며 response 기록도 그 loader의 것이어야 한다. 변경·비 HTTP(S)·응답 결속 실패면 글을 버리고 탭을 닫는다. 이전 문서의 상태·URL·글을 조합하거나 상태 200을 합성하지 않는다.
5. **새 탭과 입력 금지:** 감시 generation은 처음 만든 page target만 읽는다. `Target.attachedToTarget`의 다른 page는 즉시 `Target.closeTarget`으로 닫는다.
    - `PausedTargetAdmission`에도 같은 정책을 전달해 응답 대기 중 붙은 popup을 resume하거나 현재 탭으로 바꾸기 전에 닫는다. 기존 admission의 "설정 후 resume" 표식을 단순히 재사용해 닫힌 popup을 `apply_event`가 선택하게 하지 않는다. 최초 탭 설정과 새 page 거절을 구분한다. 대화 generation은 기존 popup 이동을 유지한다.
    - 누르기·입력·키·스크롤·붙여넣기·`Page.captureScreenshot`·번호표 DOM 변경은 없다. 다운로드 deny, file chooser 감지 시 닫기, 허용 scheme 확인은 049 경계를 그대로 쓴다. 페이지 자체의 JavaScript 실행은 허용한다.
6. **공유와 정리:** 감시 점유는 manager의 slot과 같은 원자적 획득 경계에서 정한다. 대화의 `prepare`·`acquire`·`capture_prepared`·`close_prepared`는 감시 generation에 접근하지 못하고 `browser_busy`를 받는다. 사용자 `/browser takeover`와 `handoff`의 host 진입도 감시 점유이면 즉시 busy다. slot 획득 전과 획득 후에 generation 번호를 확인해 check-then-lock 틈을 없앤다.
    - 대화의 `assistant`·답 기다림·`user` takeover 어느 상태에서든 감시는 busy로 분류한다. 050의 `BrowserError::UserControl`을 감시 기록에는 `browser_busy`로 변환한다. takeover가 먼저 자리를 예약한 때도 감시가 끼어들지 않는다. 기존 세션의 미전달 닫힘 알림을 감시가 소비하지 않는다.
    - 감시가 차지한 동안 보기 화면은 050의 `assistant` 설정(`-viewonly`, `ViewerRoutes::View`, paste listener 없음)이다. 감시 완료 뒤 takeover 예약을 자동 실행하지 않는다. 사용자는 다시 요청한다.
    - 일반 `browser`의 `close`와 사용자 명령 `/browser close`는 구분한다. 전자는 busy이고, 후자는 050의 전역 사용자 닫기를 유지해 감시 generation의 닫기를 요청할 수 있다. `browser forget`·shutdown도 닫는다. 닫기로 중단된 확인은 성공이나 로그인 실패로 꾸미지 않고 `browser_unavailable`다. durable 작업 자체가 취소·변경됐으면 042의 취소/오래된 lineage 경로가 우선한다.
    - 임시 감시도 `browser_lock::acquire(ProfileUse::Temporary)`의 `active.lock` 공유 잠금·`forget.lock` 확인·`browser/` descriptor 공유 flock을 모두 가진다. 마지막 directory lock 뒤 identity 재확인 전에는 child가 없다. `BrowserLockHold`는 process와 generation 임시 폴더 정리가 끝난 뒤 놓는다. 저장 프로필을 열거나 만들지 않는다.
    - **maintenance를 멈추지 않는다.** `LockWatch`·`WatchedLocks::check`의 1초 점검은 slot 밖에서 계속 돈다. 감시 전용 generation 취소 신호를 `BrowserCancellation`/DevTools 대기에 연결해 forget·경로 교체·사용자 close가 기다리는 요청도 중단시킨다. 기존 manager 전체의 shutdown 취소 bool을 재사용하면 다음 generation도 취소되므로 generation마다 새 신호를 쓴다. 시작 중 받은 닫기는 다음 시작 경계와 child 실행 전후에서 확인한다.
    - slot을 쥔 코드가 `maintenance` 전체를 재귀 호출하지 않는다. maintenance는 신호만 전달하고, slot 소유자가 단일 `end` 경로로 정리한다. `Deferred.live` 번호와 감시 점유 번호를 함께 retire하여 지난 close가 다음 generation을 닫지 못하게 한다. 정리 중에는 새 시작·takeover를 허용하지 않는다. 실패·시작 실패·취소·정상 종료 모두 같은 수명 규칙이며 감시 때문에 대화 세션용 닫힘 표식을 남기지 않는다.
7. **실패·기록·알림:** `WebCheckFailure`에 `BrowserUnavailable`, `BrowserBusy`, `RenderedContentUnavailable`을 더한다. `web_watch_notify_render.rs::failure_text` 등 exhaustive match를 함께 갱신한다.

   | 상황 | 관측 결과 |
   |---|---|
   | 꺼짐, 공식 container/Chromium 부재, 시작 gate 거절, 시작 실패, 강제 닫힘 | `browser_unavailable` |
   | 대화·takeover·다른 확인이 자리 사용/시작/정리 중 | `browser_busy` |
   | 성공 HTTP 문서의 고정 문구를 찾지 못함 | `rendered_content_unavailable` |
   | load/idle/본문 읽기 한도, transport 실패, 문서 변경/응답 결속 실패 | 기존 `network_error`; 성공 값 없음 |
   | URL/주소 guard 거절을 확인한 경우 | 기존 `guard_blocked` |
   | 주 문서 429 / 다른 오류 HTTP | 기존 `rate_limited` / `http_error` |
   | 후보 개수·숫자 형식 불일치 | 기존 `ambiguous_value` / `format_mismatch` |

    - `browser_unavailable`의 이유는 고정 reason code(예: `permission_denied`, `disabled`, `startup_failed`, `closed`)로 남긴다. `WebCheckResult::Failed`와 미리보기/확인의 `WebWatchError::Unreadable`에 optional `browser_reason`을 더해 reader의 enum 값만 전달한다. 저장 자료에서 누락은 사유 없음이고, 기존 fetch 실패에는 추가하지 않는다. 임의 오류 문자열·페이지 글·header·원문 URL query를 넣지 않는다. SOCKS 거절과 일반 transport 실패를 구분할 증거가 없으면 `network_error`이며 원인을 추정하지 않는다.
    - timeout 때 화면에 옛 글과 고정 문구가 있어도 추출하지 않는다. 확인된 응답이 없으면 `response: None`이다. HTTP 응답은 확인됐지만 글을 버린 실패에서는 버린 글의 digest를 만들지 않는다. 현재 `WebResponseFacts.text_digest: String`은 이런 실패에 한해 빈 문자열로 "읽은 글 없음"을 표현하고 값과 baseline은 남기지 않는다.
    - `build_record`의 `text_version`은 계획의 추출 버전에서 온다. `web_watch_store.rs::WebWatchLedger`·`web_watch_decision.rs::next_delay`·실패 구간 계산은 그대로 소비한다. busy도 실패 구간을 잇고 다음 cadence를 예약한다. 즉시 재시도하지 않는다. 실패는 baseline을 갱신하지 않는다.
    - `WebNotifyIntent`에 읽기 방식을 저장하고 변화·실패·회복·공백·마감 intent 생성 경로 모두에서 채운다. renderer가 현재 config를 다시 읽지 않는다. fetch 한계 글은 기존 글 그대로이고 browser는 `rendered_text_v1`, 로그인 프로필 미사용, 읽기 전용, 대기/잘림 한계, 다른 곳 검증 없음, remote 수신 ACK 아님을 적는다. 로그인 설명은 PRD 010이 더한다.
8. **표면과 문서:** `read_web_watch`·`plan_view`·CLI/API JSON·대화 상태 조회에 읽기 방식을 보인다. `mask_web_watch_metadata`, `mask_web_watch_session`, `safe_extraction`의 URL query/fragment 가리기를 유지한다. `docs/USAGE.md`와 README에 CLI/API 등록, 실패 종류, 브라우저 공유, 제한된 완료 대기, 비로그인 감시는 저장 프로필을 쓰지 않는다는 것을 적는다. 이 PRD 구현 때 문서를 갱신하며, PRD 작성만으로 현재 기능인 것처럼 사용 문서를 바꾸지 않는다.

## Non Scope

- 저장 로그인 프로필을 쓰는 감시, 대화형 자동 제안·로그인 선택·다시 확인(PRD 010).
- 클릭·입력·스크롤, 사이트별 추출기, 접근 제한 우회, provider를 통한 값 추정.
- 새 queue·scheduler, 모든 외부 자원의 렌더링 완료 증명, 화면 캡처.
- 기존 대화 브라우저의 popup 이동·takeover 회차·준비/실행 판정 변경.

## Parent Requirement Mapping

| SPEC 051 기준 | 이 PRD의 계약 | 수용 조건 |
|---|---|---|
| 소유 범위 1 중 읽기 방식·token·digest·기준 재설정 | Scope 1·2 | 1·2·8 |
| 소유 범위 2, 수용 조건 1의 비로그인 읽기 전체 | Scope 4·7 | 2·3·4·8 |
| 소유 범위 3의 비로그인 실행, 수용 조건 2의 비로그인 전체 | Scope 2–5 | 3·5·6·9 |
| 소유 범위 4, 수용 조건 3 전체 | Scope 6·7 | 7·8·9 |
| 소유 범위 5·7 중 비로그인 실패·알림 한계·조회 | Scope 7·8 | 8·10 |
| 수용 조건 6 | 구현 후 Cargo 검증·문서 | 11 |

SPEC 수용 조건 1·2의 로그인 몫과 4·5는 PRD 010이다. 000의 종료에 010의 대화형 요약이나 Docker 실행을 요구하지 않는다.

## Required Contract

1. 읽기 방식은 계획·token·digest·revision에 결속된다. 기존 fetch 자료와 예약의 뜻은 바뀌지 않는다.
2. watcher는 provider·agent turn·`browser` tool 없이 실제 process gate를 지나 한 번 열고 읽고 닫는다.
3. 공개 주소 guard와 임시 프로필은 감시 generation의 선택이다. 대화 config나 열린 브라우저를 빌리지 않는다.
4. load·idle·한도·주 문서 전후 확인을 만족한 글만 추출한다. HTTP 상태와 `Retry-After`는 그 문서에 묶인다.
5. 감시는 입력·캡처를 하지 않고 새 탭을 닫는다. 관측 원문은 추출 뒤 버린다.
6. 대화와 감시는 같은 한 자리에서 서로 busy다. 감시 중 takeover는 기다렸다가 성공하지 않는다.
7. forget·경로 교체·닫기가 진행 중인 읽기를 끝낼 수 있고, process·임시 폴더 정리 전에 잠금과 점유를 놓지 않는다.
8. 029·036·042의 lease·lineage·취소·실패 구간·알림 중복 억제와 전달 상태는 유지한다.

## Acceptance Criteria

아래는 구현할 테스트 이름과 위치다. 이미 존재하거나 통과했다는 기록이 아니다. 새 파일은 각 package의 기존 `tests/it/main.rs`에 일반 `mod`로 등록한다. 별도 `tests/*.rs` 실행 파일은 만들지 않는다. 기존 042·049·050 fixture를 확장하고, 같은 경로를 확인하는 시험을 중복 추가하지 않는다.

1. `crates/shacs-core/tests/it/spec051_rendered_observation.rs`의 `reading_mode_is_bound_to_preview_plan_and_wake`: 실제 preview→confirm→durable wake를 거쳐 browser token을 fetch로 바꾼 확인은 거절되고, 방식 변경은 revision·digest·baseline을 바꾸며 지난 wake는 실행·통지되지 않는지 본다. 같은 파일의 `legacy_fetch_records_keep_their_digest_and_schedule`: 기존 field 없는 저장 fixture와 token·intent를 읽어 기존 digest·예정 시각·한계 글을 유지한다. 단순 serde 왕복 대신 예약 실행까지 확인한다.
2. 같은 파일의 `rendered_value_drives_existing_condition_decisions`: 가짜 reader의 그려진 글로 기준 12,900 → A 12,900 → 무관 변화 12,500 → B 10,900, 조건 11,000 이하를 실행한다. 순서대로 `suppress`, `suppress`, `notify_candidate`, `rendered_text_v1`, 마지막 성공과 다음 wake가 맞는지 본다. NFC·연속 공백 및 화면에 실제 보이는 `<`·`&amp;`가 HTML로 다시 해석되지 않는 경우, 문구 조건도 같은 시험의 필요한 사례로 넣는다.
3. `crates/shacs-cli/tests/it/spec051_rendered_browser.rs`의 `rendered_read_waits_for_its_loader_and_keeps_http_facts`: 실제 manager·가짜 DevTools/실행기·시험용 시계로 load 뒤 늦은 요청이 끝나야 500 ms를 세고, 다른 loader의 load/429를 쓰지 않는지 본다. 주 문서 429·503의 status와 `Retry-After`가 reader 결과에 남는다. core 파일의 `rendered_http_failures_keep_retry_after_and_baseline`은 이 결과를 기존 dispatcher에 넣어 backoff·실패 구간·baseline 보존을 검증한다.
4. 같은 CLI 파일의 `rendered_read_discards_changed_or_unfinished_documents`: loader 변경, 비 HTTP(S), 응답 loader 불일치, 끝나지 않는 요청을 각각 주입한다. 옛 고정 문구가 있어도 값이 없고 탭·generation이 정리되는지 본다. `rendered_text_limit_marks_truncation_without_splitting_utf8`: 1 MiB 경계의 한글과 고정 문구를 사용해 응답 byte 상한·잘림 표식·완전한 구간만 추출함을 본다. core 파일의 `missing_rendered_anchors_are_not_a_new_baseline`은 고정 문구 부재·잘린 구간·모호한 값·형식 불일치가 각 실패와 `unknown`을 만들고 기준을 보존하는지 본다.
5. 같은 CLI 파일의 `watch_start_uses_real_process_gate_without_provider`: 실제 `browser_start_input`·`ProcessGate`와 가짜 실행기를 사용한다. user-local enable·유효 containment가 있는 허용 사례는 한 번 시작하고, enable 부재/꺼짐·static deny·무효 containment는 child spawn 0, provider/classifier 호출 0, `browser_unavailable`와 권한 이유다. 단순히 가짜 gate가 오류를 돌려주는 것으로 대체하지 않는다.
6. 같은 CLI 파일의 `watch_uses_public_proxy_temporary_profile_and_closes_popups`: `persistLogin=true`, 사용자 whitelist에 사설 주소가 있는 config에서도 비로그인 감시의 프로필이 임시이고 저장 폴더는 그대로인지 본다. 실제 `guarded_connect`와 가짜 resolver/connector로 사설·혼합 DNS 결과의 connect 수 0, 공개 주소만 연결됨을 본다. 응답 대기 중 paused popup과 비동기 popup 모두 닫히고 최초 target만 읽으며, 기록된 DevTools 명령에 입력·스크롤·캡처가 없는지 본다.
7. 같은 CLI 파일의 `watch_and_conversation_share_one_slot_without_takeover_waiting`: 대화 assistant·reply wait·user takeover·시작 중 각각에서 watch가 즉시 busy이고 기존 문서/회차를 바꾸지 않는다. 반대로 watch를 DevTools 응답 경계에 멈추고 다른 thread의 `browser`와 takeover를 호출해 watch를 놓기 전에 busy가 돌아오는지 본다. 먼저 온 대화 takeover 예약도 watch가 빼앗지 못한다. 보기 설정은 assistant이고 paste가 열리지 않는다.
8. core 파일의 `busy_window_notifies_once_then_recovers`: 시험용 시계로 busy가 기준 시간을 넘기면 실패 통지 intent 하나, 자리 해제 뒤 성공에서 회복 intent 하나, 실패 중 변화 intent 0을 확인한다. 중복 wake·재시작 replay·방식 변경 경합에서 중복과 stale 결과가 없는지 기존 042 fixture를 확장한다. `notifications_keep_the_confirmed_reading_limits`: 변화·실패·회복·공백·마감 모두 저장한 intent의 방식으로 렌더링하며 현재 config 변경으로 설명이 바뀌지 않는다.
9. 같은 CLI 파일의 `forget_and_lock_maintenance_interrupt_an_active_watch`: 실제 임시 data 폴더 잠금으로 읽기/시작을 멈춘 사이 forget·active lock 교체·사용자 close를 각각 넣는다. 1초 점검이 slot을 기다리지 않고 신호를 보내며, 중단 후 child와 임시 폴더가 없어져야 active 및 browser directory flock이 풀리는지 본다. 정리 중 새 시작은 거절되고 지난 신호는 다음 generation을 닫지 않는다. `gateway_worker_uses_the_production_rendered_reader`: 실제 `from_bundle`의 production tooling 구성과 gateway worker 생성 경로를 통해 due browser wake를 실행한다. process 실행기와 DevTools만 시험용으로 주입하고 reader를 worker에 직접 덧꽂아 배선 결함을 숨기지 않는다. 대화 host와 점유 공유, 같은 lease owner, terminal fact와 다음 wake, provider 호출 0을 관찰한다. 시험은 기존 `tests/it`에서 호출하며 worker 구현을 복제하지 않는다.
10. `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs`의 `browser_preview_confirmation_and_status_use_the_selected_reader`와 `crates/shacs-api/tests/it/spec051_web_watch_api.rs`의 `browser_preview_confirmation_and_status_preserve_mode`: 실제 인자/body 파싱→preview→확인→status에서 방식과 기준·오류가 맞고 원문·query·cookie·화면이 출력되지 않는지 본다. 000을 대화형 PRD 없이 닫는 증거다.
11. 구현 후 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` → `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings` → `cargo test --manifest-path crates/Cargo.toml -p shacs-core -p shacs-cli -p shacs-api --locked -q` 순서로 확인한다. 변경 package가 더 있으면 실제 변경 package를 추가한다. 관련 시험만 반복할 때는 `--test it -q spec051_`로 좁힌다. 전체 workspace 테스트는 사용자 승인 뒤에만 돌린다. 새 의존성·`Cargo.lock` 변경이 없고 Scope 8의 사용 문서가 맞는지 확인한다.

## Docker Check

SPEC 수용 조건 5는 전부 PRD 010의 `scripts/spec051-web-watch-check.sh --approved`가 소유한다. 000은 실제 Docker 실행을 기다리지 않고 위 Cargo의 가짜 DevTools·실행기·시계와 실제 worker 연결로 닫힌다. 049·050의 닫힌 결과를 소비한다는 것이 051의 실제 Chromium 렌더링을 확인했다는 뜻은 아니다. 그 미검증 범위는 Closure Evidence에 남긴다.

## Closure Evidence

2026-11-12 구현 및 검증 기록은 [IMPLEMENTATION-000.md](../IMPLEMENTATION-000.md)에 있다.
Required Contract 8개와 Acceptance Criteria 11개를 코드·시험 위치에 대응했고,
15종류의 보호 장치 비활성화 실패 확인, 원자 점유 경합 재현·수정, 반복 횟수,
Cargo 결과와 미검증 범위를 기록했다. 기존 시험 내부의 자식 Cargo는 허용되고
별도의 Cargo 명령을 동시에 시작하지 않는다는 범위를 확인했다. 보류했던
`spec033_release_runner`를 단독 전경 명령과 `--test-threads=1`로 재실행하여
15개 통과, 기존 수동 fixture 1개 ignored, 종료 코드 0을 확인했다.
이 PRD의 구현 검증은 끝났으며 독립 검토를 기다린다. 실제 Docker/Chromium
렌더링 시험은 PRD 010 범위로 남는다.

1. CLI/API preview·확인·status와 저장 계획의 mode·revision·digest, 기존 fetch 예약 보존 결과.
2. 실제 manager의 gate receipt·child 명령·DevTools 명령과 load/idle/문서 변경/잘림 시험 결과.
3. 실제 gateway 구성으로 실행한 due wake의 fact·다음 wake·provider 호출 0, 양방향 busy와 maintenance/forget의 종료 순서.
4. 실패·회복·변화 및 다른 종류의 알림 intent와 확인 한계 글, 오래된 결과 차단 증거.
5. Cargo 검증 기록, 사용 문서, lockfile 무변경. 실제 Docker 렌더링은 이 PRD에서 미검증이며 010 소유라고 적는다.

## 남은 제한

- 500 ms network idle은 최초 page target의 session이 보고한 요청에 대한 기준이며 사이트가 최종 값을 표시했다는 증명이 아니다. 늦은 timer, WebSocket, cross-process iframe·별도 worker의 이후 변화는 놓칠 수 있다. 끝나지 않는 HTTP 요청은 20초 안에 완료되지 않아 실패한다.
- 같은 worker thread가 관측·자동화·surface 승인을 차례로 처리한다. 브라우저 읽기 동안 그 뒤의 일은 늦어질 수 있다. 외부 inbound loop를 막지 않는 050 구조와 기존 세션 lock 대기는 유지한다.
- 감시마다 시작·정리 비용이 들고, 대화의 reply wait·takeover가 길면 busy 실패가 이어진다.
- 임시 프로필은 그 확인 동안 익명 cookie를 생성·저장·전송하고 사이트 자료를 저장할 수 있다. HTTP cache도 응답 내용을 generation 임시 폴더에 저장한다. 둘 다 042 무저장 계약의 예외이고 050의 저장 프로필 비밀 파일 보호 밖이다. 정상 종료 때 임시 폴더와 함께 없어지지만 강제 종료 때 남을 수 있다. "로그인 프로필을 쓰지 않음"이지 cookie나 cache 저장의 금지가 아니다.
- proxy는 페이지의 모든 부작용을 막는 장치가 아니다. 읽기 위해 연 페이지의 JavaScript가 공개 주소로 요청할 수 있다. 입력 자동화와 접근 제한 우회는 하지 않는다.
- 본문 메모리·DevTools 응답은 제한하지만 Chromium 내부 DOM 전체 크기는 제한하지 않는다. 강제 process 종료 뒤 남는 임시 폴더는 049·050의 기존 제한이다.
