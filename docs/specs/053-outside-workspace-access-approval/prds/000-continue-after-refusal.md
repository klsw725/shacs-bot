# PRD 000. 거절 뒤 계속하기

Status: Implemented (independent review approved 2026-11-16)

Depends on: 없음. 004의 tool 실행과 결과 분류, 010의 안전 장치와 경로 검사, 020의 deferred bridge, 022의 승인 재개, 037 PRD 040의 출력 가리기, 048의 판정 표 5번과 실행하지 않은 호출 구분을 소비한다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

도구가 정책 경계를 이유로 거절해도 어시스턴트가 결과와 안내를 읽고 일을 이어 간다. 같은 요청 안에서 실제 도구·거절 종류·대상이 같은 거절이 세 번째이면 `tool_error`로 멈춘다. 승인 질문에 답해 이어진 실행도 같은 요청으로 센다.

## Scope

1. 지금 `is_workspace_violation`이 알아보는 tool 결과와 auto mode 판정 표 5번 `ToolRefusal`을 거절 사건으로 받는다. classifier가 실행하지 않은 호출(`auto_mode_blocks.did_not_run`)은 제외한다. 거절 사건을 받는 인터페이스는 종류와 대상을 받아 뒤 PRD의 비대화형 거절도 같은 셈을 쓰게 한다. 이 PRD는 그 거절을 새로 만들지 않는다.
2. 결과를 받은 자리에서 한 번만 세고 그 결과의 실패 등급을 고정한다. 첫째와 둘째는 `Failed { class: Recoverable }`, 셋째부터는 `Fatal`이다. `fail_on_tool_error`는 첫 실패부터 `Fatal`이다. 이후 종료 판단·실행 기록·이벤트 작성은 고정한 판정만 읽는다.
3. 실제 도구 이름을 쓴다. bridge의 `tool_call`이라는 이름이나 호출 id는 같은 거절의 키에 넣지 않는다. 호출 id 재사용과 이후 dispatch는 각각 새 결과이다.
4. 종류는 workspace 경계, working directory 해석 실패, file tool 범위, deny pattern, 잘못된 deny pattern, allow pattern 불일치, 내부 주소, 나머지 실행 전 자기 거절로 구분한다. 경로 없는 종류는 종류만으로 센다. 경로 있는 종류의 정규화와 저장은 아래 결정대로 한다.
5. 일반 사용자 요청은 새 root에서 시작한다. 승인 후 실행한 도구와 그 뒤 모델 실행은 원래 root를 쓴다. root id는 runtime의 사용자 turn 식별자에 process id·시각·process 내 단조 증가 번호를 붙인다. 대화 길이가 줄거나 같은 메시지 id가 다시 와도 새 시작이 예전 root를 쓰지 않으며, 모델 인자에서 읽지 않는다. 세션 metadata에 root별 횟수를 저장하고 승인 문맥에 root id를 넣어 재시작 뒤에도 찾는다. 서로 다른 root의 횟수는 합하지 않는다.
6. 회복 가능한 결과에는 아래 안내와 현재 횟수를 붙인다. 이 글은 권한을 주지 않는다.

   ```text
   This refusal is a policy boundary; retrying the same action will not change it. Do not bypass it with symlinks, encoding, another tool, or a different working_dir. Work inside the workspace if possible; otherwise tell the user what cannot be done and ask how to proceed.
   ```

   workspace guard이면 `The user can turn off tools.restrictToWorkspace; other guards may still refuse the call.`을, 내부 주소이면 `The user can configure tools.ssrfWhitelist for an intended internal URL.`을 더한다. 현재 PRD는 sandbox가 없을 때의 guard 조건을 바꾸지 않으므로 설정을 끄면 반드시 실행된다고 말하지 않는다.
