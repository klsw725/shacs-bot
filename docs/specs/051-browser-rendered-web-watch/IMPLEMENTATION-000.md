# PRD 000 구현 검증 기록 — 2026-11-12

대상은 PRD 000의 비로그인 browser 관측이다. PRD 010의 저장 로그인 감시,
대화형 자동 전환, Docker 스크립트는 구현하지 않았다. 독립 검토는 별도다.

구현과 검증을 마쳤고 PRD 000 상태는 `Implemented (independent review pending)`이다.
전체 패키지 명령은 종료 코드 0이었다. 기존 시험 내부의 자식 Cargo는 허용되며,
직접 시작하는 별도의 Cargo 명령끼리 겹치지 않아야 한다는 범위를 lead가 확인했다.
보류했던 core release 시험도 다른 Cargo 없이 단독 전경 명령으로 재실행해 통과했다.

## Required Contract 대응

| 계약 | 구현 | 시험 |
|---|---|---|
| 1. 방식·token·digest·revision 결속과 fetch 호환 | `crates/shacs-core/src/runtime/web_watch.rs:16`, `crates/shacs-core/src/runtime/web_watch_surface.rs:192`, `:317`, `:479` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:102` `reading_mode_is_bound_to_preview_plan_and_wake`, `:196` `legacy_fetch_records_keep_their_digest_and_schedule` |
| 2. provider·agent turn·browser tool 없는 실제 gate와 worker 실행 | `crates/shacs-cli/src/spec035_cli_media/tool_registry.rs:55`, `crates/shacs-cli/src/lib.rs:19797`, `crates/shacs-cli/src/automation_worker.rs:58`, `crates/shacs-cli/src/browser/browser_manager.rs:1446` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:624`, `:1320` |
| 3. 감시 전용 공개 주소 guard와 임시 프로필 | `crates/shacs-cli/src/browser/browser_manager.rs:1446`, `crates/shacs-cli/src/browser/browser_process.rs:424` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:738`: 실제 generation SOCKS 연결, 실제 `guarded_connect`의 사설·혼합 DNS·공개 주소, 저장 프로필 보존 |
| 4. load·idle·시간/본문 한도·동일 문서·HTTP 결속 | `crates/shacs-cli/src/browser/rendered_load.rs:22`, `:99`, `:115`, `crates/shacs-cli/src/browser/cdp_document.rs:958`, `crates/shacs-core/src/runtime/rendered_web_reader.rs:51` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:676`, `:704`, `:725`; `crates/shacs-core/tests/it/spec051_rendered_observation.rs:313`; `crates/shacs-core/src/runtime/rendered_web_reader_tests.rs:6` |
| 5. 입력·캡처 금지, popup 거절, 원문 비저장 | `crates/shacs-cli/src/browser/cdp_document.rs:121`, `:931`, `crates/shacs-core/src/runtime/rendered_web_reader.rs:11`, `:77` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:738`; `crates/shacs-core/tests/it/spec051_rendered_observation.rs:240`; `crates/shacs-api/tests/it/spec051_web_watch_api.rs:71` |
| 6. 같은 자리의 양방향 busy, 즉시 takeover 거절 | `crates/shacs-cli/src/browser/browser_manager.rs:664`, `:677`, `:1659`, `:2172` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:876`, `:969`; `crates/shacs-cli/tests/it/spec050_browser_takeover.rs:1395` |
| 7. forget·교체·닫기·shutdown 취소와 정리 후 잠금/점유 해제 | `crates/shacs-cli/src/browser/browser_manager.rs:564`, `:855`, `:1198`, `crates/shacs-cli/src/browser/browser_process.rs:424`, `:786` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:1230`, `:1103`, `:969` |
| 8. 기존 lease·lineage·취소·실패 구간·알림 전달 유지 | `crates/shacs-core/src/runtime/web_watch_dispatch.rs:37`, `:157`, `:186`, `crates/shacs-core/src/runtime/web_watch_decision.rs:29`, `crates/shacs-core/src/runtime/web_watch_notify_events.rs:75`, `:133` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:415`, `:505`, `:102`; `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:1320` |

