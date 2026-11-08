# PRD 030. 브라우저 판정 연결

Status: Planned

Depends on: PRD [000](000-tool-result-images.md)·[010](010-container-browser-and-boundary.md)·[020](020-browser-tool.md)의 닫힌 결과와 048 PRD 000–050의 닫힌 판정·비밀 값 가리기·도구 결과 검사. PRD 040과 050·051의 구현에 의존하지 않는다.

## Goal

브라우저에서 보는 일과 페이지를 바꾸는 일을 동작마다 구별한다. classifier와 승인 질문은 요소 번호 대신 사이트·버튼·입력·양식의 내용을 보고 판정한다. 020의 임시 bypass 전용 제한을 제거하고 부모 스펙의 모든 mode 판정을 완성한다.

## 지금 상태

- `SafetyCapability`는 `FsRead`부터 `SelfModification`까지 열 항목이며 `SafetyCapability::parse`, `SafetyCapability::all`도 그 집합을 쓴다(`crates/shacs-config/src/permissions.rs`). `browser_act`는 없다.
- `normalize_tool_candidate`는 `tool_capabilities(registry, tool_name)`을 부르고 인자만으로 target을 만든다. `infer_capabilities`는 tool 이름과 process adapter만 본다(`crates/shacs-core/src/runtime/permission_action.rs`). 현재 페이지의 요소 설명은 인자에 없으므로 번호만으로는 판정할 수 없다.
- `decide_plan`은 `FsRead`만 허용하고 나머지를 거절한다. default·accept_edits는 읽기·쓰기 이외를 `ask_or_deny`로 보낸다(`crates/shacs-core/src/runtime/permission_policy.rs`의 `decide_plan`, `decide_default`, `decide_accept_edits`). 따라서 plan의 browser 읽기 허용과 act 질문은 browser에 한정한 명시적 추가 처리가 필요하다.
- `parse_specifier`는 `web_fetch`에만 `domain:`을 받으며 known tool 목록에 browser가 없다(`crates/shacs-core/src/runtime/auto_mode/permission_rule.rs`). `blocking_match`의 host 분기도 `web_fetch`만 본다(`crates/shacs-core/src/runtime/auto_mode/rule_match.rs`). parser만 넓혀서는 browser 규칙이 적용되지 않는다.
- `AutoModeSlots::resolve`가 쓰는 `DEFAULT_SOFT_DENY`는 일곱 항목이다(`crates/shacs-core/src/runtime/auto_mode/classifier_slots.rs`). 구매 등 계정 동작 항목은 있지만 browser 확정 전용 항목은 없다.
- `normalize_tool_candidate`는 `action_secrets::argument_views`로 판정용 값과 노출용 값을 구별한다(`crates/shacs-core/src/runtime/permission_action.rs`). `result_check::checked_text`는 content-part 배열이면 text만 고른다(`crates/shacs-core/src/runtime/auto_mode/result_check.rs`). PRD 000의 `tool_images::persist`는 그 검사 전에 그림을 파일 참조 글로 바꾼다(`crates/shacs-core/src/runtime/tool_images.rs`).

## Scope

