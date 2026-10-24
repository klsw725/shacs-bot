# PRD 010. 재현 가능한 계산과 결과 확인

Status: Complete (Scoped)

Depends on: [PRD 000](000-method-selection-and-user-override.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

사용자가 맡긴 한 계산 문제를 실제 코드 실행 결과와 별도의 확인 근거로 설명한다. **보고서나 이미지 제작 없이 계산 답·가정·검증 상태를 보여주는 한 사용자 흐름으로 완료한다.**

## Scope

1. 선택된 계산 방식에서 사용한 입력·단위·가정·실행된 명령/코드·출력과 종료 상태를 같은 요청에 기록한다. 기존 tool/runtime 경계로 Python을 실행할 수 있지만 전용 kernel을 완료 조건으로 고정하지 않는다.
2. 결과에 영향을 주는 반례 또는 독립적인 확인을 수행하고 확인한 범위, 실패한 항목, 미검증 부분을 구분한다. 오류를 수정해 재실행한 경우 이전 결과를 검증 완료로 남기지 않는다.
3. 사용자에게 답과 핵심 근거를 먼저 보여주고, 코드·입력·상세 실행·확인 기록은 요청하면 볼 수 있게 한다. tool success, 정답 여부, 사용자 수용은 서로 다른 사실이다.

## Non Scope

- 보고서 출처/구성, 이미지 provider·artifact, 복합 결과 병합, 전용 Python interpreter service.
- 보장되지 않는 floating point 완전 정확성, 모든 계산 분야의 의미 검증, 030 trusted execution/sandbox 재정의.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 3 중 계산 검증, 범위 4의 계산 결과/상세 조회를 소유한다.
- 부모 수용 조건 2 전체와 조건 5의 계산 happy path·실패 실제 표면을 소유한다.

## Acceptance Criteria

1. 실제 CLI/대화 표면에서 단위와 가정이 있는 제어 계산을 실행하고 답·입력·독립 확인·상세 코드를 확인한다. 단순 `exit 0`만으로 `verified`가 되지 않는다.
2. 반례 입력, 잘못된 단위, 실패한 실행 또는 서로 다른 두 결과를 주면 오류/불확실성이 드러나고 이전 검증 상태를 조용히 재사용하지 않는다.
3. Python 또는 필요한 패키지를 쓸 수 없는 구성은 unavailable/unsupported로 반환하고 계산 완료를 합성하지 않는다. 실행은 당시 030 hook/permission/process 경계를 따른다.

## Closure Evidence

- 실제 입력·코드·도구 결과·독립 확인과 최종 답의 연결 기록.
- 단위/반례/실행 실패 및 제공되지 않는 환경의 회귀 테스트와 사용자-visible 제한.

PRD 010은 **하나의 계산 요청을 재현하고 검증된 범위만 답하는 기능**으로 닫는다. 뒤 번호 PRD의 보고서·이미지·복합 출력은 필요하지 않다.

## 구현 메모 및 검증 근거 (2026-10-22)

아래는 구현·실행 기록이다. 완료 판정은 마지막 종료 확인 절에 기록한다.

- **실행 경로 결정:** 기존 Spec030 process gate는 바꾸지 않았다. native host에서는 승인 뒤에도 containment를 확인할 수 없어 `exec`가 막히며, 이는 Spec037 이전 baseline에서도 재현된다. 그래서 계산은 막힌 실행을 `unavailable`로 정직하게 표시한다. 실제 Python 실행 happy path는 공식 container와 같은 표지(`SHACS_RUNTIME_PACKAGE=shacs-bot-official-container`, 비root, docker socket 없음)를 갖춘 Docker container 안에서 확인했다.
- **기록:** 계산 방식 요청의 `exec` 실행마다 다음을 같은 요청의 `calculation` 기록에 남긴다. selection revision에 귀속한다.
  - redaction한 명령(2,048자)과 출력(1,024자), 명령 SHA-256, 종료 상태(`Exit code`, gate 거부, 오류, 취소).
  - 결과 줄 `SHACS_CALC {json}`(role, answer, unit, inputs, assumptions, method).
  - 일반 provider 실행은 기존 hook/tool-event 경계로, 승인 후 재개 실행은 기존 execution report로 관측한다. 다른 방식의 요청에서는 계산 기록을 만들지 않는다.
- **판정(`task_method_calc_eval`):** 같은 revision의 실행만 결정적으로 판정한다.
  - 마지막 primary 뒤에 성공한 다른 코드의 check가 같은 값(상대 1e-9)과 단위를 낼 때만 `verified`다.
  - 값·단위가 하나라도 다르면 `conflict`다.
  - check가 없거나 실패했거나 같은 코드면 `not verified`다.
  - primary가 실패하면 `failed`다.
  - gate 거부, Python 없음(127), `ModuleNotFoundError`는 `unavailable`이다.
  - 결과 줄이 없으면 `not recorded`, 실행이 없으면 `not run`이다.
  - 종료 코드 0이나 모델의 주장은 판정에 쓰지 않는다. 재실행 뒤에는 이전 check를 재사용하지 않는다.
- **확인 항목:** 판정은 PRD000의 validation claim 확장점에 inputs/units/assumptions/independent check로 기록된다. 그래서 방식 변경 시 PRD000의 `superseded_not_inherited` 규칙을 그대로 따른다.
- **표면:**
  - 답 다음에 판정, 확인된 답 또는 확인되지 않은 주 계산 결과, 입력, 가정, 실행 상태, 사용자 수용 여부를 보여 준다. `/method status`는 실행별 명령·출력·결과 줄과 `superseded` 여부를 보여 준다.
  - Provider projection에는 계산 규칙(결과 줄 형식, 독립 check, 판정은 runtime이 한다는 점)을 넣는다.
  - `/method accept`는 사용자 수용을 별도 사실로 기록하고 판정은 바꾸지 않는다. 사용자 아닌 turn에서는 거절한다.
- **기존 기대 변경:**
  - PRD000 테스트 중 "계산 방식도 validation claim이 비어 있다"는 기대를 "계산 방식은 not verified claim 4개"로 바꿨다.
  - "method metadata에 raw 명령이 없다"는 기대를 "외부 효과 기록에는 raw 명령이 없고, 계산 실행 기록에는 redaction한 명령이 있다"로 바꿨다.
  - 둘 다 PRD010 요구에 따른 의도적 변경이며 USAGE에 반영했다.
- **테스트:**
  - 판정 단위 테스트 6개.
  - AgentLoop 계산 시나리오 6개: verified, 단위 오류로 인한 conflict, 재실행, 실패·거부·Python 없음, 사용자 수용, 방식 변경.
  - compiled REPL native 거부 테스트 1개.
  - 사용자 아닌 turn의 `/method accept` 거절은 기존 시나리오에 추가했다.
- **실제 표면:**
  - `.omo/evidence/spec037/prd010/qa-docker-official-container-final.txt`(최종 binary, `docker-qa.sh`로 재현 가능. 이전 문구의 첫 실행은 `qa-docker-official-container.txt`): 공식 container 표지를 가진 Docker 안 REPL에서 실제 `python3` 실행으로 verified, conflict(600 vs 6), numpy 없음 unavailable, `/method status` 상세, `/method accept`, history를 확인했다.
  - `qa-native-macos.txt`: macOS native REPL에서 승인 후 gate 거부가 `unavailable`로 표시됨을 확인했다.
- **전체 검증:** 최종 코드로 `cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast`를 실행했다(`.omo/evidence/spec037/prd010/workspace-test.log`, 집계 스크립트 `count-tests.py`). Oracle 1–6차 보정을 모두 반영한 결과다.
  - 결과는 suite 325개 기준 **3,178 passed / 1 failed / 13 ignored**, 1,306초다.
  - 직전 실행(`workspace-test-flaky-ownership-lock.log`)에서는 Spec037과 무관한 기존 `shacs-cli` 테스트 `ownership_process_lock_is_released_with_its_guard`가 한 번 실패했다. 이 테스트는 파일 lock 해제를 확인한다. 같은 코드에서 단독 3회와 `shacs-cli --lib` 전체 394개를 다시 실행하니 모두 통과했다(`shacs-cli-lib-rerun.log`). 그 뒤 전체 workspace를 다시 실행한 결과가 위 수치다.
  - 집계 기준: `test result` 줄을 단순 합산하면 3,186이다. 차이 8은 일부 테스트(`durable_event_store_serializes_independent_processes` 등)가 띄운 하위 test process의 결과 줄이다. 이 줄은 `filtered out`이 0이 아니며, 출력이 섞여 `test result: okok`처럼 기록되기도 한다. 그래서 `filtered out`이 0인 suite 줄만 합산한다. 이전 실행들의 단순 합계에도 같은 하위 결과 줄이 포함되어 있었다.
  - 같은 기준으로 baseline은 3,111, PRD000 종료 시점은 3,150이며, PRD010의 새 테스트 28개가 모두 통과했다. 내역은 core 단위 13, AgentLoop 시나리오 14, compiled REPL 1이다. 유일한 실패는 baseline과 같은 Spec034 fixture 누락 테스트다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
- **Oracle gate 1차 판정(BLOCKED) 후 보정:**
  - 확인된 primary·check 뒤에 실패·거부·환경 누락 재실행이 오면 `verified`가 유지되던 문제를 고쳤다. 이제 마지막 primary 뒤에 실패가 하나라도 있으면 `verified`를 금지한다. 거부·Python/package 누락은 `unavailable`, 그 밖은 `not verified`이며, 새 primary와 새 check가 모두 성공해야 다시 `verified`가 된다.
  - 앞선 plugin hook이 `exec`를 막으면 관측 hook이 호출 정보를 받지 못해 `not run`이 되던 문제를 고쳤다. 계산 관측 hook을 가장 앞에 두고, tool event 없이 끝난 `exec`는 실행 결과 메시지에서 거부 결과를 찾아 기록한다. 같은 호출의 최신 assistant 메시지 뒤에 나온 결과만 사용한다.
  - 판정·확인 항목·표시·종료 보존이 tool call ID만으로 실행을 찾아, ID가 재사용되면 다른 revision이나 다른 실행을 가리킬 수 있던 문제를 고쳤다. 실행마다 요청 안의 순번을 부여하고 모든 참조를 순번으로 바꿨다.
  - 회귀 테스트를 추가했다: 판정 단위 테스트 2개(실패 재실행의 검증 철회와 복구, 재사용 ID 해석)와 AgentLoop 시나리오 2개(hook 차단, 같은 tool call ID의 primary·check).
  - 수정 전 실패를 재현했다: `oracle-fix-before-failed-rerun.log`, `oracle-fix-before-hook-block.log`. 순번 재설계는 이전 구조로 되돌릴 수 없어, 재사용 ID 테스트로 고정했다. 그 과정에서 이력에 같은 ID의 이전 결과가 있으면 잘못 짝지어지는 결함도 찾아 고쳤다.
- **Oracle gate 2차 판정(BLOCKED) 후 보정:**
  - 확인된 primary·check 뒤에 결과 줄 없이 `Exit code: 0`로 끝난 실행이나 잘못된 결과 줄의 실행이 오면 `verified`가 유지되던 문제를 고쳤다. 이제 마지막 primary 뒤에 유효한 결과 줄이 없는 실행이 하나라도 있으면 `not verified`다.
  - 잘린 명령·출력이 표시 접미사 때문에 문서의 2,048자·1,024자 한도를 넘던 문제를 고쳤다. 이제 접미사를 포함해 한도 안에 든다.
  - 두 결함 모두 수정 전 실패를 재현한 단위 테스트로 고정했다(`oracle2-fix-before.log`).
- **Oracle gate 3차 판정(BLOCKED) 후 보정:** tool event 없이 hook에 막힌 `exec`를 call ID 하나를 키로 보관했었다. 그래서 같은 ID가 재사용되면 앞선 거부 기록이 덮여 사라질 수 있었다.
  - 이제 계산 관측기는 기존 hook 전체를 감싼다. 호출 정보를 먼저 기록하고, 기존 hook이 막은 결과를 그 자리에서 받아 시도 순서대로 거부 실행으로 기록한다. 실행 기록 이력과 짝짓던 사후 보정 경로는 없앴다. 인자를 받지 않는 기존 hook에는 CompositeHook의 기존 인자 축소 규칙이 그대로 적용된다.
  - 회귀 테스트: 확인된 primary·check → 같은 ID로 hook 거부 → 같은 ID 재시도 check 성공 순서에서 거부가 남고 판정이 `unavailable`로 철회됨을 확인한다(`calculation_gates.rs`).
  - 수정 전 실패: `oracle3-fix-before.log`.
- **Oracle gate 4차 판정(BLOCKED) 후 보정과 같은 계열 점검:**
  - 실행 상태와 결과 줄을 따로 해석해, 결과 줄이 섞인 hook 거부 메시지가 결과로 표시될 수 있던 문제를 고쳤다. 이제 성공한 실행의 결과 줄만 보존한다. 실패·거부된 실행의 결과 줄은 무시했다는 기록만 남긴다.
  - 같은 계열로, 사용자가 `exec` 승인을 거절하면 실행 기록이 남지 않아 이전 `verified`가 유지되던 경로를 찾아 고쳤다. 거절한 호출을 거부된 실행으로 기록하므로 판정이 철회된다.
  - provider projection이 계산 규칙 문구 때문에 8,192자 한도를 넘을 수 있던 문제도 고쳤다. 이제 규칙 문구를 포함한 전체가 한도 안에 든다.
  - 회귀 테스트: 단위 테스트 2개(결과 줄이 섞인 실패·거부 실행, projection 한도)와 AgentLoop 시나리오 2개(hook 거부 사유 속 결과 줄, 확인 뒤 사용자 거절)를 추가했다.
  - 수정 전 실패: `oracle4-fix-before-*.log`.
- **Oracle gate 5차 판정(BLOCKED) 후 보정:** 잘못된 인자로 준비 단계에서 거절된 `exec`와, 취소나 치명적 도구 오류로 건너뛴 `exec`는 실행 결과 메시지만 남고 관측 event가 없었다. 그래서 계산 기록에서 빠져 이전 `verified`가 유지될 수 있었다.
  - runner가 이제 해당 호출의 tool event를 발생시킨다. 준비 단계 거절은 기존 `tool_event_for_message`로 `Error` event를 낸다. 취소·치명적 오류로 건너뛴 호출과, 같은 batch의 다른 호출이 승인을 기다리는 동안 실행되지 않고 버려지는 호출은 `Skipped` event를 낸다. 기존 hook 순서와 인자 공개 규칙은 바꾸지 않았다.
  - 계산 관측기는 준비 단계에서 거절된 `exec`를 `<exec call rejected before it ran>` 명령의 실패 실행으로, 건너뛴 호출은 취소 실행으로 기록한다.
  - 회귀 테스트(`calculation_terminal.rs`) 3개:
    - 확인된 primary·check 뒤의 잘못된 인자 `exec`가 기록되어 판정이 철회된다.
    - 치명적 도구 오류로 건너뛴 `exec`가 취소 실행으로 기록된다.
    - 다른 호출의 승인 대기로 버려진 `exec`가 취소 실행으로 기록되어 판정이 철회된다.
  - 수정 전 실패: `oracle5-fix-before.log`, `oracle5-fix-before-interrupt-skip.log`.
- **Oracle gate 6차 판정(BLOCKED) 후 보정:** 준비 단계 전에 취소된 `exec`는 관측기에 아직 등록되기 전이었다. 그래서 `Skipped` event가 와도 후보를 찾지 못해 무시되었다.
  - 이제 등록 후보가 없는 `exec`의 `Skipped` event도 `<exec call skipped before it ran>` 명령의 취소 실행으로 기록한다.
  - 같은 호출에 event가 두 번 오지 않는지도 점검했다. 승인 대기로 버려진 호출은 interrupt가 있을 때만, 치명적 오류·취소로 건너뛴 호출은 `append_skipped_tool_results`에서만 event를 낸다.
  - runner 수준 회귀 테스트는 `awaiting_tools` checkpoint에서 취소해 준비 전 취소 경로를 재현한다(`task_method_observe_tests.rs`). 수정 전 실패는 `oracle6-fix-before.log`다.
  - 점검한 `exec` 종료 경로는 다음과 같다.
    - 실행 완료, hook 차단, 정책·분류기 거부, 시간 초과.
    - 준비 단계 거절, 준비 전 취소, hook 이후 취소, 치명적 오류로 건너뜀, 승인 대기 중 버려짐.
    - 승인 후 실행, 사용자 거절·만료, provider 오류로 중단된 실행.
    - 응답 직후 취소는 호출이 대화에 들어가기 전에 끝나므로 기록할 실행 시도가 없다.

## 종료 확인 (2026-10-22)

- 범위: 계산 방식 요청에서 실제 실행한 코드의 입력·단위·가정·결과를 같은 요청에 기록한다. 다른 코드의 독립 확인으로 판정하며, 답과 핵심 근거를 먼저 보이고 코드·출력은 `/method status`로 보인다. 사용자 수용은 `/method accept`로 판정과 따로 기록한다. 보고서·이미지·복합 결과와 전용 Python 서비스는 포함하지 않는다.
- 최종 검증: 전체 workspace는 suite 325개 기준 **3,178 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline과 같은 Spec034 fixture 테스트다. fmt와 workspace clippy `-D warnings`도 통과했다.
- 실제 표면:
  - 공식 container 표지를 가진 Docker 안 compiled REPL에서 실제 `python3`로 확인했다: verified, 단위 오류 conflict(600 vs 6), numpy 없음 `unavailable`, 상세 조회, 사용자 수용, history.
  - macOS native에서는 기존 process gate 거부가 `unavailable`로 표시됨을 확인했다.
- Oracle 독립 검증: **PASS** (7차).
  - 1–6차에서 다음 결함을 차례로 찾았고, 모두 고친 뒤 수정 전 실패 로그와 회귀 테스트로 고정했다.
    - 후속 실패·거부·결과 누락·사용자 거절 뒤에도 검증이 유지되던 문제.
    - event 없이 끝난 `exec` 시도(hook 차단, 준비 거절, 취소·건너뜀)가 기록되지 않던 문제.
    - revision·call ID 재사용에 따른 참조 오류.
    - 결과 줄 밀수, 저장·projection 한도 초과.
  - 이 PASS는 PRD 020–040의 보고서·이미지·복합 결과를 포함하지 않는다.
- 남은 제한:
  - 독립성은 "다른 코드"까지만 기계적으로 확인하며, 실제로 다른 방법인지는 사용자가 상세 기록으로 확인해야 한다.
  - Containment를 확인할 수 없는 native host에서는 기존 Spec030 process gate 때문에 계산 실행이 `unavailable`로 끝난다.
- 이후 변경(PRD040에서, 2026-10-23):
  - 마지막 주 계산 뒤에 취소된 실행만 있으면 이유를 "a run after the latest primary result was cancelled"로 구별한다(판정은 전과 같이 `not verified`). PRD040 Oracle 2차 반영이다.
  - 결과 줄의 `answer`는 수(`6`, `-1.5e3`, `6,000`, `1/3` 등), `unit`은 공백·대괄호 없는 24자 이하 한 낱말이어야 한다. 아니면 "result line problem"으로 기록하고 결과로 쓰지 않는다. 기록하는 글(입력·가정·방법·오류·위치 등)의 줄바꿈과 제어 문자는 공백으로 바꾼다. 실행 코드와 출력은 줄을 그대로 둔다. 복합 요청에서 runtime이 확인된 값을 직접 적기 때문이다(PRD040 Oracle 4차 반영).