## Acceptance Criteria 대응

| 번호 | 코드와 시험 근거 |
|---|---|
| 1 | `web_watch_surface.rs:192/317/479`, `web_watch.rs:16/151`; core 시험 `reading_mode_is_bound_to_preview_plan_and_wake`, `legacy_fetch_records_keep_their_digest_and_schedule` |
| 2 | `rendered_web_reader.rs:77`, `web_watch_decision.rs:29`; core 시험 `rendered_value_drives_existing_condition_decisions`: 12,900 → 12,900 → 12,500 → 10,900, NFC·공백·`<`·`&amp;`, 문구 조건 |
| 3 | `rendered_load.rs:22/99/115`, `cdp_document.rs:958`, `web_watch_fetch.rs`의 `check_plan`; CLI 시험 `rendered_read_waits_for_its_loader_and_keeps_http_facts`, core 시험 `rendered_http_failures_keep_retry_after_and_baseline` |
| 4 | `cdp_document.rs:958`, `rendered_web_reader.rs:51`; CLI 시험 `rendered_read_discards_changed_or_unfinished_documents`, `rendered_text_limit_marks_truncation_without_splitting_utf8`; core 시험 `missing_rendered_anchors_are_not_a_new_baseline`, 실제 Boa 식 실행 시험 |
| 5 | `browser_manager.rs`의 `start` → 기존 `browser_start_input` → `ProcessGate::evaluate_and_maybe_spawn`; CLI 시험 `watch_start_uses_real_process_gate_without_provider`: 허용, enable 없음/false, static deny, containment proof 없음 |
| 6 | `browser_manager.rs`의 `start`, `cdp_document.rs:121/393`; CLI 시험 `watch_uses_public_proxy_temporary_profile_and_closes_popups` |
| 7 | `browser_manager.rs:664/677/1659/2172`; CLI 시험 `watch_and_conversation_share_one_slot_without_takeover_waiting`, `watch_start_cleanup_panic_and_takeover_reservation_keep_one_occupancy`, 기존 takeover 시험의 watch 거절 확인 |
| 8 | `web_watch_dispatch.rs:37/186`, `web_watch_notify_events.rs:75/133`, `web_watch_notify_render.rs:115`; core 시험 `busy_window_notifies_once_then_recovers`, `notifications_keep_the_confirmed_reading_limits` |
| 9 | `browser_manager.rs:564/855/1198/2172`, `automation_worker.rs:58`, production registry/adapter; CLI 시험 `forget_and_lock_maintenance_interrupt_an_active_watch`, `forget_waits_for_watch_cleanup_with_one_shared_fifteen_second_budget`, `watch_start_cleanup_panic_and_takeover_reservation_keep_one_occupancy`, `gateway_worker_uses_the_production_rendered_reader` |
| 10 | `crates/shacs-cli/src/spec042_web_watch_cli.rs:66/232`, `crates/shacs-api/src/spec042_web_watch.rs:18/60/168/186`; `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs:8`, `crates/shacs-api/tests/it/spec051_web_watch_api.rs:71` |
| 11 | 아래 Cargo 실행 기록. 전체 workspace 테스트, Docker, commit은 실행하지 않는다. |

위 표에서 짧게 쓴 구현 파일은 `web_watch_*`·`rendered_web_reader.rs`이면
`crates/shacs-core/src/runtime/`, `browser_manager.rs`·`rendered_load.rs`·`cdp_document.rs`이면
`crates/shacs-cli/src/browser/` 아래다. core 시험은
`crates/shacs-core/tests/it/spec051_rendered_observation.rs`, CLI 시험은
`crates/shacs-cli/tests/it/spec051_rendered_browser.rs`에 있다.