1. **새 capability:** `SafetyCapability::BrowserAct`를 더하고 serde·label·parse에 `browser_act`, `all()`의 집합에 열한 번째 항목을 넣는다. core의 capability label/검증과 classifier eligibility의 exhaustive match, diagnostics 직렬화, config schema·허용값 목록과 상속 ceiling 비교를 모두 같은 값으로 맞춘다. `browser_act`는 `net_outbound`, `proc_exec`, `external_delivery`와 별도다. 부모 ceiling에 이 값이 없으면 browser act를 허락하지 않으며, 다른 capability가 대신 만족시키지 않는다. `runtime_capability_ceiling_from_registry`(`crates/shacs-cli/src/lib.rs`)의 020 browser 분기를 확장해 새 production ceiling에는 `net_outbound`와 `browser_act`를 모두 광고한다. 이 작업은 030 소유다. 새 프로필에서 모든 capability를 뜻하는 집합은 `all()`을 소비하게 하고 기존 저장 ceiling에 없던 값을 자동으로 보태지 않는다.
2. **동작별 정규화:** 직접 호출·deferred bridge·승인 뒤 실행이 공용 정규화를 쓰도록 동작 인자를 읽는다. `open`, `look`, `scroll`, `back`, `wait`, `close`는 `[NetOutbound]`, `click`, `type`, `press`는 `[BrowserAct]`다. tool은 `process_adapter_kind: None`을 유지한다. session·mode·대화형 여부는 020이 실행 경로에 채운 `BrowserCallContext`와 동일한 run snapshot을 사용하고 registry나 model 인자에서 새로 추정하지 않는다. 잘못된 action은 capability 없는 정상 동작으로 바꾸지 않고 정규화 오류로 거절한다. 020의 bypass 전용 실행 guard와 전 동작 net 임시 분기를 이 PRD에서 제거한다. 010의 user-local enable 기반 시작 허락과 browser 전용 gate는 바꾸지 않는다.
3. **판정 설명과 고정된 실행 대상:** host가 가진 최신 검증 snapshot에서 판정용 browser action을 준비한다. 모델이 `origin`·역할·양식 정보를 인자로 자가 신고하게 하지 않는다. 준비는 페이지 변경·캡처·새 browser 시작 없이 수행한다. `open`은 요청 URL, 다른 동작은 현재 주 문서 origin을 사용한다. `click`·`type`은 해당 요소, `press`는 현재 focus 요소를 설명한다. focus나 form이 없으면 `none`이라고 명시한다.
    - 설명에는 action, origin, role, accessible name, type의 입력 text·submit 여부 또는 press key, 실제 연결된 form의 accessible name과 DOM에서 해석한 절대 submit URL·method를 넣는다. form owner는 DOM의 실제 소속(`form` 속성으로 외부에 연결된 경우 포함)으로 정한다. 제출에 쓰일 submitter의 node id와 `formaction`·`formmethod`·`formenctype`·`formtarget`·`formnovalidate` override를 기록하고 URL·method 등 최종 제출 속성에 반영한다. URL은 현재 document base URL로 해석한다. Enter의 제출 대상이나 submitter를 확인할 수 없으면 추정한 대상에 승인받지 않고 다시 관찰하라는 거절을 반환한다. 양식이 없으면 URL도 none이다. 번호는 참고로만 남기며 설명의 유일한 근거가 될 수 없다.
    - 원본과 노출본을 구별하고 모든 설명 문자열에 022 `redact_value`/`redact_string`과 048 `.env.json` 값 가리기를 적용한다. 이름·form·URL도 페이지가 준 비신뢰 자료다. URL userinfo와 비밀 query 값도 redaction 대상이다. 8자 이상 `.env.json` 값은 `[secret 이름]`이고 classifier·질문·diagnostics·로그에 원문이 없다. 048처럼 값 snapshot을 읽지 못하면 classifier는 판정 없음이다.
    - 원래 tool 인자·snapshot revision·target/frame/loader·origin·요소/form 설명·form owner·submitter node와 override·해석된 최종 제출 URL/method 등의 원본 digest를 action digest와 승인 correlation에 묶는다. `press`는 정확한 focused frame과 실제 focused node도 포함하고 shadow root와 자식 frame의 실제 focus까지 확인한다. 노출용 마스킹 때문에 다른 입력이 같은 승인으로 실행되지 않게 한다.
    - classifier 허용·승인 후 실제 Input event 직전에 DOM에서 대상의 존재·역할/이름, 실제 form owner, 제출 URL·method, submitter node와 모든 override를 다시 읽어 binding과 비교한다. `press`는 실제 focused frame/node도 다시 읽는다. 020 click의 올바른 frame hit-test도 유지한다. loader·URL·revision이 같더라도 form action, base URL, 소속 form, method, submitter override 또는 focus가 바뀌면 입력하지 않고 기존 binding을 무효로 한다. 결과는 다시 look하고 새 판정을 받으라는 안내이며 자동으로 새 대상에 승인이나 classifier 결과를 재사용하지 않는다.
    - 페이지가 아직 없을 때 open·close는 요소 없이 준비할 수 있다. 읽기 동작의 판정 준비가 내부 문서를 읽는 우회로가 되지 않게 010의 검증된 자료만 사용한다. snapshot이 없거나 다른 session 소유면 실행 가능하지 않다는 이유를 돌려주며 새 페이지를 읽어 설명을 채우지 않는다.