7. 한도에 닿거나 `fail_on_tool_error`로 멈춘 일반 요청은 기존 거절 결과가 마지막 답이다. 중단하는 결과에는 범위 6의 안내·횟수·설정 설명을 붙이지 않는다. 이미지·복합 요청은 실행 중과 승인 뒤 실행 모두 037 PRD 040의 digest 가리기를 그대로 쓴다.
8. `tool_refusal` 진행 이벤트에 실제 tool, 종류, 횟수, 계속/중단을 담는다. 인자·경로·원문은 넣지 않는다. 실행 기록에도 recoverable/fatal 차이가 남는다.

## 결정과 이유

### 실행 순서

단순히 `fatal_tool_message`에서 세면 같은 결과를 여러 번 세고, 배치가 끝난 뒤에야 셋째를 알아보므로 뒤의 쓰기를 막지 못한다. 결과를 받아들이는 경계를 따로 두고 다음 호출을 시작하기 전에 확인한다.

순차 실행과 classifier 실행, bridge는 호출마다 결과를 받아들인다. 일반 동시 실행에서는 기존 `preflight_refusal`을 먼저 읽는다. 하나라도 실행 전 거절이 있으면 그 묶음을 순차 실행한다. 사전 확인은 실행 여부를 결정하거나 횟수를 늘리지 않는다. 기존 권한 판정이 먼저 적용되고 실제 반환된 결과만 센다. 따라서 deny·ask·classifier 판정의 우선순위가 바뀌지 않는다.

모든 호출을 무조건 순차 실행하는 방식은 거절과 무관한 읽기의 동시 실행까지 없애므로 고르지 않았다. 실행 전 통과한 호출이 파일 상태 변화나 외부 도구의 결과 때문에 실행 중 처음 거절되는 경우, 같은 동시 실행 묶음의 다른 읽기 전용 호출은 이미 시작됐을 수 있다. 이 경우 결과는 호출 순서로 세지만 이미 시작한 읽기는 취소하지 않는다. 함께 묶는 것은 `concurrency_safe()`인 읽기 전용 호출뿐이므로 쓰기는 이 예외에 들지 않는다. 실행 전에 알 수 있는 거절 셋 뒤의 호출은 순차·동시 설정 모두 시작되지 않으며, 순차 설정에서는 실행 중 생긴 거절도 엄격히 중단한다.

### 대상과 저장

file tool은 `path`, working directory 오류는 `working_dir`을 쓴다. 명령의 workspace 거절은 기존 명령 분석이 찾은 절대·home·상위 폴더 경로들을 정규화하고 중복 제거·정렬한 목록을 쓴다. 상대 경로의 기준은 요청한 working directory, 없으면 workspace다. `~`는 home으로 풀고, `.`·`..`를 접고, 기존 symlink와 존재하는 가장 가까운 상위 폴더를 해석한다. 명령 글에서 경로를 해석할 수 없으면 종류만으로 센다. 이 분석은 거절 키에만 쓰며 guard의 허용 범위를 넓히지 않는다.

저장 키는 실제 tool·종류·정규화한 대상의 SHA-256이다. metadata `tool_refusal_counts`는 `{root_id: {거절 키: 횟수}}`이고, 원래 경로나 명령을 저장하지 않는다. 승인 문맥의 `tool_refusal_root`가 이어갈 root를 지정한다. 빈 횟수는 저장하지 않는다. 요청 완료, 승인 거절, 기억 규칙 저장 실패, 최근 거절 재시도 종료에서 그 root의 횟수를 지우고 `/new`는 전체를 지운다. 저장 횟수나 root 형식이 잘못됐으면 0으로 숨기지 않고 해당 metadata 키가 적힌 세션 오류로 돌려준다.

승인 재개에서 빈 인자로 호출을 재구성하기 전에 실행 보고서의 원래 action으로 거절을 받아들인다. 이후 빈 인자로 시작하는 실행 기록은 거절 키의 근거가 아니다. 이 구조는 뒤 PRD가 출처별 root id와 새 거절 종류를 넘길 수 있는 경계이며, cron 등의 출처 전달은 이 PRD에 넣지 않는다.

## Non Scope