### 시험 이름과 위치

| 시험 | file:line |
|---|---|
| `reading_mode_is_bound_to_preview_plan_and_wake` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:102` |
| `legacy_fetch_records_keep_their_digest_and_schedule` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:196` |
| `rendered_value_drives_existing_condition_decisions` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:240` |
| `rendered_http_failures_keep_retry_after_and_baseline` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:313` |
| `missing_rendered_anchors_are_not_a_new_baseline` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:361` |
| `busy_window_notifies_once_then_recovers` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:415` |
| `notifications_keep_the_confirmed_reading_limits` | `crates/shacs-core/tests/it/spec051_rendered_observation.rs:505` |
| `watch_start_uses_real_process_gate_without_provider` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:624` |
| `rendered_read_waits_for_its_loader_and_keeps_http_facts` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:676` |
| `rendered_read_discards_changed_or_unfinished_documents` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:704` |
| `rendered_text_limit_marks_truncation_without_splitting_utf8` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:725` |
| `watch_uses_public_proxy_temporary_profile_and_closes_popups` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:738` |
| `watch_and_conversation_share_one_slot_without_takeover_waiting` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:876` |
| `watch_start_cleanup_panic_and_takeover_reservation_keep_one_occupancy` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:969` |
| `forget_waits_for_watch_cleanup_with_one_shared_fifteen_second_budget` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:1103` |
| `forget_and_lock_maintenance_interrupt_an_active_watch` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:1230` |
| `gateway_worker_uses_the_production_rendered_reader` | `crates/shacs-cli/tests/it/spec051_rendered_browser.rs:1320` |
| `browser_preview_confirmation_and_status_use_the_selected_reader` | `crates/shacs-cli/tests/it/spec051_web_watch_cli.rs:8` |
| `browser_preview_confirmation_and_status_preserve_mode` | `crates/shacs-api/tests/it/spec051_web_watch_api.rs:71` |
| `spec051_browser_side_expression_bounds_utf8_before_transfer` | `crates/shacs-core/src/runtime/rendered_web_reader_tests.rs:6` |

## 보호 장치 실패 확인

아래 15종류는 production 조건을 하나씩 임시로 바꾸고 해당 시험의 실제 assertion 실패를
확인한 뒤 복원했다. 컴파일 오류를 red 증거로 세지 않았다. 각 복원 뒤 파일 SHA-256이
변경 전과 같은지 확인했다.

| 임시로 깨뜨린 보호 | 관찰한 실패 |
|---|---|
| token 읽기 방식 일치 | fetch로 바꾼 확인이 `InvalidInput`이 아니게 됨 |
| 500 ms 연속 idle | 본문 읽기 시각이 600 ms 대신 0 ms가 됨 |
| 본문 전후 document epoch | 바뀐 문서를 성공으로 반환함 |
| 감시 중 takeover 즉시 거절 | 시험 시계가 500 → 35,500 ms로 진행함 |
| 정리 후 filesystem lock 해제 | child 정리 중 active lock을 다른 쪽이 잡음 |
| maintenance의 watch 취소 신호 | forget 중 취소 검사가 `Cancelled` 대신 `Ok` |
| generation별 새 취소 신호 | 앞 generation의 닫기가 다음 generation도 취소함 |
| production worker reader 배선 | 실제 gateway worker가 가짜 DevTools 읽기 경계에 도달하지 않음 |
| 공개 주소 guard | 사용자 whitelist의 loopback에 SOCKS 성공(0), 예상 거절은 2 |
| paused popup 거절 | popup을 닫기 전에 재개하고 `rejected` 표식이 없음 |
| 브라우저 내부 UTF-8 byte 제한 | 1,048,575 byte 대신 1,048,582 byte를 반환함 |
| 정리 후 점유 retire | 정리 중 takeover가 즉시 거절되지 않고 시험 시계를 진행함 |
| request ID와 주 문서 응답 결속 | 다른 request ID의 응답으로 성공함 |
| 임시 프로필 강제 | `persistLogin=true`에서 저장 프로필의 실행 인자를 사용함 |
| shutdown의 generation 취소 | 다른 닫기 요청 없이 shutdown만 보냈을 때 취소 신호가 오지 않음 |

추가로 시작 가능 여부를 확인하는 사이 전역 닫기가 `NothingOpen`을 반환하는 경합을
새 시험으로 먼저 재현했다. `deferred`와 nonblocking slot 획득을 한 경계로 묶어
시작 가능 여부를 확인하기 전에 generation과 취소 신호를 게시하도록 고쳤다.
수정 뒤 같은 요청은 `Scheduled`이고 child 시작은 0이다.

비활성화 상태에서 실행한 명령은 아래와 같다. 각 실행은 해당 시험 1개의 실패를
확인했다. 같은 명령을 서로 다른 보호 장치에 사용한 경우는 위 표의 별도 행으로 센다.

```sh
cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_rendered_observation::reading_mode_is_bound_to_preview_plan_and_wake
cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q --locked spec051_browser_side_expression_bounds_utf8_before_transfer
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::rendered_read_waits_for_its_loader_and_keeps_http_facts
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::rendered_read_discards_changed_or_unfinished_documents
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::watch_and_conversation_share_one_slot_without_takeover_waiting
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::watch_start_cleanup_panic_and_takeover_reservation_keep_one_occupancy
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::forget_and_lock_maintenance_interrupt_an_active_watch
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::gateway_worker_uses_the_production_rendered_reader
cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_rendered_browser::watch_uses_public_proxy_temporary_profile_and_closes_popups
```

## Cargo 기록

직접 실행한 Rust 명령은 모두 `--manifest-path crates/Cargo.toml`을 사용했고,
별도 Cargo 명령은 앞 명령의 종료 뒤 시작했다. `CARGO_TARGET_DIR`를 지정하거나
새 crate·feature·lockfile 변경을 하지 않았다. 전체 패키지 실행에서 발견한 기존
release 시험의 내부 자식 Cargo는 허용 범위를 확인했고, 해당 release target을
시험별 직렬 실행으로 다시 검증했다.

| 명령 | 결과 |
|---|---|
| `cargo check --manifest-path crates/Cargo.toml -p shacs-core -p shacs-cli -p shacs-api --locked` | 통과 |
| `cargo check --manifest-path crates/Cargo.toml -p shacs-core -p shacs-cli -p shacs-api --all-targets --locked` | 개발 중 새 field와 callback 인자에 맞지 않은 기존 fixture를 발견해 수정. 최종 all-targets 검사는 Clippy로 통과 |
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --config imports_granularity=Module,format_macro_matchers=true` | 적용 |
| `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true` | 최종 소스 통과 |
| `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets --locked -- -D warnings` | helper의 `unwrap()` 21곳을 명시적 실패 맥락으로 수정한 뒤 최종 소스 통과 |
| `cargo clippy --manifest-path crates/Cargo.toml -p shacs-cli --all-targets --features spec031-test-fixture --locked -- -D warnings` | 최종 feature 구성 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q --locked spec051_` | 7개 통과, 최종 core 코드에서 10회 연속 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q --locked spec051_` | 1개 통과. 기존 Boa로 실제 제한 식 실행 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-api --test it -q --locked spec051_` | 1개 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --features spec031-test-fixture --test it -q --locked spec051_` | 최종 11개씩 10회 연속 통과. 반복 루프 누계 35회 중 34회 성공, 1회 실패를 고친 결과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core -p shacs-cli -p shacs-api --locked -q` | 종료 코드 0. API 104개 단위/23개 통합, CLI 472개 단위/197개 통합, core 504개 단위/640개 통합 통과·기존 통합 시험 2개 ignored, 나머지 target도 통과. 기존 내부 자식 Cargo 구간은 아래 별도 명령으로 재검증 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli -q --locked` | 마지막 시험 보완 뒤 단독 재실행, 종료 코드 0. 472개 단위/197개 통합과 나머지 target 통과. 기존 SMTP opt-in 3개 ignored 유지 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test spec033_automation_dispatch -q --locked` | 중첩으로 지적된 target을 다른 Cargo 없이 단독 재실행, 17개 통과 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test spec033_release_runner -q --locked -- --test-threads=1` | 단독 전경 재실행, 15개 통과·기존 수동 fixture 1개 ignored, 122.09초, 종료 코드 0. 자동 분리 뒤에도 다른 Cargo를 시작하지 않고 종료를 확인 |