4. **022·048 판정 자리:** 상속 한도·인자 오류·사용자 deny/ask·승인 correlation 등 기존 선행 규칙을 그대로 적용한 뒤 다음 baseline을 쓴다. 사용자 allow와 기억된 승인도 048의 기존 자리·범위를 따르며 tool의 password/file/문서 검사를 풀지 못한다.

   | mode/실행 | `net_outbound` 동작 | `browser_act` 동작 |
   |---|---|---|
   | auto(대화형) | `web_fetch`와 같은 048 4단계 classifier | 048 4단계 classifier. 허용만 실행 |
   | default, accept_edits | 기존 `web_fetch` 판정 | 대화형은 승인 질문 |
   | plan | browser의 open/look/scroll/back/wait/close 허용. `web_fetch`는 기존 거절 유지 | 대화형은 승인 질문, 비대화형은 거절 |
   | dont_ask | 기존 `web_fetch` 판정 | 거절 |
   | 비대화형(bypass 외) | 기존 `web_fetch` 판정, plan의 browser 읽기는 위 예외 적용 | 거절 |
   | bypass_permissions(대화형·비대화형) | 기존 `web_fetch` 판정 | 허용. 기존 deny/ask·상속 한도는 적용 |

   대화형 auto classifier 막음·판정 없음은 실행하지 않고 048의 거절 누적·계속 진행·임계치 질문을 그대로 따른다. `browser_act`의 비대화형 non-bypass와 `dont_ask`는 allow·기억된 승인·classifier로 실행 가능해지지 않으며 질문이나 classifier 요청 없이 거절한다. bypass는 대화형·비대화형 모두 실행하되 기존 사용자 deny/ask와 상속 한도를 유지한다. 비대화형의 ask 규칙은 기존처럼 거절이다.

   `decide_plan`(`crates/shacs-core/src/runtime/permission_policy.rs`)의 예외는 정규화된 실제 tool이 `browser`이고 동작과 capability가 Scope 2에 맞을 때만 적용한다. browser의 여섯 net 동작은 대화형·비대화형 모두 baseline 허용, 세 act 동작은 대화형 질문·비대화형 거절이다. generic `NetOutbound` 전체를 plan에 허용하지 않는다. `web_fetch`·다른 tool·다른 capability와 기존 선행 규칙은 그대로다.
5. **classifier·승인 입력:** 공용 action 설명을 classifier pending action과 승인 질문에 함께 넣는다. classifier는 048 transcript 규칙(사용자 메시지·앞선 도구 호출만, 도구 결과·assistant 글 제외)과 16,000자 action 한도를 유지한다. 설명까지 잘리면 `truncated`다. 승인 질문은 origin·역할·이름·입력·form과 제출 대상을 보이고 022 선택지·correlation·가린 비밀이 있는 질문의 일회 승인 계약을 유지한다. 준비된 정보는 tool 결과가 아니며 페이지 지시문을 system 안내로 삽입하지 않는다.
6. **기본 soft deny:** `DEFAULT_SOFT_DENY` 끝에 다음 한 줄만 더한다. 사용자 override와 `$defaults` 의미는 그대로다.

   ```text
   - [Browser Commitment] In the browser, submitting or confirming a purchase, booking, payment, message, post, or account-setting change the user did not ask for.
   ```

   048의 직접적·구체적인 사용자 의도 또는 ALLOW 예외만 soft deny를 푼다. 클릭 호출의 모델 설명·tool 결과·승인 답을 새 사용자 메시지로 만들지 않는다. hard deny는 그대로 우선한다.