- `exec` guard 활성 조건, file tool 허용 범위, sandbox와 process gate, 권한 판정 순서 변경.
- workspace 밖 접근 승인, 비대화형 실행 출처, cron 사전 허락 목록(PRD 010/020/030).
- classifier 막힘의 3회 연속·20회 누적 기준(048 PRD 020).
- 이미지·복합 요청 가리기 범위 확대.

## Parent Requirement Mapping

- 053 소유 범위 4 전체와 6 중 이 동작의 표시·문서를 소유한다.
- 독립 종료 조건은 수용 조건 1과 이 변경에 대한 수용 조건 6이다.
- 앞선 004/048의 즉시 중단 계약과 037의 중단 계기를 바꾼다. 앞선 스펙 문서는 고치지 않는다.

## Acceptance Criteria

1. 가짜 provider와 실제 `AgentLoop`에서 workspace 밖 명령, deny pattern, 내부 주소, 범위 밖 읽기, auto mode 표 5번 거절 뒤에 실패 결과와 안내를 읽고 다음 호출로 끝낸다.
2. 같은 거절 셋째가 `tool_error`이고, 같은 묶음의 뒤 쓰기는 시작하지 않는다. 순차·동시 설정을 확인한다. 다른 대상은 별개이고 bridge는 실제 tool로 합산한다.
3. 승인 후와 재시작 후에도 횟수가 이어지며 다음 사용자 요청은 0부터다. `fail_on_tool_error`는 첫 거절에서 멈춘다.
4. 048의 실행하지 않은 호출 시험은 유지한다. 037의 실행 중·승인 후 오류 가리기 시험은 세 번째 거절을 중단 계기로 바꿔 보호 범위를 확인한다.
5. fmt, workspace clippy, core lib와 변경한 통합 시험 모듈이 통과한다. 전체 workspace 테스트는 실행하지 않는다.

## 구현 메모 및 검증 근거