개발 중 발견한 실패는 legacy digest 기대값에 가려진 출력용 anchor를 사용한 시험,
token 거절 사례의 잘못된 위치, callback/새 field의 fixture 호환, CLI 전역 `--config`
위치, Darwin SOCKS 시험의 요청 전송 방식, takeover 시험 시계 연결이었다. 각각 원인을
고친 뒤 재실행했다. 실패 시험을 지우거나 assertion을 약하게 바꾸지 않았다.
`imap-proto v0.10.2`의 기존 future-incompatibility 안내는 남아 있다.

마지막 반복의 5회차에는 forget 성공 경로의 시험 시계가 1,000 ms 대신 1,200 ms가
되는 실패가 한 번 있었다. 정리 완료 통지와 실제 OS flock 해제를 같은 순간이라고
가정한 fixture를 고쳤다. 다음 가상 시간 단계 전에 active/browser directory flock이
실제로 해제됐는지 관찰하며, 기존 1,000 ms 및 공유 15,000 ms assertion은 그대로 유지했다.
그 뒤 같은 11개 그룹이 10회 연속 통과했다.

중첩의 기존 경로는 `crates/shacs-core/tests/spec033_release_runner.rs:266` →
`crates/shacs-core/src/runtime/spec033_release/release_runner.rs:198` →
`crates/shacs-core/src/runtime/spec033_release.rs:44`다. 기존 시험을 고치거나
건너뛰지 않았다. 부모가 기다리는 동안 기존 시험이 시작하는 자식 Cargo는 허용됨을
확인한 뒤 해당 target을 `--test-threads=1`로 직렬 재실행했다.