7. **사용자 규칙:** known tool에 `browser`를 넣고 `browser`, `browser(domain:<host pattern>)`만 더한다. action 이름 지정 같은 새 문법은 없다. `HostPattern`의 기존 대소문자·끝 점 제거·`*`·`*.example.com` 의미를 그대로 쓴다. open은 요청 URL의 host, 나머지는 현재 검증된 주 문서 origin의 host가 subject다. close에 현재 문서가 없으면 domain 규칙은 맞지 않고 bare browser는 맞는다. browser/web_fetch 사이에 domain 규칙이 새지 않도록 parser와 deny·ask·allow matcher 모두 selector를 확인한다. deny → ask → allow의 기존 우선순위와 비대화형 ask 거절을 유지한다. 승인 뒤 origin이 바뀌면 규칙을 적용했던 action은 실행하지 않는다.
8. **결과 검사:** 실제 실행한 browser 결과의 text만 기존 048 검사로 보낸다. 000 저장 뒤 URL·제목·요소·보이는 글·그림 참조 text가 대상이며, PNG/base64·provider 요청에만 붙인 그림 메시지는 요청에 없다. 걸리면 기존 경고를 덧붙이고 결과를 지우거나 새 승인을 요구하지 않는다. permission 거절·스키마 오류 등 실행하지 않은 호출은 검사하지 않는다. 검사 실패와 classifier 부재는 기존 diagnostics만 남긴다.
9. **문서:** `docs/USAGE.md`와 README 권한·브라우저 문단에서 020 임시 제한 설명을 최종 표로 교체한다. `browser_act`, domain 예, soft deny의 확정 동작과 명시 의도, 요소 이름의 불신·캡처 미검사를 적는다. 앞선 022·048 스펙과 닫힌 PRD 본문은 고치지 않는다.

## Non Scope

- browser 시작 gate와 proxy 판정, viewer, 다른 tool capability·mode 변경.
- classifier template 전체 변경, 사용자 규칙의 새 action 문법, tool 결과의 그림 검사.
- 050 handoff·takeover·로그인, 051 감시의 비대화형 읽기.

## Parent Requirement Mapping

- [049 SPEC](../SPEC.md) 소유 범위 5, 8 중 자기 문서, 수용 조건 4·8을 소유한다.
- 바꾸는 앞선 계약: SPEC "앞선 스펙에서 바꾸는 것"의 022 capability 행, 022 plan mode 행, 022 비대화형 bypass 행, 048 domain 문법 행, 048 기본 SOFT DENY 행 다섯 개다. 030 실행 종류 행과 030·022 시작 판정 행은 010 소유이며 여기서 바꾸지 않는다.
- 지금 계약은 022의 열 capability·plan의 fs_read 전용 baseline·비대화형 질문의 거절, 048 PRD 000 Scope 4의 web_fetch 전용 domain 및 Scope 13의 일곱 기본 soft deny다. 바뀐 뒤는 `browser_act` 추가, browser만의 plan 읽기 허용과 act 질문/거절, 비대화형 bypass act 실행, browser domain 지정, 여덟 번째 Browser Commitment다. 해당 확장은 부모 스펙에 이미 선언돼 있으며 앞선 문서는 완료 당시 기록으로 둔다.

## Required Contract

1. 동작의 capability는 하나이며 `browser_act`를 다른 capability로 대신 승인하지 않는다.
2. classifier·승인 질문은 host가 검증한 내용 설명을 받고, 승인과 실행은 같은 snapshot·인자·focus·form/submitter binding에 묶인다. 입력 직전 DOM 재확인에서 same-loader 변경도 거절한다.
3. 대화형 auto는 048 4단계를 따른다. plan은 browser 읽기를 허용하고 act는 대화형 질문·비대화형 거절이다. bypass act는 비대화형도 실행하며 그 밖의 비대화형 act와 dont_ask는 거절한다. 선행 사용자 규칙과 상속 한도를 건너뛰지 않는다.
4. browser domain은 해당 tool과 실제 판정 대상 host에만 맞는다.
5. soft deny 문장은 부모 표와 한 글자까지 같고, 결과 검사는 text만 본다.
6. `Cargo.lock`에 새 crate를 더하지 않는다.

## Acceptance Criteria