- `tool_refusal.rs`가 거절 종류와 대상, `tool_refusals.rs`가 횟수와 결과별 고정 판정을 맡는다. 결과의 호출 id와 결과 글의 digest로 판정을 조회한다. 회복 가능한 결과에는 횟수가 붙고 중단 결과는 원문을 유지하므로, 같은 id를 다시 써도 앞선 회복 결과와 중단 결과를 구분한다. classifier가 실행하지 않은 결과도 받은 자리에서 별도로 고정해 id를 재사용해도 거절 횟수에 넣지 않는다. action은 호출 id별 대기열로 연결하고, 같은 id 안에서는 순서를 지킨다. action 하나가 빠져도 다른 id의 결과 연결에 영향을 주지 않는다.
- `runner.rs`는 직접 호출·classifier 호출·bridge 결과를 각각 다음 실행 전에 받아들인다. 정상 병렬 실행은 연속한 concurrency-safe 호출만 묶고, 그 묶음의 결과를 확인한 뒤 다음 묶음으로 간다. 실행 중 거절의 잔여 한계는 이미 시작된 같은 묶음에만 있다.
- `agent_loop.rs`는 승인 실행 결과와 그 뒤 run이 같은 상태를 쓰게 하고 checkpoint·승인 대기 시점에 저장한다. 완료한 요청의 상태는 정리한다.
- 048의 호출 id 재사용 시험은 실제 거절을 세 번으로 옮기고, classifier 막힘 글이 호출 id 충돌 때문에 도구 거절로 바뀌던 기존 기대는 제외 계약에 맞게 바꿨다. `a_deferred_process_tool_without_a_container_proof_is_refused_with_the_gate_text`도 표 5번 `ToolRefusal`이므로 원래 process gate 거절 뒤 안내를 붙이는 기대로 바꿨다. process가 시작되지 않고 classifier도 부르지 않는 확인은 유지한다. 037의 `composed_errors`·`composed_output`은 같은 원문 유출 검사를 유지하며 세 번째 거절로 멈추게 했다. 같은 즉시 중단 계약을 쓰던 `report_terminal`·`calculation_terminal`도 세 번째로 바꿨다.
- 최초 새 시험의 5개 실패는 공용 auto 시험 설정의 mode만 `bypass_permissions`로 바꾸고 부모 권한 한도는 `auto`로 남긴 준비 코드가 원인이었다. `evaluate_inherited_ceiling`의 `ModeWidening`으로 도구 호출이 실행되기 전에 거절되어, 도구 자체의 거절 결과와 반복 제한에 도달하지 않았다. mode와 그 mode의 한도를 함께 만들도록 고쳤다. 승인 시험의 `default`는 `auto`보다 좁아 원래도 이 문제를 겪지 않았다.
- 독립 검토 뒤 중단 원문 보존, fatal skip 이유, 종료 경로의 횟수 정리, 빈 저장 생략을 보완했다. 병렬 시험은 셋째 거절 바로 뒤에 실행 횟수를 기록하는 읽기 전용 도구도 넣어 실행 전 거절 직렬화가 없으면 실패하게 했다. 이벤트 payload와 실행 기록의 recoverable/fatal/skip 이유, 안내의 설정 이름, 승인 거절 뒤 저장 정리도 확인한다.
- 재검토 뒤 횟수 정리는 공용 응답 함수의 종료 이유 문자열이 아니라, 실행 중인 요청이 실제로 끝나는 각 분기에서 자기 root에만 적용한다. 두 번 거절된 뒤 질문을 기다리는 요청을 재시작하고 `/permission recent retry`가 닫힌 응답을 내도, 질문에 답해 이어 간 셋째 거절에서 멈추는 시험을 더했다. fatal 뒤 건너뛴 호출은 마지막 결과 보충 자리에서 한 번만 기록하며 시험은 늦은 중복 기록도 실패로 본다.
- 기존 `is_workspace_violation`의 문자열 기반 인식 범위는 유지했다. 성공한 파일 읽기 결과에 그 문구가 들어 있어도 거절로 알아볼 수 있다는 기존 한계(검토 L2)는 남는다. 이번 변경은 그 문자열 인식 자체를 새 형식으로 바꾸지 않는다.
- `cargo check --manifest-path crates/Cargo.toml -p shacs-core --locked`: exit 0.
- `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`: exit 0.
- `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`: 최초 실행은 승인 결과 함수의 인자 수 경고로 exit 101. 세션에서 얻을 수 있는 중복 인자를 없앤 뒤 exit 0. 기존 의존성 `imap-proto v0.10.2`의 future-incompatibility 알림은 남아 있다.
- 독립 검토 보완 뒤 같은 fmt 검사는 exit 0이다. clippy는 시험 이벤트 콜백의 `unwrap`에 실패해 원인을 적은 `expect`로 바꿨으며, 최종 실행은 exit 0이다.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_agent -q runtime_runner_honors_concurrent_tools_flag`: 1 passed, exit 0.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_agent -q runtime_runner_fail_on_tool_error`: 3 passed, exit 0.
- `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_loop -q stops_after_fatal_tool_outcome`: 2 passed, exit 0.
- 최초 core lib 검증 세션은 504개 시험 실행 중 결과 요약 없이 종료되어 통과로 기록하지 않는다. 당시 오래 실행 중이던 것은 `runtime::generated_media_release::runner`의 격리·정리 시험들이며 실패 판정은 받지 않았다.
- 재검토 보완 전 core lib 시험도 504 passed, exit 0이었으며, 아래 결과는 재검토 수정 후 다시 실행한 것이다. 최종 검증 로그: `/tmp/spec053-prd000-approved-tests.log`. 전체 workspace 테스트와 빌드·`cargo clean`·커밋은 실행하지 않는다.

`cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q spec053_refusals::` — exit 0:

```text
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 649 filtered out; finished in 0.86s
```

`cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q spec048_auto_mode::` — exit 0:

```text
test result: ok. 99 passed; 0 failed; 0 ignored; 0 measured; 558 filtered out; finished in 6.10s
```

`cargo test --manifest-path crates/Cargo.toml -p shacs-core --test it -q spec037_method_selection::` — exit 0:

```text
test result: ok. 124 passed; 0 failed; 0 ignored; 0 measured; 533 filtered out; finished in 10.18s
```

`cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q` — exit 0(재검토 수정 후 최종 코드):

```text
test result: ok. 504 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 589.45s
```

위 코드에서 fmt 검사(Google 옵션)와 workspace clippy `-D warnings`도 exit 0이었다. 독립 코드 검토는 2차에서 승인됐으나, 추가 gate가 아래 혼합 이미지 결과 결함을 발견했다(`.omo/evidence/spec053-prd000-continue-after-refusal-code-review.md`, 추적하지 않음). 전체 workspace 시험과 커밋은 사용자 승인 뒤에 한다.

### 추가 gate: 혼합 텍스트·이미지 거절

- 문제: 거절의 판정을 JSON 배열 원문으로 저장한 뒤 `finalize_tool_message`에서 이미지 저장과 텍스트 변환을 했다. 셋째 결과는 변환 뒤 키가 달라져 저장한 `Fatal`을 찾지 못했다. 앞의 두 결과에는 JSON 배열 뒤에 안내를 붙여 배열 자체를 읽을 수 없게 만들었다.
- 재현: `spec053_refusals::mixed_results::image_conversion_preserves_the_third_refusals_fatal_decision`은 수정 전 세 번째 이벤트의 `fatal: true`를 확인한 뒤 실패했다. 실제 종료 이유가 `tool_error` 대신 추가 provider 요청의 `error`였고, 명령은 exit 101이었다.
- 수정: 직접 호출·classifier 호출·승인 실행이 쓰는 `admit_tool_report`와 bridge의 결과 수신 자리에서 먼저 이미지 배열을 저장하고 텍스트로 바꾼다. 그 뒤 거절을 세고 필요한 안내를 붙인다. 판정 조회 전에 결과 내용을 바꾸는 단계를 없앴으며 `finalize_tool_message`도 결과를 불변 참조로 받는다. 공개 결과 타입에 새 식별자를 더하는 대신, 기존 이미지 표현 변환을 한 번만 먼저 끝내는 방식을 골랐다.
- 회귀 시험: 실제 `AgentLoop`를 직접 호출·bridge·승인 재개 세 경로로 돌린다. 두 회복 가능한 거절의 안내와 이미지가 다음 provider 요청에 도착하고, 세 번째는 실행 기록에도 `Fatal`이며, 요청은 `tool_error`로 끝나고 추가 provider 요청과 뒤 쓰기는 시작되지 않아야 한다. 승인 실행 기록은 턴마다 비워지므로 각 승인 직후의 실패 등급을 모아 확인한다.
- 이 추가 수정 뒤 최종 코드의 검증(2026-11-16):
  - `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`: exit 0.
  - `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`: exit 0(기존 `imap-proto v0.10.2` future-incompatibility 알림만 남는다).
  - `cargo test --manifest-path crates/Cargo.toml -p shacs-core -q --no-fail-fast`: exit 0, 결과 요약 79개 합계 2,043 passed / 0 failed(lib 504 passed, `it` 656 passed / 2 ignored: 하위 프로세스용 `proxy_probe`, 기본 model이 필요한 `real_model`). 로그 `/tmp/spec053-prd000-pkg2.log`.
  - `shacs-cli`(public adapter와 surface approval worker가 `AgentLoop`를 만든다): `--lib -q public_adapter_approval_reuses_stored_scope_and_ceiling_for_real_exec` 1 passed, `--lib -q surface_approval_consumer_` 6 passed, `--test it -q spec048_ask_verbose::` 2 passed, 모두 exit 0. 로그 `/tmp/spec053-prd000-cli2.log`.
- deep-high 독립 검토: 1차(2026-11-16)는 위 혼합 이미지 결함으로 수정 요청, 수정 뒤 2차는 승인(차단 없음). 2차는 직접·병렬·classifier·bridge·승인 실행 경로가 모두 이미지 변환 뒤 거절을 받아들이고, 이미지 저장이 두 번 일어나지 않으며, 037 가리기 순서가 이전과 같음을 확인했다. 위 최종 검증 결과는 이 승인 뒤의 기록이다.