## 문서 보완과 미검증 범위

- 대화의 기존 수정 요약에는 읽기 방식이 없다. PRD 010 전까지 browser 감시 수정은
  CLI/API 확인으로 안내하고 거절한다. fetch 대화 동작과 두 방식의 상태 조회는 유지한다.
  SPEC의 2026-11-12 기록에 이 보완을 남겼다.
- gateway/CLI 시험은 기존 `spec031-test-fixture` feature와 같은 `tests/it` 실행 파일의
  별도 process를 사용한다. 실제 manager를 process singleton에 먼저 넣고 실제
  `from_bundle`과 production registry를 거친다. worker reader를 직접 주입하지 않는다.
  공식 container의 관측 사실은 기존 049 fixture를 쓰고 실제 ProcessGate가 판정한다.
- 고정 DOM 식은 core의 기존 Boa로 실행해 제한을 확인한다. 실제 Chromium의 렌더링,
  Docker 및 SMTP 전체 실행은 이 작업에서 확인하지 않았으며 PRD 010 범위다.
- 기존 opt-in/대화형 시험의 `ignore` 표식은 변경하지 않았다. 여기에는 Spec 036의
  production API/SMTP 시험 3개, 설치된 모델을 요구하는 시험, 수동 release fixture와
  대화형 driver가 있다. 기존 subprocess probe는 각 상위 시험이 실행한다.
- 독립 검토는 이 구현 작업의 완료 선언에 포함하지 않는다.

## 이번 작업의 변경 파일