1. `crates/shacs-core/tests/it/spec049_browser_permissions.rs`의 `action_capabilities_and_mode_matrix`: 020 가짜 host·provider로 mode × 읽기/변경 × classifier 허용/막음/판정 없음 × 대화형 여부를 표로 시험한다. 기존 선행 deny/ask·승인·상속 ceiling도 같은 표의 필요한 경계만 보탠다. look/scroll은 net, click/type/press는 browser_act이며 proc_exec가 아니다. plan의 여섯 browser net 동작은 대화형·비대화형 모두 허용하고 같은 net capability인 web_fetch는 계속 거절한다. plan act는 대화형 질문·비대화형 거절이다. bypass act는 대화형·비대화형 모두 실행하고, 나머지 mode의 비대화형 act와 dont_ask는 classifier 호출·CDP 입력 없이 거절한다. 대화형 auto 막음은 CDP 입력 없이 기존 계속 진행 경로로 간다.
2. 같은 파일의 `action_description_is_bound_redacted_and_rechecked`: 실제 AgentLoop의 직접·deferred·승인 실행에서 classifier와 질문에 origin·role·name·입력·form/최종 submit URL·method·submitter override가 들어 있고 번호만 있지 않다. `.env.json` 값·022 비밀 key는 가려지고, 승인 뒤 origin/loader/요소가 바뀌면 CDP 입력이 없다. loader·URL·revision을 유지한 채 form action·method·소속 form·submitter의 formaction/formmethod를 바꾼 경우와 press 직전 focus만 바꾼 경우도 입력 없이 기존 binding을 무효로 하고 새 관찰·판정을 요구한다. 변경 없는 경우에는 입력이 실행되고 가린 값이 같은 서로 다른 원본 인자는 승인을 공유하지 않는다.
3. 같은 파일의 `browser_domain_rules_select_the_actual_origin`: exact/wildcard/끝 점/대소문자, open 대상과 현재 origin 차이, bare browser, 문서 없는 close, deny→ask→allow, browser/web_fetch 교차 오적용 방지를 최소 표로 확인한다. parser와 matcher를 함께 통과시킨다.
4. 같은 파일의 `commitment_rule_uses_only_explicit_user_intent`: 기본 slot의 정확한 한 줄과 `$defaults` 확장을 확인한다. 가짜 classifier가 구매 확정을 막은 뒤 실제 다음 사용자 메시지의 구체적 요청에서는 허용한다. tool 결과·assistant 글만 바꿔서는 classifier transcript에 사용자 의도가 생기지 않는다.
5. `crates/shacs-core/tests/it/spec048_auto_mode/result_checks.rs`의 기존 결과 검사 시험을 browser 결과로 확장한다. text만 검사되고 base64·붙인 이미지 메시지가 없으며, 거절된 browser는 검사하지 않고 경고/실패 처리가 기존과 같은지 확인한다.
6. `crates/shacs-config/tests/it/spec049_browser_config.rs`에 `browser_act` parse·공개 직렬화·all 집합과 ceiling의 허용/거절 경계 검사를 보탠다. 020의 `crates/shacs-cli/tests/it/spec049_browser_registry.rs` production adapter 시험에는 `runtime_capability_ceiling_from_registry`가 browser의 net_outbound·browser_act 둘 다 내고 실제 비대화형 bypass act가 ceiling을 통과하는 assert를 보탠다. 기존에 저장한 net-only ceiling은 act를 허용하지 않는다. 새 파일은 기존 `tests/it/main.rs`에 모듈로 등록하며 중복 getter 시험이나 새 최상위 test binary를 만들지 않는다.
7. fmt → workspace clippy `-D warnings` → 변경 package의 `cargo test --manifest-path crates/Cargo.toml -p <package> -q`를 `--locked`로 실행한다. 관련 기존 048 시험을 해당 package에서 함께 확인한다. 전체 workspace test는 사용자 승인 때만 한다. 문서와 `Cargo.lock` 무변경을 확인한다.

## Closure Evidence

1. plan browser 읽기 허용/web_fetch 거절·대화형 act 질문·비대화형 bypass act 실행을 포함한 mode 행렬, classifier·승인 질문의 가린 action 예, same-loader form·focus 변경의 입력 거절 증거.
2. parser/matcher·production ceiling·slot·결과 검사 시험 결과와 선행 계약 다섯 행의 구현 위치.
3. 020 임시 제한 제거 diff, 사용 문서와 Cargo 검증 기록.

PRD 030은 viewer 없이 닫힌다. browser 생성·화면 읽기는 010·020의 완료 계약을 그대로 소비한다.

## 남은 제한

- 요소 이름과 form 이름은 사이트가 정한다. 이름이 효과를 정직하게 설명한다는 보장은 없다.
- 캡처 안의 글은 결과 검사에 들어가지 않는다. 검사 경고가 실행 후 결과를 없애지는 않는다.
- auto에서는 변경 동작마다 classifier 지연이 생기고 default에서는 각 동작에 질문할 수 있다.