기존 001–050 문서는 수정하지 않았다. 아래의 기존 시험 파일 변경은 새 field,
취소 인자, paused-target callback의 호환과 user takeover 중 watch 거절 확인이다.
PRD 010 문서는 보존했다. `Cargo.toml`과 `Cargo.lock`은 바꾸지 않았다.

```text
README.md
docs/USAGE.md
docs/specs/051-browser-rendered-web-watch/SPEC.md
docs/specs/051-browser-rendered-web-watch/prds/000-rendered-observation.md
docs/specs/051-browser-rendered-web-watch/IMPLEMENTATION-000.md
crates/shacs-api/src/media_api/adapter.rs
crates/shacs-api/src/spec042_web_watch.rs
crates/shacs-api/tests/it/main.rs
crates/shacs-api/tests/it/spec042_web_watch_api.rs
crates/shacs-api/tests/it/spec051_web_watch_api.rs
crates/shacs-cli/src/automation_worker.rs
crates/shacs-cli/src/browser/browser_manager.rs
crates/shacs-cli/src/browser/browser_process.rs
crates/shacs-cli/src/browser/cdp_connection.rs
crates/shacs-cli/src/browser/cdp_document.rs
crates/shacs-cli/src/browser/mod.rs
crates/shacs-cli/src/browser/rendered_load.rs
crates/shacs-cli/src/lib.rs
crates/shacs-cli/src/spec035_cli_media/tool_registry.rs
crates/shacs-cli/src/spec042_web_watch_cli.rs
crates/shacs-cli/src/spec050_browser_ingress_tests/stage.rs
crates/shacs-cli/src/spec051_gateway_fixture.rs
crates/shacs-cli/src/surface_approval_worker/fixture/support.rs
crates/shacs-cli/tests/it/main.rs
crates/shacs-cli/tests/it/spec042_web_watch_cli.rs
crates/shacs-cli/tests/it/spec049_browser_registry.rs
crates/shacs-cli/tests/it/spec049_browser_runtime.rs
crates/shacs-cli/tests/it/spec049_browser_viewer.rs
crates/shacs-cli/tests/it/spec050_browser_saved_login.rs
crates/shacs-cli/tests/it/spec050_browser_takeover.rs
crates/shacs-cli/tests/it/spec051_rendered_browser.rs
crates/shacs-cli/tests/it/spec051_web_watch_cli.rs
crates/shacs-core/src/runtime/durable_filtered_reads_tests.rs
crates/shacs-core/src/runtime/mod.rs
crates/shacs-core/src/runtime/rendered_web_reader.rs
crates/shacs-core/src/runtime/rendered_web_reader_tests.rs
crates/shacs-core/src/runtime/web_watch.rs
crates/shacs-core/src/runtime/web_watch_conversation.rs
crates/shacs-core/src/runtime/web_watch_conversation_setup.rs
crates/shacs-core/src/runtime/web_watch_conversation_status.rs
crates/shacs-core/src/runtime/web_watch_conversation_summary.rs
crates/shacs-core/src/runtime/web_watch_decision.rs
crates/shacs-core/src/runtime/web_watch_dispatch.rs
crates/shacs-core/src/runtime/web_watch_fetch.rs
crates/shacs-core/src/runtime/web_watch_notify.rs
crates/shacs-core/src/runtime/web_watch_notify_events.rs
crates/shacs-core/src/runtime/web_watch_notify_render.rs
crates/shacs-core/src/runtime/web_watch_read.rs
crates/shacs-core/src/runtime/web_watch_record.rs
crates/shacs-core/src/runtime/web_watch_surface.rs
crates/shacs-core/tests/it/main.rs
crates/shacs-core/tests/it/spec042_web_watch/conversation.rs
crates/shacs-core/tests/it/spec042_web_watch/failures.rs
crates/shacs-core/tests/it/spec042_web_watch/support.rs
crates/shacs-core/tests/it/spec051_rendered_observation.rs
```
