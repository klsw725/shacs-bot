# PRD 040. 복합 작업의 결과 결합과 사용자 인계

Status: Complete (Scoped)

Depends on: [PRD 000](000-method-selection-and-user-override.md), [PRD 010](010-reproducible-calculation-evidence.md), [PRD 020](020-source-grounded-report-output.md), [PRD 030](030-image-result-review-and-revision.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

하나의 요청에서 둘 이상의 작업 방식을 사용하더라도 목표·결정은 하나로 이어가고, 산출물별 확인 상태를 구별한 **답 우선 최종 결과**를 사용자에게 인계한다. 이 PRD가 [037 SPEC](../SPEC.md)의 실제 복합 사용자 흐름을 닫는다.

## Scope

1. 기존 024 workflow/session 경계에 계산 결과·보고서 출처·생성 artifact의 안전한 참조를 연결한다. 한 단계의 성공은 다른 단계의 실패·미검증 결과를 통과시키지 않는다.
2. 일반 방식에서 전문 방식으로 바꾸거나 복합 작업 도중 사용자가 방식을 바꿔도 목표·사용자 결정·이미 확인한 산출물은 유지한다. 변경된 출력은 영향받은 검증만 다시 판단한다.
3. 사용자에게 답/산출물과 핵심 근거, 미완료·미지원 상태를 먼저 보여주고 코드·자료 출처·각 도구의 실행/검증 상세는 필요할 때 inspect 가능하게 한다.

## Non Scope

- 새로운 workflow engine/child graph, 모든 작업의 자동 병렬화, 사용자 대신 외부 확정 행동, 선제 감시나 방법 학습.
- 구성되지 않은 이미지 provider의 성공이나 보고서에서 읽지 않은 자료의 인용.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 2 및 4의 복합 결과 귀속/답 우선 표면을 소유한다.
- 부모 수용 조건 4 전체와 조건 5의 일반/전문/변경/미지원 실제 사용자 표면 통합을 소유한다. 각 분야 검증의 진실은 앞선 PRD의 산출물을 소비한다.

## Acceptance Criteria

1. 동일 요청의 보고서에 계산 근거를 넣고 각각의 실제 입력·산출물·검증 결과를 조회한다. 한 분야의 실패를 만들어 다른 분야의 PASS와 혼동하지 않음을 확인한다.
2. 사용 가능한 이미지 provider에서는 보고서와 생성 이미지를 함께 다루고, 미지원 환경에서는 이미지 부분을 unavailable로 남긴 채 확인된 다른 결과만 제한적으로 전달한다.
3. 진행 중 사용자 방식 변경·부분 취소 뒤 최종 답이 동일 목표와 이미 확인한 근거에 연결되고, 새 방식의 확인되지 않은 단계가 자동 PASS가 되지 않는다. 실제 CLI 또는 대화 표면에서 답 우선 화면과 상세 조회를 관측한다.

## Closure Evidence

- 보고서+계산, 제공되는 경우 보고서+이미지의 실제 사용자 transcript와 산출물별 source/verification linkage.
- 단계 실패·미지원 provider·사용자 변경·부분 취소의 상태 분리 테스트 및 상세 기록 접근 확인.

PRD 040은 앞선 독립 기능들의 사용자 결과를 한 요청으로 결합해 닫는다. 이후 번호의 기능·학습·출시 인증을 요구하지 않는다.

## 구현 메모 및 검증 근거 (2026-10-22)

아래는 구현·실행 기록이다. 완료 판정은 독립 Oracle gate 후 종료 확인 절에 기록한다.

- **복합 요청 지원(`task_method_classify.rs`, `task_method_compose.rs`):** 계산·보고서·이미지 중 둘 이상의 의도가 있으면 `composed` 방식으로 선택하고 부분 목록(`Selection.components`)을 남긴다. PRD000에서 막던 복합 요청을 이제 실행한다.
  - 가용성은 부분별로 판단한다. 필요한 도구가 없는 부분은 `unavailable`로 남기고, 한 부분이라도 쓸 수 있으면 요청을 진행한다. 모든 부분을 쓸 수 없을 때만 provider 호출 전에 막는다.
  - 방식 표시는 `composed [calculation + report]`, 쓸 수 없는 부분은 "unavailable parts: image"로 보여 준다.
- **부분별 판정과 결합(`task_method_apply.rs::refresh`):** 같은 관측 기록에서 계산 실행, 보고서 출처·출력, 이미지 호출을 각 부분의 증거로 넣고, 각 부분을 PRD010/020/030 규칙 그대로 따로 판정한다.
  - 확인 항목은 부분 이름을 붙여 합친다(예: `calculation: independent check`, `report: claim-source linkage`). 쓸 수 없는 부분의 확인 항목은 `not verified`이며 근거에 "unavailable: <도구> is not registered"를 남긴다.
  - 한 부분의 판정은 다른 부분의 판정을 바꾸지 않는다. 계산이 `conflict`이면 보고서가 출처와 연결돼 있어도 계산 결과를 인용한 줄은 연결되지 않는다.
- **보고서에서 계산 결과(`[calculation]`):** 보고서의 `[calculation]` 줄은 `[image]`처럼 runtime이 채우는 자리다(Oracle 3차 반영). 그 줄의 모델 문장은 버리고, 계산이 `verified`면 "[Calculation: <값> <단위> — verified by a separate check run]", 아니면 "[Calculation: none — the calculation part is <상태>]"로 바꾼다(`task_method_calc_render.rs::calc_line`). 이 줄은 보고서 확인 대상에서 빠진다. 파일로 쓴 보고서의 `[calculation]` 줄은 runtime이 바꿔 쓸 수 없으므로 확인하지 않은 인용(`ResultUnchecked`)으로 남고, 계산 부분이 없으면 `no such part in this request`다. (처음 구현과 2차 반영에서는 줄의 문장을 읽어 값이 맞는지 판단했다. 아래 3차 절 참조.)
- **이미지 자리(`[image]`):** 보고서의 `[image]` 줄은 이미지가 들어갈 자리다. PRD030 Oracle 4차 판단(확인하지 않은 내용 주장을 사용자에게 보이지 않는다)을 복합 요청에도 적용했다.
  - 그 줄의 모델 문장은 버리고 runtime 줄로 바꾼다. 저장됐으면 "[Image: <참조> — saved and checked on disk; content and look not checked]", 아니면 "[Image: none — the image part is <상태>]"다.
  - 이 줄은 보고서 확인 대상(`report_outputs`)에서도 뺀다. 그래서 모델 문장이 저장된 판정이나 `/method status`에 남지 않는다.
- **답 우선 표면(`task_method_compose_answer.rs`):** 모델 답변은 runtime이 줄마다 확인할 수 있을 때만 보여 준다.
  - 보고서 부분이 있으면 모델 보고서를 보여 주고, 확인되지 않은 줄 끝마다 `[runtime: …]`를 붙인다(출처 표시 없음, 연결되지 않은 인용과 그 이유, 확인 한도 밖). 앞에 경고만 붙이면 뒤의 주장이 그대로 남는다는 PRD030 4차 지적을 줄 단위로 막는다.
  - 확인되지 않은 부분이 있으면 답 맨 앞에 "Not every part of this request is confirmed (부분 — 상태; …)"를 붙인다.
  - 보고서 부분이 없으면(계산 + 이미지) 줄마다 확인할 수 없으므로 이미지 방식처럼 모델 답변 대신 runtime 기록("Parts (each checked on its own): …" + 이미지 결과)을 보여 주고, 원문은 `withheld_reply`에만 남긴다.
  - 그 다음 "Parts (each checked on its own): …" 한 줄과 부분별 요약을 보여 준다. 사용자 수용 줄은 한 번만 보여 준다. 코드·실행 출력·출처 digest·이미지 호출 상세는 `/method status`에서 본다.
  - 대화 기록의 마지막 답 자리에는 사용자에게 보인 답을 저장하고, 도구 호출과 함께 온 문장과 reasoning은 지운다. 복합 요청은 모델 답변을 진행 중 text delta로 내보내지 않는다.
- **방식 변경과 부분 취소:** 진행 중 `/method use report`처럼 한 부분만 남기면 PRD000 변경 규칙대로 보류 중인 승인·질문을 취소하고 같은 목표(goal)로 이어간다.
  - 새 방식에도 남는 부분은 이미 만든 결과를 유지하고 새 판에서 다시 판정한다(Scope 2). 보고서는 이미 읽은 자료를 이월 자료(`read before the method change`)로 가져와 다시 확인한다(작업 폴더 파일이 바뀌지 않았을 때만 연결). 계산은 앞서 기록된 실행을, 이미지는 앞서 저장된 이미지를 다시 판정한다(`CalculationEvidence.since`, `ImageEvidence.since`: 그 부분이 이어진 첫 판부터의 기록만 센다).
  - 빠지거나 새로 더해진 부분은 그 판부터 다시 시작한다. 이전 판의 확인 항목은 물려받지 않는다(`superseded_not_inherited`).
- **runtime 표시 보호:** `[runtime: …]`는 runtime만 붙인다. 모델이 쓴 `[runtime: …]`는 화면에서 지우고, 보고서 확인에서도 표시로 취급하지 않는다.
- **workflow·도구 제한:** PRD030 Oracle 5·6차 판단을 복합 요청에도 적용했다. 줄 단위 확인을 거치지 않는 모델 문장이 답 밖으로 나가지 않게 하려는 것이다.
  - 자동·metadata workflow로 보내지 않고, 사용자 turn에 대기 중인 workflow는 이어 실행하지 않고 durable 취소(`composed_request`)로 기록한다.
  - 모델에게 `ask_user`, `message`, `spawn`, `propose_entrusted_work`를 주지 않는다(`agent_loop_method_image.rs::method_tools`). 다른 도구는 그대로이며, 이미지 방식은 계속 `image_generate`만 받는다.
  - 진행 출력에서 text·reasoning delta와 도구 호출 입력을 내보내지 않고, 주지 않은 도구 호출의 진행 이벤트도 내보내지 않는다(`hold_model_text`). runtime hook에도 조용한 사본만 넘기고(PRD030 7차의 `Quiet`), 복구용 checkpoint와 대화 기록에서 모델 문장·reasoning을 뺀다(PRD030 8차). 이미지 부분이 없는 복합 요청도 같다. 그렇지 않으면 runtime 표시가 붙기 전의 모델 문장이 `--verbose` 로그나 복구된 기록에 표시 없이 남기 때문이다. 3차 반영 뒤에는 도구 호출 인자도 진행 이벤트와 진단용 hook에는 파일·주소와 digest만 넘긴다. 4차 반영 뒤에는 도구 결과도 진행 이벤트·hook 문맥·저장 기록·checkpoint에 길이·digest만 남기고, 모델에게는 같은 요청 안에서만 원래 결과를 되돌린다(`quiet_tools.rs`, `agent_loop_method_hold.rs`). 실행·권한 판정·승인 화면·plugin `tool:before` 정책 hook·method 기록은 원래 인자와 결과를 받는다. 8차 반영 뒤에는 provider의 스트리밍 호출 시작도 제공된 도구만 digest ID로 보내고, 제공되지 않은 도구를 부른 호출은 hook·대화 기록·checkpoint에 `unoffered_tool`이라는 이름으로만 남긴다. 9차 반영 뒤에는 Anthropic thinking block을 대화 기록에서 빼고, provider API 오류의 글은 답·기록에 길이·digest로만 남긴다. 10차 반영 뒤에는 provider에 thinking을 요청하지 않으므로 thinking block을 보관하거나 되돌리지 않는다(9차의 보관은 지움). provider가 오류를 응답으로 알린 경우에도 digest만 남기고, 방식 변경 뒤 다른 방식으로 이어간 실행의 hook에도 보관했던 도구 결과는 digest로만 넘긴다. 11차 반영 뒤에는 hook에 넘기는 응답과 호출 사본을 허용한 필드만으로 새로 만들어, provider 고유 필드(호출의 확장 필드, 응답의 오류 문자열)도 넘기지 않는다. 12차 반영 뒤에는 종료 이유와 사용량도 runtime이 아는 값만 넘기고(provider 종료 이벤트 포함), 방식 변경 뒤 다른 방식의 실행 hook에도 이미지·복합 단계에서 저장된 호출은 같은 digest 사본으로 넘긴다. 13차 반영 뒤에는 실행 기록(`runtime_execution`)도 도구 호출을 ID의 SHA-256 digest로 부르고, provider 이미지 진행 이벤트의 후보 ID도 digest로 보낸다. 14차 반영 뒤에는 실행 기록에 모델의 질문도 digest로만 두고, 방식 변경으로 거친 중간 단계에서도, 다시 이미지·복합 방식이 될 때 남아 있던 기록에도 같은 규칙을 쓴다.
- **모델 문맥:** 복합 요청의 projection에는 부분별 protocol과 함께 "각 부분은 따로 확인되며 한 부분의 결과가 다른 부분을 검증하지 않는다", `[calculation]`·`[image]` 규칙, 쓸 수 없는 부분은 다른 방법으로 만들지 말고 알리라는 규칙을 넣는다.
- **기존 기대 변경:** PRD000의 "둘 이상의 전문 의도는 unsupported로 막는다" 단위 테스트를 "복합으로 선택하고 모든 부분을 쓸 수 없을 때만 막는다"로 바꿨다. PRD040 요구에 따른 의도적 변경이다.
- **구조:** PRD030의 이미지 답 교체(`guarded_answer`)를 `task_method_image_answer.rs`로 옮기고, 복합 요청 답과 같은 runtime 문장을 쓴다. 이미지 turn 처리(`agent_loop_method_image.rs`)는 이미지 부분이 있는 복합 요청까지 다룬다.
- **테스트(새 20개):**
  - AgentLoop 17개(`crates/shacs-core/tests/it/spec037_method_selection/composed*.rs`): 보고서+계산의 부분별 판정과 연결, 계산 `conflict`가 보고서의 연결을 가리지 않음, 확인된 계산과 다른 값을 적은 줄, 모델이 쓴 `[runtime: …]` 제거, 모든 부분을 쓸 수 없으면 차단, 쓸 수 없는 이미지 부분과 보고서만 전달, 저장된 이미지가 `[image]` 줄을 대신함, 보고서 없는 복합 요청의 답 보류, 진행 출력 보류, 긴 "review" 요청의 workflow 제외, 방식 변경 뒤 보고서 자료·계산 실행·저장 이미지 유지, 말하는 도구 제외, hook의 조용한 사본, 복구용 checkpoint, 이미지 부분이 없는 복합 요청의 hook·기록.
  - 단위 3개: 값 비교(`states_value`), 이미지 자리 판별(`is_image_line`), 대기 중 workflow의 `composed_request` 취소.
  - 대상 실행: `targeted-spec037.log` 94/94, `targeted-lib.log` 49/49, `targeted-cli-command.log`(이미지 CLI 2/2·verbose 1/1·REPL 4/4·command 3/3). `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`).
- **실제 표면(`.omo/evidence/spec037/prd040/`, `qa-setup.py` + `qa.sh`로 재현):**
  - `qa-docker-composed.txt`: 공식 container와 같은 조건(`SHACS_RUNTIME_PACKAGE=shacs-bot-official-container`, 실제 저장소를 Docker에서 빌드, `docker-build.log`)에서 compiled REPL로 확인했다. container 안의 결정론적 호환 이미지 API를 쓴다. 장면은 다음과 같다.
    - 보고서+계산 확인(승인 후 실제 `exec` 두 번, `verified`·`source-linked`, `/method status`의 실행 코드·출력·자료 digest·결론별 연결).
    - 계산 `conflict`(600 vs 6.0)와 그 값을 인용한 줄의 `[runtime: not backed …]`.
    - 보고서+이미지(실제 PNG 저장, `[image]` 줄이 runtime 줄로 바뀜, 모델의 "blue" 문장은 어디에도 없음).
    - 계산+이미지(답 보류, "Verified answer: 6 m^2").
    - 보고서+이미지 도중 `/method use report`로 이미지 부분 취소(보류 승인 취소, 이월 자료 재확인), `/method history`.
  - `qa-native-composed-gate.txt`: macOS native에서는 Spec030 process gate가 승인 뒤 `exec`를 막아 계산 부분이 `unavailable`이 되고, 계산을 인용한 줄은 연결되지 않는다.
  - `qa-native-composed-no-image.txt`: 이미지 생성이 꺼진 환경에서 이미지 부분은 `unavailable`, `[image]` 줄은 "[Image: none — the image part is unavailable]"다. 읽은 파일을 그대로 옮긴 줄만 보이고, 나머지 줄(열린 점, 출처 없는 줄, 출처에 연결됐지만 인용이 아닌 "The artwork is blue" 줄)은 "[runtime: line withheld — …]"로 바뀌며 요약과 `/method status`에는 수와 "(withheld)"만 나온다(Oracle 2차 반영 후 다시 만든 기록).
  - `qa-docker-history-check.txt`(`../prd030/history-check.py`): docker session의 저장 기록에 "blue"가 어디에도 없고, 보류한 답은 metadata(`withheld_reply`)에만 있다. `qa-docker-artifacts.txt`는 저장된 PNG를 `file`·`shasum`으로 따로 확인한 결과, `fake-image-api-requests.log`는 provider가 받은 요청이다.
- **전체 검증(Oracle 1차 검토 전 코드):** `cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast`를 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle1.log`, `count-tests.py`, 1,459초).
  - suite 327개 기준 **3,267 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD030 종료 시점(실행 중 외부 요인으로 실패한 테스트를 통과로 친 3,247) 대비 +20이다. PRD040의 새 테스트 20개와 일치한다.

### Oracle 1차 검토 반영 (BLOCKED → 수정)

첫 번째 독립 Oracle gate가 BLOCKED로 판정한 네 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle1-fix-before-agentloop.log`: 4개 실패). 수정 후 결과는 `oracle1-fix-after-agentloop.log`(98/98), `oracle1-fix-after-unit.log`(49/49, 이후 이미지 단어 판별 단위 테스트 1개를 더함), `oracle1-fix-after-cli.log`(이미지 CLI 2/2·verbose 1/1·REPL 4/4·command 3/3)다.

- **이미지에 관한 모델 문장이 표시만 붙은 채 보임:** `[image]` 줄만 바꾸고 나머지 줄은 `[runtime: …]` 표시를 붙여 보여 줬다. 실제 기록(`qa-native-composed-no-image.txt`)에도 "The chart is attached below."가 보였다.
  - 이제 이미지 부분이 있는 복합 요청에서는 확인된 부분이 뒷받침하는 줄, 열린 점, 제목·안내 줄만 보여 주고, 이미지에 대해 말하는 줄은 연결돼 있어도 감춘다(`task_method_compose.rs::withholds`, `mentions_image`, `task_method_compose_answer.rs`). 감춘 줄은 "[runtime: line withheld — …]"로 바꾸고 답 앞에 알린다. (낱말 목록으로 가려내는 이 방식은 2차 검토에서 인용 규칙으로 바꿨다. 아래 2차 절 참조.)
  - 요약("Not backed", 출처 없는 줄)과 `/method status`(결론 목록)도 그런 줄의 문장을 인용하지 않는다(`task_method_report_render.rs`, 새 `task_method_report_details.rs`).
- **복합 요청의 도구 실행 거부가 부분 기록에 남지 않음:** `record_denied_call`이 단일 방식만 처리했다. 이제 요청의 부분에 따라 `exec`·자료 읽기·`image_generate` 거부를 각 부분에 기록한다. 저장된 이미지 뒤의 새 생성을 거부하면 이미지 부분은 `not generated`가 된다.
- **빠졌다 다시 더해진 보고서 부분이 예전 자료를 되살림:** 방식 변경에서 보고서가 유지되지 않으면 이월 표시를 지운다(`ReportEvidence::drop_carried`).
- **`/method accept`가 첫 부분의 상태만 기록:** 복합 요청에서는 부분별 상태 전체를 응답과 수용 기록에 남긴다.
- **새 테스트(5개):** AgentLoop 4개(`composed_gate.rs`: 위 네 항목), 단위 1개(이미지 단어 판별, 2차 검토에서 낱말 목록과 함께 지움). 기존 기대 1개(`composed_change.rs`의 진행 출력 보류 테스트)를 새 동작에 맞췄다.
- **실제 표면 재확인:** `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `fake-image-api-requests.log`를 1차 반영 코드로 다시 만들었다(Docker 빌드 `docker-build.log` 포함). docker 기록의 장면과 결과는 이전과 같고, 이미지가 없는 환경 기록은 위처럼 그림 관련 줄을 감춘다.
- **전체 검증(Oracle 1차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle2.log`, `count-tests.py`, 1,360초).
  - suite 327개 기준 **3,272 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 1차 검토 전 3,267 대비 +5이며, 새 테스트 5개와 일치한다.


### Oracle 2차 검토 반영 (BLOCKED → 수정)

두 번째 독립 Oracle gate가 BLOCKED로 판정한 네 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle2-fix-before-agentloop.log`: 5개 실패, `oracle2-fix-before-unit.log`: 1개 실패).

- **이미지에 관한 문장을 낱말 목록으로 가려내 빠져나감:** 1차의 낱말 목록(`mentions_image`)에 없는 말로 그림 이야기를 하면("The artwork is blue [source: notes.md]") 출처에 연결된 줄로 그대로 보였다.
  - 낱말 목록을 지우고 인용 규칙으로 바꿨다(새 `task_method_compose_quote.rs`). 이미지 부분이 있는 복합 요청에서는 이 요청에서 읽은 파일의 문장을 그대로 옮긴 줄만 보여 준다. (부분 문자열 비교였던 이 판정은 3차에서 문장·줄 단위 비교로 바꿨다.)
  - 판정은 refresh 때마다 한다(`task_method_apply.rs::refresh` → `mark_quotes`). 인용한 파일을 다시 읽어 읽을 때의 file digest와 같고(8 MiB 이하), 줄의 문장이 그 안에 그대로 있을 때만 결론에 `quoted`를 표시한다(`ReportClaim.quoted`). 대소문자·공백·끝 문장부호는 무시한다. 웹 자료는 인용으로 치지 않는다.
  - 나머지 줄(제목·안내 줄 포함)은 "[runtime: line withheld — not a word-for-word quote of a file read in this request]"로 바꾼다. 요약과 `/method status`에는 수와 "(withheld)"만 나온다(`task_method_report_render.rs`, `task_method_report_details.rs`). 모델 문맥에도 이 규칙을 알린다.
- **`[calculation]` 연결이 다른 수나 부정을 함께 적은 줄도 받아들임:** "The result is not 6; it is 600 [calculation]"처럼 확인된 값을 부정하거나 다른 수를 함께 적은 줄도 연결됐다.
  - `states_value`를 엄격하게 바꿨다. 줄의 수가 모두 확인된 값이어야 하고, 수가 없거나 부정하는 말(영어 낱말과 한국어 "아니·않·없·못")이 있으면 연결하지 않는다. 그때 이유는 `does not plainly state only the verified result`다. (이 문장 판정은 3차에서 runtime이 값을 직접 적는 방식으로 바꾸면서 지웠다.)
- **빠졌다 다시 더해진 보고서 부분이 예전 자료를 되살림:** 1차의 `drop_carried`는 보고서가 빠질 때 이월 표시만 지웠다. 그 뒤 보고서가 다시 더해지고(`/method auto`) 보고서만 남기면(`/method use report`) `carry_sources`가 그 요청에서 읽은 모든 자료를 다시 이월했다.
  - 이제 `carry_sources(revision)`은 떠나는 판에서 쓰던 자료(이미 이월됐거나 그 판에서 읽은 자료)만 이월한다. `/method revise`의 `next_version`도 이전 판의 마지막 판정에 쓰인 자료만 가져온다.
- **방식 변경으로 취소한 보류 호출이 남는 부분의 기록에 없음:** 남는 계산 부분의 보류 실행이나 남는 이미지 부분의 보류 생성이 취소돼도 기록되지 않았다. 그래서 앞의 `verified`·`generated`가 그대로 남았다.
  - `cancel_method_pending`이 취소한 호출을 `record_denied`로 남는 부분에 기록한다(내용 "Error: tool call cancelled before it ran (the method changed).").
  - 계산 부분이면 마지막 주 계산 뒤의 취소된 실행이므로 `not verified`이고, 주 계산과 확인 계산을 다시 실행해야 돌아온다. 이유 문구는 취소만 있을 때 "a run after the latest primary result was cancelled"로 구별한다(PRD010의 실패 문구는 그대로).
  - 이미지 부분이면 앞서 저장된 이미지는 더 이상 현재 결과가 아니다.

같은 인용 규칙에서 남은 틈도 함께 막았다. 아래 앞의 두 항목은 수정 전 실패를 `oracle2-hardening-before.log`로 확인했다(2개 실패).

- 요청이 시작된 뒤 바뀐 파일은 인용으로 치지 않는다(새 `MethodRequest.started_at`과 파일 수정 시각을 비교). 모델이 자기 문장을 파일로 써 두고 읽어서 인용하는 일을 막는다. 이전 요청에서 만든 파일이나 수정 시각을 되돌린 파일은 가려내지 못하며, USAGE에 그렇게 적었다.
- 보여 주는 줄은 확인한 인용 문장과 `[source: …]`(확인된 계산에 연결됐으면 `[calculation]`)만으로 다시 만든다. 같은 줄의 `[unresolved: …]` 같은 표시에 든 모델 말은 지운다.
- 저장 한도(256자)에 걸려 잘렸을 수 있는 줄과 비밀값이 가려진 줄은 인용으로 치지 않는다. 앞의 목록 기호와 앞뒤 따옴표는 무시한다.
- 값이 수가 아닌 계산 결과(`1/3`)는 그대로 적혀 있고 다른 수가 없어야 연결된다.

검증 기록은 다음과 같다.

- **테스트:** 새 테스트 11개를 더하고 1개를 지웠다(순증 10개).
  - 새 AgentLoop 8개: `composed_gate2.rs` 4개(위 네 항목), `composed.rs` 1개(다른 수·부정), `composed_quote.rs` 3개(이 요청에서 쓴 파일, 다시 만든 인용 줄, 계산·보고서·이미지 세 부분 요청에서 인용 줄이 확인된 계산에 연결된 채 보임).
  - 새 단위 3개: 이월 자료(`a_new_version_carries_only_sources_usable_in_the_last_judged_revision`)와 인용 판별 2개. 지운 1개는 낱말 판별 단위 테스트다.
  - 기존 테스트 보강: 계산 재실행 단위 테스트에 취소된 실행과 "확인 계산만 다시 성공" 경우를, 값 비교 단위 테스트에 다른 수·부정·수가 아닌 값을 더했다.
  - 기존 기대 3개를 새 동작에 맞췄다: `composed_change.rs`(감춘 줄 문구), `composed_gate.rs`(인용 규칙, 이름도 바꿈), `composed_keep.rs`(방식 변경 뒤 주 계산과 확인 계산 재실행).
  - 결과: `oracle2-fix-after-agentloop.log` 106/106, `oracle2-fix-after-unit.log` 52/52, `oracle2-fix-after-cli.log`(이미지 CLI 2/2·verbose 1/1·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`).
- **실제 표면 재확인:** Docker 빌드(`docker-build.log`)부터 `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `fake-image-api-requests.log`를 다시 만들었다.
  - `qa-docker-composed.txt` 끝에 새 장면을 더했다. 보고서+계산에서 확인 계산이 승인을 기다릴 때 `/method use calculation`을 보낸다. 취소된 확인 실행이 기록되고(`#2 c8 [no result line] cancelled [revision 1; kept across the method change and judged again]`), 확인 계산만 다시 성공해도 "not verified — a run after the latest primary result was cancelled; rerun the primary and the check"로 남는다.
  - 보고서+이미지 장면은 container에서도 인용 줄 "The room is 3 m by 200 cm [source: notes.md]"를 보여 준다(mount된 파일의 수정 시각으로도 판별이 동작). 저장 기록 어디에도 "blue"가 없다.
  - `qa-native-composed-no-image.txt`에는 출처에 연결됐지만 인용이 아닌 줄("The artwork is blue [source: notes.md]")을 더했다. 답·요약·`/method status` 어디에도 그 문장이 없다.
- **전체 검증(Oracle 2차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle3.log`, `count-tests.py`, 1,459초).
  - suite 327개 기준 **3,282 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 1차 반영 후 3,272 대비 +10이며, 순증 테스트 10개와 일치한다.
  - 코드 diff는 `oracle2-fix.patch`(2차 검토 시점 대비)다. 전체 diff `prd040.patch`는 3차 반영 뒤 다시 만들었다.

### Oracle 3차 검토 반영 (BLOCKED → 수정)

세 번째 독립 Oracle gate가 BLOCKED로 판정한 네 항목을 고쳤다. 새 테스트를 3차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle3-fix-before-agentloop.log`: 5개 실패, `oracle3-fix-before-cli.log`: 1개 실패).

- **표시만 있는 줄이 이미지 부분이 있는 답에 그대로 보임:** 줄에서 표시를 떼어 내면 남는 글이 없어 서식 줄(layout)로 처리됐다. 그래서 "[unresolved: the artwork is blue]" 같은 줄이 그대로 보였다.
  - 이제 이미지 부분이 있으면 표시가 있는 줄은 서식 줄로 통과시키지 않는다. 글자가 없는 구분선과 빈 줄만 그대로 둔다(`task_method_compose_answer.rs::checked_lines`).
- **인용 판정이 부분 문자열 비교라 거짓 인용을 받아들임:** 파일의 "Revenue grew 100%."가 모델의 "Revenue grew 10"을 인용으로 통과시켰다.
  - 판정을 새 `task_method_quote_match.rs`로 옮기고 문장·줄 단위로 바꿨다. 줄의 문장은 파일의 문장 전체나 줄 전체와 글자 그대로 같아야 한다(대소문자 포함). 시작은 블록(문단·목록·제목·인용·표)의 시작이나 문장의 시작, 끝은 문장의 끝(`.` `?` `!`)이나 블록의 끝이어야 한다.
  - 공백의 수, 앞의 목록 기호와 따옴표, 끝의 `,` `;` `:`, 마지막 마침표만 달라도 된다. `?` `!`는 빠지면 안 된다. 여러 줄로 나눈 문단의 줄 끝은 문장의 끝으로 보지 않는다. "U.S." 같은 약어 뒤 소문자로 시작하는 줄은 문장의 시작으로 보지 않는다.
  - 그래서 숫자·단어의 일부(10/100%, true/untrue, 부호, 소수점, 지수, 천 단위 쉼표)를 자른 인용과 뜻을 바꾸는 말을 뺀 문장 일부는 감춘다. 인용한 파일은 refresh당 한 번만 읽는다.
  - 남은 한계는 USAGE에 적었다. 약어 뒤 대문자로 시작하는 인용은 약어 앞부분이 빠져도 통과할 수 있다.
- **`[calculation]` 연결이 문장의 뜻을 판단하지 못함:** 부정어 목록에 없는 말("The result differs from 6")과 `f64`로 같아지는 큰 수가 확인된 값으로 연결됐다.
  - Oracle이 권고한 방식대로 `[calculation]`을 runtime이 채우는 자리로 바꿨다. 그 줄의 모델 문장은 버리고, 확인된 값과 단위를 runtime이 직접 적는다(`task_method_calc_render.rs::calc_line`). 확인되지 않았으면 그 상태를 적는다.
  - 답의 `[calculation]` 줄은 보고서 확인 대상에서 빠진다(`agent_loop_method_finish.rs::finish_run`). 파일로 쓴 보고서의 `[calculation]` 줄은 runtime이 바꿔 쓸 수 없으므로 확인하지 않은 인용(`ResultUnchecked`)으로 남는다.
  - 문장으로 값을 판단하던 `states_value`·`negates`와 `ResultLinked`·`ResultMismatch`·`ResultUnverified`를 지웠다. 모델 문맥의 안내도 "값을 직접 쓰지 말고 `[calculation]` 줄을 두라"로 바꿨다.
- **진단용 hook과 진행 이벤트가 쓸 수 있는 도구의 인자를 그대로 받음:** `image_generate` 프롬프트 같은 모델 작성 인자가 `--verbose` 로그("Tool call image_generate args={\"prompt\":…}"), observability hook, plugin hook, tool 이벤트로 전달됐다.
  - 새 `quiet_arguments.rs`가 진단용 projection을 만든다. 호출이 가리키는 파일·주소(`path`, `url`)와 나머지 인자의 길이·digest만 남긴다.
  - `Quiet` hook은 context(assistant 메시지의 도구 호출 인자 포함), `before_execute_tools`, `after_response`에 이 projection만 넘긴다. tool 이벤트도 같은 projection으로 바꿔 보낸다(`hold_model_text`).
  - 정책 판정은 인자를 그대로 받아야 하므로 새 `AgentHook::block_tool_calls_quietly`로 나눴다. 원본 인자를 요청하는 hook(plugin `tool:before` 정책)은 인자를 그대로, 그렇지 않은 진단 hook은 projection만 받는다(`CompositeHook`이 hook별로 나눔). 실행, 권한 판정과 승인 화면, method 기록은 바뀌지 않는다.
  - 이미지 방식에도 같이 적용되므로 PRD030 문서의 남은 제한(프롬프트가 tool 이벤트에 보임)에 이후 변경을 덧붙였다.
- **Oracle 비차단 지적:** PRD030 문서 끝의 빈 줄을 지웠다(`git diff --check` 통과).

검증 기록은 다음과 같다.

- **테스트:**
  - AgentLoop 테스트는 5개를 새로 쓰고 3개를 지웠다(순증 2개). 새 테스트는 `composed_quiet.rs`(진단 hook·tool 이벤트에는 인자가 보이지 않고 정책 hook은 원래 인자로 막음), `composed_quote.rs` 2개(세 부분 요청의 계산 줄, 표시만 있는 줄과 숫자를 자른 인용), `composed.rs` 2개(어떤 문장이든 runtime이 확인된 값을 적음, 파일 보고서의 계산 줄은 확인하지 않음)다. 지운 테스트는 문장으로 계산 연결을 판단하던 3개(`composed.rs`의 값이 다른 줄·다른 수와 부정, `composed_quote.rs`의 인용 줄이 계산에 연결됨)다.
  - 새 CLI 테스트 1개: `spec037_image_verbose.rs::verbose_runtime_logs_show_no_words_of_an_image_call`(gateway `run --verbose`에서 "Tool call image_generate args="는 남고 프롬프트는 없음).
  - 단위 테스트는 4개를 새로 쓰고 3개를 지웠다(순증 1개). 새 테스트는 `task_method_quote_match` 2개(문장·줄 단위 인용, 숫자·단어 경계), `task_method_compose_quote` 1개(요청 시작 뒤·읽은 뒤 바뀐 파일), `quiet_arguments` 1개다. 지운 테스트는 값 비교 1개와 부분 문자열 인용 2개다. 이미지·계산 자리 판별 단위 테스트에는 `[calculation]` 경우를 더했다.
  - 기존 기대 변경: `composed.rs`(계산 줄), `composed_change.rs`·`composed_gate.rs`·`composed_gate2.rs`·`composed_image.rs`·`composed_quote.rs`(인용 줄을 문장 전체로).
  - 결과: `oracle3-fix-after-agentloop.log` 108/108, `oracle3-fix-after-unit.log` 53/53, `oracle3-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`).
- **실제 표면 재확인:** Docker 빌드(`docker-build.log`)부터 `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `fake-image-api-requests.log`를 다시 만들었다.
  - `qa-docker-composed.txt`: 보고서+계산의 계산 줄은 "[Calculation: 6 m^2 — verified by a separate check run]", 계산이 맞지 않으면 "[Calculation: none — the calculation part is in conflict]"이다. 모델이 쓴 "600 m^2" 문장은 보이지 않는다. 보고서+이미지의 인용 줄과 마지막 장면(취소된 확인 실행)은 이전과 같다.
  - `qa-native-composed-gate.txt`: native process gate로 계산이 `unavailable`이면 계산 줄은 "[Calculation: none — the calculation part is unavailable]"이다.
  - `qa-native-composed-no-image.txt`: 문장 일부만 옮긴 "The room is 3 m [source: notes.md]"와 표시만 있는 "[unresolved: the artwork is blue]" 줄도 감춘다. 답·요약·`/method status` 어디에도 "artwork"가 없다.
- **전체 검증(Oracle 3차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle4.log`, `count-tests.py`, 1,479초).
  - suite 327개 기준 **3,286 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 2차 반영 후 3,282 대비 +4이며, 순증 테스트(AgentLoop 2, CLI 1, 단위 1)와 일치한다.
  - 코드 diff는 `oracle3-fix.patch`(3차 검토 시점 대비)다. 전체 diff `prd040.patch`는 4차 반영 뒤 다시 만들었다.

### Oracle 4차 검토 반영 (BLOCKED → 수정)

네 번째 독립 Oracle gate가 BLOCKED로 판정한 세 항목을 고쳤다. 새 테스트를 4차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다. `oracle4-fix-before-agentloop.log`는 1개 실패로, 저장 기록에 "1| The artwork is blue."가 남았다. 이 실행에서는 새 구조(보관 metadata)에 대한 두 단정을 빼고 노출만 검사했다. `oracle4-fix-before-unit.log`는 2개 실패다.

- **도구 결과에 실린 모델 문장이 여러 표면으로 나감:** 모델이 쓴 명령의 출력이나 모델이 쓴 파일을 읽은 결과가 진행 이벤트(`detail`·`result`), hook 문맥의 tool 메시지, 저장되는 대화 기록, 복구용 checkpoint에 그대로 남았다.
  - `quiet_arguments.rs`를 `quiet_tools.rs`로 넓혔다. 도구 결과는 길이·digest만 남긴다(`quiet_text`; 오류였으면 `Error: ` 표시는 둔다). tool 메시지(`quiet_tool_message`)와 진행 이벤트(`quiet_event`, 승인 대기의 runtime 문구는 그대로)에 이를 적용한다.
  - 적용하는 곳은 다음과 같다. `Quiet` hook 문맥, `hold_model_text`의 진행 이벤트, 승인 뒤 실행 경로의 진행 이벤트(`quiet_events`, 그동안 원래 callback으로 나가던 경로), checkpoint의 `completed_tool_results`, 저장되는 대화 기록(`hold_results`)이다.
  - 모델은 같은 요청 안에서는 원래 결과를 봐야 한다(승인 뒤 이어 실행). 그래서 저장 기록에서 뺀 원래 결과를 요청이 열린 동안만 session metadata `task_method_held_results_v1`에 두고, 다음 실행의 모델 문맥에 되돌린다(`restore_results`, 방식을 바꿔도 같은 요청이면 되돌림). 요청이 끝나면 지운다(`release_results`). method 관측기는 hook 바깥에서 원래 결과를 그대로 받는다.
  - 이 코드는 새 `agent_loop_method_hold.rs`로 옮겼다(이미지 처리 파일이 250줄에 닿아서).
  - 남은 영향은 USAGE에 적었다. 요청이 끝난 뒤 다음 요청의 모델은 이전 도구 결과의 digest만 보고, checkpoint 복구 뒤에도 그렇다. `session export --yes`에는 그때 열려 있는 요청의 보관 결과가 포함된다.
- **약어 끝에서 문장 일부가 문장 전체 인용으로 통과함:** `Folded::new("U.S. revenue fell.").quotes("U.S.")`가 참이었다.
  - 이제 문장 끝 표시 뒤에 소문자로 시작하는 낱말이 오면 문장의 끝으로 보지 않는다. 다만 블록 경계이면 끝으로 본다(`task_method_quote_match.rs::ends`). 마침표를 뺀 인용 경로도 같다.
- **`[calculation]` runtime 줄에 줄바꿈으로 모델 문장을 넣을 수 있음:** 두 실행이 같은 `answer: "6\nThe artwork is blue"`를 출력하면 `verified`가 되어 여러 줄로 답에 들어갔다.
  - 결과 줄의 `answer`는 수(부호, `,`/`_` 묶음, 소수, 지수, 분수 `a/b`)여야 하고, `unit`은 공백·제어 문자·대괄호가 없는 24자 이하 한 낱말이어야 한다. 아니면 결과 줄 문제로 기록한다(`task_method_calc_parse.rs`).
  - 기록하는 한 줄 글은 모두 줄바꿈과 제어 문자를 공백으로 바꾼다(`bounded`). 실행 코드·출력과 파일 경로만 줄을 그대로 둔다(`bounded_text`). 그래서 입력·가정·위치·오류 같은 글도 화면에 줄을 더하지 못한다.
  - 이미지 부분이 있는 복합 요청의 답과 요약에는 계산의 입력·가정(모델이 쓴 글)을 적지 않고 "Inputs and assumptions: recorded, shown in /method status"만 적는다. `/method status`는 확인 근거를 살펴보는 곳이라 실행 기록(코드·출력·입력·가정, 이미지 프롬프트)을 이름을 붙여 그대로 보여 준다(USAGE에 적음).
- **자체 QA에서 찾아 고친 것:** 복합 요청에서 `/method use calculation`처럼 이미지·복합이 아닌 방식으로 바꾸면 보관 결과를 되돌리지 않고 버렸다. 이제 같은 요청이면 되돌린다. `composed_keep.rs`의 계산 유지 테스트에 확인을 더했다(방식 변경 뒤 모델 문맥에는 앞서 읽은 파일 내용이 있고, 저장 기록에는 없음).
- **자체 검토에서 찾아 고친 것:** 승인 뒤 실행한 도구의 출력에 작업 폴더 위반 문구(예: "blocked by safety guard")가 들어 있으면 그 출력이 turn을 멈추는 오류로 처리되어, 출력 원문이 답과 대화 기록에 그대로 들어갔다. 모델이 쓴 명령이 그런 문구를 출력할 수 있다. 이제 이미지·복합 요청에서는 "The approved tool call stopped this turn; its output is withheld (Error: <withheld: …>)."로 바꾼다(`quiet_failure`). (처음에는 이때 요청이 열린 채로 남는다고 적었으나 틀렸다. 이 경로도 요청을 `failed`로 닫는다. 5차 절 참조.)
- **작업 환경 문제:** 작업 중 디스크가 가득 차(여유 472 MiB) `shacs-bot` 실행 파일이 3,472바이트로 잘못 링크되어 CLI 테스트가 `Permission denied`로 실패했다. 이 작업에서 만든 scratch 빌드 디렉터리와 지난 QA 디렉터리를 지우고(약 14 GiB 확보) 다시 빌드한 뒤 모두 통과했다. 코드 문제는 아니다.

검증 기록은 다음과 같다.

- **테스트:**
  - 새 AgentLoop 테스트 2개: `composed_output.rs`. 보고서+이미지 요청에서 모델이 쓴 파일 읽기와 명령 출력이 답·`/method status`·최종 저장 기록·실행 중 세션 파일(checkpoint 포함, 보관 metadata 제외)·진단 hook·진행 이벤트 어디에도 없다. 승인 뒤 이어진 실행의 모델 문맥에는 둘 다 있고, 보관 metadata는 요청이 끝나면 없어진다. 다른 하나는 승인 뒤 출력이 turn을 멈출 때 답과 대화 기록에 digest만 남는지 확인한다.
  - 새 단위 테스트 2개: `quiet_tools`(도구 결과와 진행 이벤트), `task_method_calc`(결과 줄의 수·단위 문법과 한 줄 저장).
  - 기존 테스트 보강: 인용 판별 단위 테스트에 약어 경우, `composed_keep.rs`에 방식 변경 뒤 모델 문맥을 더했다.
  - 결과: `oracle4-fix-after-agentloop.log` 110/110, `oracle4-fix-after-unit.log` 55/55, `oracle4-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`).
- **실제 표면 재확인:** Docker 빌드(`docker-build.log`)부터 `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `fake-image-api-requests.log`를 다시 만들었다. 장면과 결과는 3차와 같다. 계산+이미지 요약은 이제 "Inputs and assumptions: recorded, shown in /method status"다.
  - 새 `qa-docker-tool-outputs.txt`: docker session의 저장 기록에서 tool 메시지 17개 중 14개는 길이·digest만 남았다. 그대로 남은 셋은 runtime의 취소 문구 둘과, `/method use calculation`으로 계산 단독 요청이 된 뒤의 확인 실행 출력이다(계산 방식은 출력을 보여 주는 방식이다). 요청이 끝난 뒤 보관 metadata는 없다.
- **전체 검증(Oracle 4차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle5.log`, `count-tests.py`, 1,465초).
  - suite 327개 기준 **3,290 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 3차 반영 후 3,286 대비 +4이며, 순증 테스트(AgentLoop 2, 단위 2)와 일치한다.
  - 실행 도중 디스크 여유가 줄어(7 GiB) 이전 실행들이 남긴 임시 디렉터리를 확인했다. Spec034 release runner 테스트가 만드는 읽기 전용 toolchain 사본(각 460–691 MiB)이 정리되지 않고 남는다. 이 작업 기간(10-21 이후, 현재 실행 시작 전)에 생긴 108개를 지웠다. 기존 테스트의 정리 문제이며 PRD040 범위 밖이다.
  - 코드 diff는 `oracle4-fix.patch`(4차 검토 시점 대비)다. 전체 diff `prd040.patch`는 5차 반영 뒤 다시 만들었다.

### Oracle 5차 검토 반영 (BLOCKED → 수정)

다섯 번째 독립 Oracle gate는 4차 지적 세 항목이 고쳐졌음을 확인했다. 그리고 차단 항목 하나와 비차단 항목 셋을 냈다. 새 테스트를 5차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle5-fix-before-agentloop.log`: 3개 실패).

- **보관한 원래 도구 결과가 요청이 끝나도 남음(차단):** 보관 metadata를 `finish_run`에서만 지웠다. 그래서 요청을 닫는 다른 길에서는 원래 결과가 session metadata에 남았다. 승인 뒤 출력이 turn을 멈춘 경우(`tool_error`로 요청이 `failed`가 됨), 승인 거부, provider 오류, `/stop`, `/new`가 그런 길이다.
  - 이제 method 기록을 저장할 때마다(`MethodStore::save`) 보관한 요청이 지금 열린 요청이 아니면 보관 metadata를 지운다. 요청을 닫거나 새로 여는 모든 길이 이 저장을 거치므로 한곳에서 처리된다. `release_results`는 필요 없어져 지웠다.
  - 4차 절의 "이때 요청은 열린 채로 남는다"는 설명이 틀려 고쳤다. USAGE에는 요청이 어떻게 끝나든 지운다고 적었다.
- **package 이름이 요약에 보임(비차단):** 실행 출력의 `ModuleNotFoundError`에서 읽은 package 이름이 요약의 이유 문장에 들어갔다. 이미지 부분이 있는 복합 요청의 요약에서는 "a required Python package is not installed (see /method status)"로 바꾼다(`task_method_calc_render.rs::without_package`).
- **같은 tool call ID의 다른 결과를 되돌릴 수 있음(비차단):** provider가 ID를 다시 쓰면 다른 메시지에 원래 결과를 되돌릴 수 있었다. 이제 저장된 내용이 바로 그 결과의 digest와 같을 때만 되돌린다(`restore_results`).
- **닫는 길별 확인 부족(비차단):** 새 `composed_held.rs`가 승인 거부, `/new`, 승인 뒤 provider 오류, 방식 변경 뒤 `/stop`마다 요청이 닫히고 보관 metadata와 원래 결과가 저장된 session 어디에도 없는지 확인한다. 같은 파일에서 package 이름도 확인한다. `composed_output.rs`의 turn을 멈춘 출력 테스트는 이제 요청이 닫히고 저장된 session 전체에 원래 결과가 없는지 확인한다.

검증 기록은 다음과 같다.

- **테스트:** 새 AgentLoop 테스트 2개(`composed_held.rs`), 기존 테스트 보강 1개(`composed_output.rs`). 결과는 `oracle5-fix-after-agentloop.log` 112/112, `oracle5-fix-after-unit.log` 55/55, `oracle5-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `qa-docker-tool-outputs.txt`, `fake-image-api-requests.log`를 5차 반영 코드로 다시 만들었다. 장면과 결과는 4차와 같다(저장 기록의 tool 메시지 17개 중 14개가 digest, 요청이 끝난 뒤 보관 metadata 없음).
- **전체 검증(Oracle 5차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle6.log`, `count-tests.py`, 1,397초).
  - suite 327개 기준 **3,292 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 4차 반영 후 3,290 대비 +2이며, 새 테스트 2개와 일치한다.
  - 코드 diff는 `oracle5-fix.patch`(5차 검토 시점 대비)다. 전체 diff `prd040.patch`는 6차 반영 뒤 다시 만들었다.

### Oracle 6차 검토 반영 (BLOCKED → 수정)

여섯 번째 독립 Oracle gate는 5차 수정(요청을 닫는 모든 길에서 보관 결과 정리)을 확인했다. 요청을 닫거나 바꿀 때 지우지 않는 길이나 진행 중에 너무 일찍 지우는 길은 없었다. 다만 이미지 부분이 있는 복합 요청에서 모델 문장이 나가는 길 둘을 차단 항목으로 냈다. 새 테스트를 6차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle6-fix-before-agentloop.log`: 3개 실패). 요약에 "Error: The artwork is blue", 정책 거부 사유의 프롬프트, 모델의 질문이 그대로 나왔다.

- **도구 결과에서 나온 오류 글이 요약에 들어감:** 읽기 실패의 오류(도구 결과에서 꺼낸 글, 모델이 쓴 파일이면 모델의 문장), 이미지 호출의 오류와 그 오류를 담은 이미지 판정 이유(정책 hook의 거부 사유는 프롬프트를 담을 수 있음)가 최종 답의 요약과 모델 문맥에 들어갔다.
  - 이제 이미지 부분이 있는 복합 요청에서는 보고서 요약에 "notes.md (the error is in /method status)"(`task_method_report_render.rs`)를 적는다. 이미지 요약에는 마지막 호출의 상태만 담은 "image_generate did not save an image (<상태>); the error is in /method status"를 적고, 저장되지 않은 호출 목록에서도 오류 글을 뺀다(`task_method_image_render.rs`). 원문은 `/method status`에 남는다.
  - 이미지 단독 요청(PRD030)의 요약은 바꾸지 않았다. 그 요약의 오류는 provider·runtime 문구로 PRD030 검토에서 판단된 범위다.
- **이미지·복합 방식으로 다시 들어온 요청의 앞선 기록이 정리되지 않음:** 보관 처리는 현재 선택이 이미지·복합일 때만 했다. 그래서 복합(이미지 포함) → `/method use calculation` → `/method auto`로 다시 복합이 된 요청에는 계산 단독 단계의 원래 도구 결과가 저장 기록에 남았다. 그 단계에서 모델이 한 질문도 방식 변경 응답의 "Remaining question"에 그대로 나왔다.
  - 이제 방식 변경으로 요청이 이미지·복합 방식이 되면 그 요청이 시작된 뒤(`started_at` 이후)의 저장 기록을 run과 같은 규칙으로 정리한다(`agent_loop_method_held.rs::quiet_request_history`). assistant 메시지의 모델 문장을 지우고, 도구 결과는 보관 metadata로 옮기며 digest만 남긴다. 이미 보관한 결과(저장된 내용이 그 결과의 digest와 같음)는 다시 보관하지 않으므로 원래 결과를 digest로 덮어쓰지 않는다.
  - 이미지 부분이 있는 요청의 방식 표시(`request_text`)에서는 "User decision"과 "Remaining question"의 모델 질문을 "(the model's question is withheld)"로 바꾼다. 사용자의 답은 그대로 보인다(`task_method_render.rs`).
  - 보관 처리 코드가 250줄을 넘게 되어 `agent_loop_method_held.rs`로 나눴다(`agent_loop_method_hold.rs`에는 진행 출력·hook·checkpoint 처리만 남김).

검증 기록은 다음과 같다.

- **테스트:**
  - 새 AgentLoop 테스트 3개: `composed_errors.rs` 2개(읽기 오류, 프롬프트를 담은 정책 거부 사유)와 `composed_reentry.rs` 1개.
  - `composed_reentry.rs`는 위 재진입 장면을 그대로 재현한다. 방식 변경 응답과 이어진 답에는 그 문장이 없고, 질문은 "(the model's question is withheld)"로 보인다. 저장 session(method 기록 제외)과 그다음 실행 중의 session 파일(보관 metadata·method 기록 제외), 진단 hook 어디에도 계산 단계의 원래 출력이 없다. 모델 문맥에는 그 출력이 되돌려져 있다.
  - method 기록은 `/method status`가 보여 주는 계산 실행 기록(출력 포함, 문서에 적은 예외)을 그대로 저장하므로 검사에서 뺐다.
  - 결과는 `oracle6-fix-after-agentloop.log` 115/115, `oracle6-fix-after-unit.log` 55/55, `oracle6-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 `qa-*.txt`, `qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `qa-docker-tool-outputs.txt`, `fake-image-api-requests.log`를 6차 반영 코드로 다시 만들었다. 장면과 결과는 5차와 같다.
- **전체 검증(Oracle 6차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle7.log`, `count-tests.py`, 1,399초).
  - suite 327개 기준 **3,295 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 5차 반영 후 3,292 대비 +3이며, 새 테스트 3개와 일치한다.
  - 코드 diff는 `oracle6-fix.patch`(6차 검토 시점 대비)다. 전체 diff `prd040.patch`는 7차 반영 뒤 다시 만들었다.

### Oracle 7차 검토 반영 (BLOCKED → 수정)

일곱 번째 독립 Oracle gate는 6차 수정(요약의 오류 글, 재진입 정리)을 확인하고 차단 항목 셋을 냈다. 새 통합 테스트를 7차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle7-fix-before-agentloop.log`: 3개 실패).

- **모델이 고른 이름(경로·주소·호출 ID)이 답과 진단에 보임:** 진단 projection은 인자의 `path`·`url`과 호출 ID를 그대로 넘겼다. 요약에는 실패한 읽기의 경로와 계산 실행의 호출 ID가 들어갔다. 모델은 존재하지 않는 경로나 호출 ID에도 아무 글이나 쓸 수 있다.
  - 진단: 인자는 길이·digest만 남기고(`quiet_arguments`), 호출 ID는 같은 digest(`quiet_id`, `call-…`)로 바꾼다. 적용하는 곳은 진행 이벤트, hook의 호출 목록과 응답, hook 문맥의 assistant 호출과 tool 메시지다. 저장 기록과 checkpoint의 호출 ID는 provider가 호출과 결과를 짝지어야 하므로 그대로 둔다.
  - 답과 요약(이미지 부분이 있는 복합 요청): 인용 줄은 "[source #1]"처럼 자료 번호로 적는다(`task_method_compose_answer.rs`). 요약은 "Sources read: source #1 (names in /method status)", "Sources that could not be read: source #2 (names and errors in /method status)"(`task_method_report_render.rs`), "Execution: primary #1 exit 0; check #2 exit 0"(`task_method_calc_render.rs`)으로 적는다.
  - 방식 표시에서는 이미지 부분이 있는 요청의 "Observed written file"을 digest로 적는다(`task_method_render.rs`). 이름 원문은 `/method status` 상세에 남는다.
  - 이 때문에 이런 요청에서는 skill 사용 알림(읽은 경로로 판단)이 나오지 않는다(USAGE에 적음).
- **재진입 정리의 시작점을 못 찾으면 아무것도 정리하지 않음:** `started_at`이나 timestamp를 해석하지 못하면 정리 없이 방식이 바뀌었다(이전에 만든 요청에는 `started_at`이 없다).
  - 이제 시작점은 요청의 목표 문장(goal digest가 같은 첫 user 메시지)으로 먼저 찾는다. 목표 문장이 이전 요청에도 있으면 범위가 넓어질 뿐이다. 없으면 `started_at` 이후의 첫 메시지로 찾는다(`agent_loop_method_held.rs::request_start`).
  - 둘 다 못 찾으면 이미지·복합 방식으로의 변경을 거절하고 선택을 그대로 둔다(`reported_start`, `NOT_CLEARED`). 이 확인은 보류 승인 취소 같은 변경보다 먼저 한다.
- **같은 tool call ID의 결과가 서로 덮어씀:** 보관이 호출 ID당 결과 하나라서 나중 결과가 앞의 결과를 덮었다. 그래서 앞의 결과는 모델 문맥에 돌아가지 않았다.
  - 이제 호출 ID마다 결과 목록을 두고, 저장된 digest가 같은 결과를 제자리에 되돌린다(`hold_from`, `restore_from`). 이전 형식(결과 하나)도 목록으로 읽는다.

검증 기록은 다음과 같다.

- **테스트:**
  - 새 AgentLoop 테스트 3개: `composed_names.rs` 2개(모델이 고른 경로와 쓴 파일 경로, 계산 실행의 호출 ID), `composed_reentry.rs` 1개(같은 호출 ID의 두 결과가 재진입 뒤 모두 모델 문맥에 돌아감).
  - 새 단위 테스트 2개: `agent_loop_method_held` 2개. 하나는 시작점 판정으로, 목표 문장, 늦은 시각이 찍힌 앞선 메시지, `started_at` 없음, 잘못된 시각, timestamp 없음을 확인한다. 다른 하나는 같은 호출 ID의 결과 보관과 되돌림이다.
  - 기존 기대 변경: 인용 줄과 요약의 자료 번호(`composed_change.rs`, `composed_gate.rs`, `composed_gate2.rs`, `composed_image.rs`, `composed_output.rs`, `composed_quote.rs`, `composed_errors.rs`), 진단의 경로·호출 ID(`composed_quiet.rs`, `quiet_tools` 단위 테스트).
  - 결과는 `oracle7-fix-after-agentloop.log` 118/118, `oracle7-fix-after-unit.log` 57/57, `oracle7-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 7차 반영 코드로 다시 만들었다. 보고서+이미지 장면의 인용 줄은 "The room is 3 m by 200 cm [source #1]", 요약은 "Sources read: source #1 (names in /method status)"다. 계산+이미지 장면의 실행 줄은 "Execution: primary #1 exit 0; check #2 exit 0"이다. 이미지가 없는 보고서+계산 장면은 이전처럼 이름과 호출 ID를 보인다. 저장 기록의 tool 메시지 결과는 6차와 같다.
- **전체 검증(Oracle 7차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle8.log`, `count-tests.py`, 1,392초).
  - suite 327개 기준 **3,300 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 6차 반영 후 3,295 대비 +5이며, 새 테스트 5개(AgentLoop 3, 단위 2)와 일치한다.
  - 코드 diff는 `oracle7-fix.patch`(7차 검토 시점 대비)다. 전체 diff `prd040.patch`는 8차 반영 뒤 다시 만들었다.

### Oracle 8차 검토 반영 (BLOCKED → 수정)

여덟 번째 독립 Oracle gate는 7차 수정 세 가지(모델이 고른 이름, 재진입 시작점, 같은 호출 ID의 결과)를 확인했다. 그리고 진단 표면으로 모델 작성 문자열이 나가는 길 둘을 차단 항목으로 냈다. 새 통합 테스트를 8차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle8-fix-before-agentloop.log`: 3개 실패). provider 이벤트에 `ToolCallStart { id: "The-artwork-is-red", name: "The artwork is blue" }`가 그대로 나왔고, 저장 기록과 재진입 뒤 기록에도 그 이름이 남았다.

- **provider의 스트리밍 호출 시작 이벤트가 호출 ID와 도구 이름을 그대로 전달함:** `hold_model_text`가 `ProviderEvent::ToolCallStart`를 바꾸지 않고 보내서, 모델이 정할 수 있는 호출 ID와 이름(없는 도구 이름 포함)이 observability hook 등에 그대로 갔다.
  - 이제 제공된 도구의 호출 시작만 보내고, ID는 진행 이벤트와 같은 digest(`quiet_id`)로 바꾼다. 제공되지 않은 도구의 호출 시작은 버린다(`agent_loop_method_hold.rs`).
- **조용한 hook 사본에 제공되지 않은 도구 이름이 남음:** `Quiet`는 호출 ID와 인자만 바꾸고 이름은 그대로 두었다. 그래서 모델이 지어낸 도구 이름이 진단 hook(`after_response`, hook 문맥의 assistant 호출과 tool 메시지, `--verbose`의 "Tool call …")과 정책 hook에 그대로 갔다.
  - 이제 제공되지 않은 도구를 부른 호출은 모든 hook 사본에서 `unoffered_tool`이라는 이름으로 바뀐다(`quiet_tools.rs::rename_unoffered`, `UNOFFERED_TOOL`, `agent_loop_method_quiet.rs`). 그런 호출은 실행되지 않으므로 정책 hook도 이름과 인자 없이 받는다.
  - 같은 이유로 저장 기록(`withhold_run_text`, tool 메시지 포함)과 복구용 checkpoint(`quiet_checkpoints`: assistant 메시지, 대기 호출, 완료된 결과)에서도 이름을 바꾼다. 호출 ID는 provider가 호출과 결과를 짝짓는 데 필요하므로 대화 기록에 그대로 둔다.
  - 방식 변경으로 다시 이미지·복합 방식이 된 요청의 앞선 기록(`quiet_request_history`)에서는 등록되지 않은 이름, 즉 모델만 쓸 수 있었던 이름만 바꾼다. 등록된 도구 이름은 runtime의 목록에서 온 것이므로 그대로 둔다.
  - 테스트용 `Streaming` provider(`image_stream.rs`)는 이제 실제 provider처럼 호출마다 `ToolCallStart`를 먼저 보낸다. 그래서 그것을 쓰는 기존 테스트도 이 이벤트를 거친다.
- **자체 검토에서 찾아 고친 것:** 실행 결과의 사용한 도구 목록(`AgentLoopTurnResult.tools_used`, SDK `RunResult.tools_used`로 호출자에게 감)이 모델이 부른 이름을 그대로 담았다. 이제 같은 규칙으로 제공되지 않은 도구는 `unoffered_tool`로 바꾼다(`withhold_run_text`). 이 확인은 위 둘째 테스트에 더했으며, 8차 검토 시점 코드에서 먼저 실행한 뒤에 더한 것이라 `oracle8-fix-before-agentloop.log`에는 따로 나오지 않는다(그 테스트는 이미 다른 단정에서 실패했다).

검증 기록은 다음과 같다.

- **테스트:**
  - 새 AgentLoop 테스트 3개(`composed_unoffered.rs`)를 더했다.
    - 첫째는 스트리밍 호출 시작 이벤트에 제공된 도구 이름과 digest ID만 있는지 확인한다.
    - 둘째는 지어낸 도구 이름이 답, 사용한 도구 목록, 저장 session, 실행 중 session 파일(checkpoint 포함, 보관 metadata 제외), 진단 hook, 정책 hook, tool 이벤트, provider 이벤트 어디에도 없고 저장 기록과 사용한 도구 목록에는 `unoffered_tool`로 남는지 확인한다.
    - 셋째는 재진입 정리에서 지어낸 이름만 바뀌고 `ask_user` 같은 등록된 이름은 남는지 확인한다.
  - 새 단위 테스트 1개(`quiet_tools`: tool 메시지·assistant 호출·대기 호출 항목의 이름 변경, ID 유지, 다른 메시지는 그대로)를 더했다.
  - 결과는 `oracle8-fix-after-agentloop.log` 121/121, `oracle8-fix-after-unit.log` 58/58, `oracle8-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 8차 반영 코드로 다시 만들었다. 장면과 결과는 7차와 같다.
- **전체 검증(Oracle 8차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/workspace-test-before-oracle9.log`, `count-tests.py`, 1,390초).
  - suite 327개 기준 **3,304 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 7차 반영 후 3,300 대비 +4이며, 새 테스트 4개(AgentLoop 3, 단위 1)와 일치한다.
  - 코드 diff는 `oracle8-fix.patch`(8차 검토 시점 대비)다. 전체 diff `prd040.patch`는 9차 반영 뒤 다시 만들었다.

### Oracle 9차 검토 반영 (BLOCKED → 수정)

아홉 번째 독립 Oracle gate는 8차 수정(이벤트 ID digest, 제공되지 않은 도구 이름 바꾸기, `tools_used`, checkpoint의 호출 ID 짝, 정책 hook의 원래 인자)을 확인하고 차단 항목 셋을 냈다. 새 통합 테스트 2개를 9차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle9-fix-before-agentloop.log`: 2개 실패). 저장 기록에 thinking block과 그 문장이 남았고, provider 오류 답에 모델이 지어낸 도구 이름이 그대로 나왔다("tool_use 'The artwork is red' is not a valid tool").

- **승인 뒤 provider에 다시 보내는 assistant turn이 바뀜:** Oracle은 저장 기록에서 지운 본문과 바꾼 호출(이름·인자)이 승인 뒤 그대로 provider에 가므로 400 오류가 날 수 있다고 봤다. Anthropic은 thinking이 있는 도구 실행 중에는 assistant content 전체를 그대로 돌려받아야 한다는 이유였다. 그래서 원래 block 순서, thinking·본문, 호출 이름·ID·인자를 정확히 되돌리라고 했다.
  - 확인해 보니 바꾸지 않은 채 돌려줘야 하는 것은 thinking block이다(서명이 thinking 내용에 붙는다). 이 저장소의 Anthropic client(`assistant_blocks`)는 원래 응답의 block을 보관하지 않는다. 어느 요청에서나 assistant turn을 "thinking block들 + 본문 하나 + tool_use들(ID 정리)"로 다시 만든다. 그래서 원래 block 순서는 일반 요청에서도 되돌리지 않으며, provider가 받는 형태의 조건은 "thinking이 맨 앞에, 바뀌지 않은 채"다.
  - 본문은 되돌리지 않는다. 확인되지 않은 모델 문장을 다음 실행의 모델 문맥에도 남기지 않는 것이 PRD030 8차부터의 규칙이다. 본문이 없는 assistant turn(thinking + tool_use)도 provider가 받는 정상 형태다.
  - 제공되지 않은 호출의 이름도 되돌리지 않는다. 그 호출은 실행되지 않았고 결과 자리는 runtime 문구다. 모델이 지어낸 이름(예: "The artwork is red")은 도구 이름 형식에 맞지 않을 수 있어서, 되돌리면 오히려 요청이 거절될 수 있다. `unoffered_tool`은 형식에 맞는 이름이다.
  - 그래서 thinking block만 provider 전용으로 보관해 되돌린다. 저장 기록에서 뺀 thinking block은 그 assistant 메시지의 호출(저장된 그대로의 digest) 아래에 보관 metadata(`task_method_held_results_v1`의 `thinking`)로 둔다. 같은 요청의 다음 실행(승인 뒤 이어 실행, 방식 변경 뒤 포함)에서는 모델 문맥의 그 메시지에 되돌린다(`agent_loop_method_held.rs::hold_thinking`, `restore_thinking`). 같은 호출의 turn이 여럿이면 최근 turn부터 최근 block을 받는다. 요청이 끝나면 도구 결과와 함께 지운다(5차의 `MethodStore::save`). (이 보관과 되돌림은 10차에서 지웠다. 이제 이미지·복합 요청에서는 thinking을 요청하지 않는다. 아래 10차 절 참조.)
  - 새 테스트는 Oracle이 요구한 장면을 쓴다. 한 응답에 thinking, 본문, 제공되지 않은 호출, 승인 대상 호출(`image_generate`)이 함께 온다. 승인 뒤 provider 요청의 그 turn에는 원래 thinking이 그대로 있고, Anthropic 요청 본문으로 만들면 첫 block이 그 thinking이다. 본문은 비어 있고 호출은 `unoffered_tool`과 `image_generate`다.
- **thinking 내용이 SDK `RunResult.messages`로 나감:** 8차까지는 thinking block을 저장 기록에 남겼다(문서에 적은 예외). SDK는 저장 기록을 그대로 돌려준다.
  - 이제 thinking block은 저장 기록에 남지 않으므로(위 보관) 저장 session과 SDK `messages`에 없다. 요청이 끝난 뒤에는 보관 metadata에도 없다. 방식 변경으로 다시 이미지·복합 방식이 된 요청의 앞선 기록(`quiet_request_history`)도 같은 보관을 거친다.
  - 복구용 checkpoint에는 8차부터 thinking을 넣지 않는다. 그래서 실행 중 프로세스가 멈춘 뒤 복구하면 그 실행의 thinking은 되돌아오지 않는다(USAGE에 적음).
  - USAGE와 PRD030 문서의 예외 설명을 고쳤다.
- **provider 오류의 글이 그대로 답·기록에 들어감:** `runner.run`이 오류를 돌려주면 `ProviderError::Api`의 message가 답, 대화 기록, 내보내는 메시지, SDK 결과, 실행 기록에 그대로 들어갔다. provider는 오류 글에 모델이 고른 호출 이름이나 ID를 인용할 수 있다.
  - 이제 이미지·복합 요청에서는 답을 "Sorry, I encountered an error calling the AI model: provider API error (HTTP 400); its message is withheld (<withheld: N chars, sha256:…>)."로 바꾼다(`agent_loop_method_hold.rs::quiet_provider_error`). 대화 기록과 내보내는 메시지도 이 문구다. 실행 기록(`runtime_execution`)에서 그 turn의 provider 오류 상세에 든 message도 같은 digest로 바꾼다(`quiet_provider_details`).
  - provider·model을 찾지 못함, 인증 필요, 지원하지 않는 기능 오류는 설정 값(provider·model ID, 기능 이름)만 담으므로 그대로 둔다.
  - Oracle은 원문을 `/method status` 상세에 따로 둘 수도 있다고 했지만, 원문은 어디에도 남기지 않았다. 진단에는 상태 코드와 길이·digest만 남는다.

검증 기록은 다음과 같다.

- **테스트:**
  - 새 AgentLoop 테스트 2개(`composed_continuity.rs`):
    - 첫째는 위 장면이다. 승인을 기다리는 동안 저장 session의 messages에 thinking과 모델 문장이 없고, 보관 metadata에는 있다. 승인 뒤 provider 요청과 Anthropic 요청 본문은 위와 같다. 요청이 끝난 뒤에는 저장 session 어디에도 thinking 문장과 서명이 없다.
    - 둘째는 모델이 지어낸 도구 이름을 오류 글에 인용하는 provider다. 답은 상태 코드와 digest로 시작하고, 저장 session 전체(실행 기록 포함)에 그 이름이 없다.
  - 새 단위 테스트 1개(`agent_loop_method_held`): thinking block이 기록에서 빠지고 자기 호출의 turn에만 되돌아간다. 같은 호출의 turn이 둘이면 각자의 block을 받고, 앞부분이 잘린 기록에서는 최근 block을 받는다. 마지막 답의 thinking은 보관하지 않는다. 기존 단위 테스트 1개는 보관 형식(`results`·`thinking`)에 맞췄다.
  - 결과는 `oracle9-fix-after-agentloop.log` 123/123, `oracle9-fix-after-unit.log` 59/59, `oracle9-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드(`docker-build.log`)부터 QA 기록 전체를 9차 반영 코드로 다시 만들었다. 장면과 결과는 8차와 같다. 저장 기록의 tool 메시지 17개 중 14개가 digest이고, 요청이 끝난 뒤 보관 metadata는 없으며, "blue"는 저장 기록 어디에도 없다.
  - `qa-native-composed-no-image.txt` 끝에 새 장면을 더했다(`qa.sh`). 미리 정한 모델 응답이 다 쓰인 뒤의 복합 요청은 debug fake provider의 API 오류로 끝난다. 답은 "Sorry, I encountered an error calling the AI model: provider API error; its message is withheld (<withheld: 43 chars, sha256:c8943ef2b06e>)."다.
  - 오류 원문("debug fake provider response queue is empty", 43자, 같은 digest)은 저장 session 어디에도 없고, 실행 기록의 오류 상세도 같은 digest다(`qa-native-provider-error-check.txt`).
  - fake provider 응답에는 thinking block이 없으므로 thinking 보관과 되돌림은 통합 테스트로만 확인했다.
- **패키지 검증(Oracle 9차 반영 후 코드):** 저장소 작업 지침(`AGENTS.md`)은 전체 workspace 테스트를 사용자 승인 없이 돌리지 말고, 작업 끝에 바꾼 package의 테스트로 확인하라고 한다. 9차 반영에서 바꾼 package는 `shacs-core` 하나이므로 `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --no-fail-fast -q`를 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test-before-oracle10.log`, `count-tests.py`, 992초). `count-tests.py`는 `-q` 출력에서도 실패한 테스트 이름을 읽도록 고쳤다.
  - suite 124개 기준 **1,582 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 8차 반영 후 전체 실행(`workspace-test-before-oracle9.log`)에서 `shacs-core` 실행 파일 구간만 같은 규칙으로 세면 suite 124개, 1,579 passed / 1 failed / 3 ignored다. 그 대비 +3이며, 새 테스트 3개(AgentLoop 2, 단위 1)와 일치한다.
  - 다른 package의 코드는 9차에서 바뀌지 않았다. `shacs-core`를 쓰는 CLI 쪽은 관련 테스트를 다시 돌렸다(`oracle9-fix-after-cli.log`). 9차 반영 코드의 전체 workspace 실행은 사용자 승인 뒤로 남겼다.
  - 실행이 남긴 Spec034 toolchain 사본 4개(각 230–460 MB)는 실행이 끝난 뒤 지웠다.
  - 코드 diff는 `oracle9-fix.patch`(9차 검토 시점 대비)다. 전체 diff `prd040.patch`는 10차 반영 뒤 다시 만들었다.

### Oracle 10차 검토 반영 (BLOCKED → 수정)

열 번째 독립 Oracle gate는 9차의 직접 수정이 대상 테스트를 통과함을 확인하고 차단 항목 셋을 냈다. 새 테스트와 바꾼 테스트를 10차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle10-fix-before-agentloop.log`: 4개 실패).

- 이미지 요청에 설정한 reasoning effort("high")가 그대로 갔다.
- 방식 변경 뒤 계산 방식으로 이어간 실행의 hook이 보관했던 파일 내용을 받았다.
- 이미지 부분이 없는 복합 요청의 답에 "Error: tool_use 'The artwork is red' is not a valid tool [runtime: no source marker, not checked]"와 "Error: path is outside the configured workspace [source: notes.md] [runtime: not checked]"가 그대로 보였다.

고친 내용은 다음과 같다.

- **방식 변경 뒤 hook이 보관한 내용을 받음:** 복합 요청에서 `/method use calculation`으로 바꿔도 같은 요청이므로 보관한 도구 결과(9차부터는 thinking도)는 모델 문맥에 돌아갔다. 그런데 hook에 조용한 사본을 넘기는 일은 지금 방식이 이미지·복합일 때만 했다. 그래서 계산 방식 실행의 진단·정책 hook이 그 원문을 받았다.
  - 이제 지금 방식이 이미지·복합이 아니어도, 그 요청이 보관한 결과가 있으면 그 실행의 모든 hook 문맥에서 보관했던 결과를 저장 기록처럼 길이·digest로 바꾼다(`agent_loop_method_quiet.rs::Unheld`, `agent_loop_method_held.rs::held_outputs`).
  - 그 실행에서 새로 생긴 내용(계산 방식의 출력 등)은 그 방식의 규칙대로 둔다.
  - thinking은 아래 셋째 항목대로 더는 보관하지 않으므로 되돌릴 것이 없다.
- **provider가 오류를 응답으로 알리면 원문이 답에 들어감:** 9차는 `ProviderError::Api`만 바꿨다. 그런데 Anthropic client는 HTTP 오류 응답과 스트림의 오류 frame을 `finish_reason: "error"` 응답(내용 "Error: <원문>")으로 돌려준다. 재시도가 다 끝난 뒤의 오류도 같은 모양이다. runner는 그 내용을 최종 답과 오류로 돌려주므로, 이미지 부분이 없는 복합 요청에서는 원문이 runtime 표시와 함께 답·저장 기록·SDK 결과에 들어갔다.
  - 이제 이미지·복합 요청의 실행이 `error`로 끝나면 최종 답과 오류를 "Sorry, I encountered an error calling the AI model: provider API error (HTTP 400); its message is withheld (<withheld: N chars, sha256:…>)."로 바꾼다(새 `agent_loop_method_errors.rs::quiet_run_error`).
  - 상태 코드는 runner가 응답에서 새로 넘긴다(`AgentRunResult.error_status`, 값이 있을 때만 직렬화).
  - 복합 요청의 답은 이 줄을 보고서 줄로 확인하지 않고 그대로 보인다(`task_method_compose_answer.rs`).
  - 자체 검토에서 같은 모양의 다른 길도 막았다. 실행 중 도구 결과가 turn을 멈추는 오류(작업 폴더 밖 경로 등)이면 runner는 그 출력을 최종 답으로 돌려준다. 승인 뒤 실행 경로는 4차의 `quiet_failure`가 이미 바꿨지만, 실행 중 경로는 아니었다. 이제 "A tool call stopped this turn; its output is withheld (Error: <withheld: …>)."로 바꾼다.
  - provider 오류를 다루는 함수들은 `agent_loop_method_hold.rs`에서 새 `agent_loop_method_errors.rs`로 옮겼다(줄 수 한도).
- **Anthropic thinking을 provider가 요구하는 모양 그대로 되돌릴 수 없음:** client는 응답의 content를 thinking·합친 본문·도구 호출로 나눠 저장하고, `redacted_thinking`은 버린다. 다시 보낼 때는 thinking을 늘 맨 앞에 둔다. 그리고 interleaved thinking beta는 늘 켜져 있다. 그래서 9차의 "thinking을 바꾸지 않고 맨 앞에 두면 된다"는 답은 모든 thinking 형태에 성립하지 않는다.
  - Oracle이 제시한 대안대로, 이미지·복합 요청의 실행에서는 provider에 thinking을 요청하지 않는다. 설정한 reasoning effort를 보내지 않고, 앞선 대화의 thinking block도 provider 문맥에서 뺀다(`hold_model_text`).
  - 그래도 provider가 보낸 thinking block은 저장할 때 버린다(`hold_from`).
  - 9차의 thinking 보관과 되돌림(`hold_thinking`, `calls_key`, `restore_thinking`, 보관 metadata의 `thinking`)은 지웠다.
  - 그래서 이미지·복합 요청에서는 확장 추론을 쓰지 않는다는 제한이 생긴다(USAGE·PRD030에 적음). OpenAI 호환 provider에서는 reasoning effort를 보내지 않으면 그 provider의 기본값을 쓴다.

검증 기록은 다음과 같다.

- **테스트:**
  - `composed_continuity.rs`의 두 테스트를 바꿨다.
    - thinking 테스트는 reasoning effort "high" 설정에서 일반 요청에는 그 값이 가고, 이미지 요청의 두 provider 요청(첫 실행, 승인 뒤)에는 가지 않음을 확인한다. 그 두 요청의 Anthropic 요청 본문에는 `thinking`도, 앞선 대화의 thinking block(`sig-0`)도 없다. provider가 그래도 보낸 thinking(`sig-1`)은 승인 대기 중과 요청이 끝난 뒤의 저장 session 어디에도 없다.
    - provider 오류 테스트는 네 경우를 돈다. `ProviderError::Api`(이미지 포함 복합), 오류 응답(이미지 없는 복합, 비스트리밍과 스트리밍), 재시도가 다 끝난 오류 응답(이미지 포함 복합)이다. 답에는 상태 코드와 digest만 있고, 저장 session 전체(실행 기록 포함)에 지어낸 이름이 없다.
  - `composed_keep.rs`의 방식 변경 테스트에 진단 hook 기록을 더했다. 계산 방식으로 이어간 뒤 모델 문맥에는 보관했던 파일 내용이 있고, hook이 받은 어떤 문맥에도 없다.
  - 새 AgentLoop 테스트 1개(`composed_errors.rs`): 실행 중 turn을 멈춘 도구 오류의 답은 digest만 보이고, 저장 session 어디에도 원문이 없다.
  - 9차의 thinking 단위 테스트는 그 동작이 없어져 지웠다. 그래서 테스트 수의 순증은 0개다(AgentLoop +1, 단위 −1).
  - 결과는 `oracle10-fix-after-agentloop.log` 124/124, `oracle10-fix-after-unit.log` 58/58, `oracle10-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다. 테스트는 모두 `-q`로 실행했다.
- **실제 표면 재확인:** Docker 빌드(`docker-build.log`)부터 QA 기록 전체를 10차 반영 코드로 다시 만들었다. docker 장면과 결과는 9차와 같다.
  - `qa-native-composed-gate.txt` 끝에 새 장면을 더했다(`qa-setup.py`, `qa.sh`). provider가 지어낸 도구 이름을 인용한 오류 응답(HTTP 400)을 돌려주면, 답은 "Sorry, I encountered an error calling the AI model: provider API error (HTTP 400); its message is withheld (<withheld: 49 chars, sha256:50e8f8379e70>)."다.
  - `qa-native-provider-error-check.txt`는 이 장면과 9차의 API 오류 장면 모두 저장 session 어디에도 원문이 없음을 확인한다.
- **패키지 검증(Oracle 10차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test-before-oracle11.log`, `count-tests.py`, 1,070초).
  - suite 124개 기준 **1,582 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 9차 반영 후 1,582와 같으며, 테스트 순증 0개와 일치한다.
  - `AgentRunResult`에 더한 `error_status`는 `shacs-core` 안에서만 쓰인다(다른 crate는 이 타입을 쓰지 않는다). 다른 package의 코드는 바뀌지 않았고, CLI 쪽 관련 테스트는 다시 돌렸다. 10차 반영 코드의 전체 workspace 실행도 사용자 승인 뒤로 남겼다.
  - 실행이 남긴 Spec034 toolchain 사본 4개(각 230–460 MB)는 끝난 뒤 지웠다. 디스크 여유가 3 GiB까지 줄어, `crates/target`의 오래된 incremental build cache(오늘 09:30 전에 마지막으로 쓰인 것, 두 번에 걸쳐 약 8.5 GiB)도 지웠다. 다시 만들 수 있는 build cache다.
  - 코드 diff는 `oracle10-fix.patch`(10차 검토 시점 대비)다. 전체 diff `prd040.patch`는 11차 반영 뒤 다시 만들었다.

### Oracle 11차 검토 반영 (BLOCKED → 수정)

열한 번째 독립 Oracle gate는 10차 수정을 확인했다. 사용자 답, 기록, checkpoint, thinking 차단 경로와 대상 테스트에는 문제가 없었다. 차단 항목은 하나였다. 바꾼 테스트를 11차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle11-fix-before-agentloop.log`: 1개 실패). 여섯 가지 canary 문자열이 모두 진단 hook 기록에 나왔다.

- **hook에 넘기는 응답 사본에 provider 고유 필드가 남음:** `Quiet::after_response`는 provider 응답을 통째로 복제한 뒤 본문, reasoning, thinking, 호출 ID와 인자만 지웠다. 그래서 다음이 그대로 남았다.
  - 도구 호출의 확장 필드 `extra_content`, `provider_specific_fields`, `function_provider_specific_fields`. OpenAI 호환 parser는 응답의 임의 확장 필드를 여기에 복사한다.
  - 응답의 오류 문자열 `error_kind`, `error_type`, `error_code`.
  - 이 필드들은 진단·plugin hook에 그대로 갔다. hook 문맥의 assistant 메시지 호출(`to_openai_tool_call`로 저장된 모양)에도 확장 필드가 남았다.
- 이제 hook에 넘기는 사본은 허용한 필드만으로 새로 만든다(`agent_loop_method_quiet.rs`).
  - 응답은 호출(digest ID, 제공된 도구 이름 또는 `unoffered_tool`, 인자 digest), `finish_reason`, `usage`, 재시도 시간, 상태 코드, 재시도 여부만 담는다(`Quiet::response`).
  - hook 문맥의 assistant 메시지 호출도 `id`(digest), `type`, `function.name`, `function.arguments`(digest)만으로 다시 만든다.
  - 저장 기록과 checkpoint의 호출은 provider가 대화를 이어가는 데 필요하므로(예: 확장 필드에 담기는 provider의 서명) 그대로 둔다. 이는 문서에 적은 예외(제공된 도구의 호출, 인자와 ID 포함)에 해당한다.

검증 기록은 다음과 같다.

- **테스트:** `composed_quiet.rs`의 진단 hook 테스트에서, provider의 셋째 응답 호출에 세 확장 필드를, 응답에 세 오류 문자열을 각기 다른 canary 값으로 넣었다. 진단 hook 기록(응답 사본과 문맥)과 tool 이벤트 어디에도 "canary"가 없음을 확인한다. 새 테스트 파일이나 테스트 수 변화는 없다.
  - 결과는 `oracle11-fix-after-agentloop.log` 124/124, `oracle11-fix-after-unit.log` 58/58, `oracle11-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 11차 반영 코드로 다시 만들었다. 장면과 결과는 10차와 같다.
- **패키지 검증(Oracle 11차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test-before-oracle12.log`, `count-tests.py`, 1,031초).
  - suite 124개 기준 **1,582 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다. 10차 반영 후와 같다.
  - 실행이 남긴 Spec034 toolchain 사본 4개는 끝난 뒤 지웠다. 11차 반영 코드의 전체 workspace 실행도 사용자 승인 뒤로 남겼다.
  - 코드 diff는 `oracle11-fix.patch`(11차 검토 시점 대비)다. 전체 diff `prd040.patch`는 12차 반영 뒤 다시 만들었다.

### Oracle 12차 검토 반영 (BLOCKED → 수정)

열두 번째 독립 Oracle gate는 차단 항목 둘을 냈다. 바꾼 테스트를 12차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle12-fix-before-agentloop.log`: 3개 실패).

- provider 이벤트와 진단 hook에 "canary-finish" 종료 이유와 "canary-usage" 사용량 항목이 그대로 나왔다.
- 방식 변경 뒤 계산 방식 실행의 hook에 이전 단계의 호출 ID(`r1`)가 그대로 나왔다.

고친 내용은 다음과 같다.

- **provider가 정한 종료 이유와 사용량 항목이 그대로 감:** 11차의 허용 목록은 `finish_reason`과 `usage`를 그대로 복사했다. provider parser는 알 수 없는 종료 사유를 원문 그대로 두므로, provider가 넣은 임의 문자열이 plugin `llm:after` 같은 hook에 갔다. 스트리밍의 `ProviderEvent::Finish`도 원래 `reason`과 `usage`를 그대로 보냈다.
  - 이제 hook에 넘기는 응답과 provider 종료 이벤트에서 종료 이유는 runtime이 아는 값(`stop`, `tool_calls`, `length`, `content_filter`, `error`)만 두고 나머지는 `other`로 바꾼다(`quiet_tools.rs::quiet_finish_reason`).
  - 사용량은 runtime의 provider들이 쓰는 token 수 항목(`prompt_tokens`, `completion_tokens`, `total_tokens`, `cached_tokens`, `input_tokens`, `output_tokens`, `cache_read_input_tokens`, `cache_creation_input_tokens`)의 수만 남긴다(`quiet_usage`).
  - 테스트용 `Streaming` provider(`image_stream.rs`)는 이제 실제 provider처럼 응답마다 `Finish` 이벤트를 보낸다.
- **방식 변경 뒤 이전 단계의 호출 ID와 호출이 hook 문맥에 남음:** 10차의 `Unheld`는 되돌린 도구 결과 본문만 digest로 바꿨다. 그래서 이미지·복합 단계에서 저장된 호출의 ID(assistant 호출과 tool 메시지 양쪽), 인자, provider 확장 필드는 그대로 hook에 갔다. 방식 변경으로 취소된 호출처럼 보관된 결과가 없는 호출도 마찬가지였다.
  - 이제 보관 metadata는 이미지·복합 실행이 저장한 모든 호출 ID도 적어 둔다(`agent_loop_method_held.rs::hold_from`, `Held.calls`).
  - 다른 방식의 실행에서는 그 ID에 연결된 assistant 메시지와 tool 메시지를 이미지·복합 실행의 hook과 같은 방식으로 바꾼다(`agent_loop_method_quiet.rs::Unheld`). 호출은 digest ID와 이름, digest 인자로 다시 만들고, tool 메시지는 결과와 호출 ID를 digest로 바꾼다. 이 처리는 `Quiet`와 같은 함수(`quiet_assistant`, `quiet_tool`)를 쓴다.
  - 새 방식에서 생긴 그 실행의 호출은 그 방식의 규칙대로 그대로 둔다.

검증 기록은 다음과 같다.

- **테스트:** 새 테스트 파일이나 테스트 수 변화는 없다.
  - `composed_unoffered.rs`의 스트리밍 테스트: 마지막 응답의 종료 이유와 사용량 항목에 canary를 넣었다. provider 이벤트에는 `reason: "other"`와 알려진 `total_tokens`만 있고 "canary"는 없다.
  - `composed_quiet.rs`의 진단 hook 테스트: 마지막 응답의 종료 이유와 사용량 항목에 canary를 넣었다. hook 기록에 "canary"가 없다.
  - `composed_keep.rs`의 방식 변경 테스트: 계산 방식 실행의 hook 기록에 이전 단계의 호출 ID(`r1`, `run-1`, 방식 변경으로 취소된 `run-2`)가 없고, 그 실행의 호출(`run-3`)은 있다. 모델 문맥에는 보관했던 파일 내용이 되돌려진다.
  - 결과는 `oracle12-fix-after-agentloop.log` 124/124, `oracle12-fix-after-unit.log` 58/58, `oracle12-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 12차 반영 코드로 다시 만들었다. 장면과 결과는 11차와 같다.
- **패키지 검증(Oracle 12차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test-before-oracle13.log`, `count-tests.py`, 963초).
  - suite 124개 기준 **1,582 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다. 11차 반영 후와 같다.
  - 실행이 남긴 Spec034 toolchain 사본 4개는 끝난 뒤 지웠다.
  - 디스크 여유가 4 GiB까지 줄어, 실행 전에 `crates/target/debug/deps`에서 오늘 09:00 뒤로 다시 만들어지지 않은 test 실행 파일 303개(11.5 GiB)를 지웠다. 이전 전체 실행의 다른 crate 것과 지난 build의 사본이며, 필요하면 cargo가 다시 만든다.
  - 12차 반영 코드의 전체 workspace 실행도 사용자 승인 뒤로 남겼다.
  - 코드 diff는 `oracle12-fix.patch`(12차 검토 시점 대비)다. 전체 diff `prd040.patch`는 13차 반영 뒤 다시 만들었다.

### Oracle 13차 검토 반영 (BLOCKED → 수정)

열세 번째 독립 Oracle gate는 차단 항목 둘을 냈다. 바꾼 테스트를 13차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle13-fix-before-agentloop.log`: 2개 실패).

- 저장된 실행 기록에 `tool:…:run-2`와 `"run-1"` 같은 원래 호출 ID가 그대로 있었다.
- provider 이미지 진행 이벤트에 provider가 정한 후보 ID "The-artwork-is-blue"가 그대로 나왔다.

고친 내용은 다음과 같다.

- **실행 기록에 모델이 정한 호출 ID가 그대로 남음:** runner는 도구 호출의 실행 식별자(`effect_id`, `correlation_id`, `idempotency_key`)를 원래 호출 ID로 만들었다. 이 실행 기록은 session metadata `runtime_execution`에 저장되고(복구용 checkpoint 포함) durable event의 출처 정보로도 쓰인다. 방식 변경 뒤에도 그대로 남았다.
  - 이제 이미지·복합 요청의 실행(승인 뒤 실행 경로 포함)에서는 실행 기록이 도구 호출을 원래 ID 대신 그 전체 SHA-256 digest(`sha256:…`)로 부른다(`runner.rs::tool_call_key`, 새 `AgentRunSpec.opaque_tool_ids`).
  - 이 표시는 `hold_model_text`와 승인 뒤 실행 경로(`agent_loop_method_image.rs::opaque_tool_ids`)가 켠다. 방식을 읽지 못하면 켠다.
  - 같은 호출의 시작과 결과는 같은 digest라 기록 안에서 서로 맞는다. 저장된 실행 기록은 다음 turn에 도구 기록을 다시 쓰지 않으므로(하위 작업 대기만 이어짐) 다른 동작은 바뀌지 않는다.
  - 다른 요청은 지금처럼 원래 ID를 쓴다. 다른 테스트가 원래 ID를 기대하기 때문이다.
  - 대화 기록과 checkpoint의 호출 ID는 provider가 호출과 결과를 짝짓는 데 필요하므로 그대로다(문서에 적은 예외).
- **이미지 진행 이벤트에 provider가 정한 후보 ID가 그대로 감:** `hold_model_text`는 `ProviderEvent::MediaLifecycle`을 바꾸지 않고 보냈다. 내장 Codex 경로는 후보 ID를 digest로 만들지만, provider 계약은 영숫자와 `-`·`_`로 된 임의 문자열을 허용한다.
  - 이제 상태와 순번은 그대로 두고, 후보 ID는 그 SHA-256 digest(`sha256-…`)로 바꾼 이벤트를 보낸다(`quiet_tools.rs::quiet_media`).

검증 기록은 다음과 같다.

- **테스트:** 새 테스트 파일이나 테스트 수 변화는 없다.
  - `composed_keep.rs`의 방식 변경 테스트: `/method use calculation` 직후 저장된 `runtime_execution`에 이전 단계의 호출 ID(`r1`, `run-1`, 방식 변경으로 취소된 `run-2`)가 없고 `sha256:` 식별자가 있다.
  - `composed_unoffered.rs`의 스트리밍 테스트: provider가 후보 ID "The-artwork-is-blue"로 이미지 진행 이벤트를 보내도 provider 이벤트 기록에 `MediaLifecycle`은 있고 "artwork"는 없다.
  - 결과는 `oracle13-fix-after-agentloop.log` 124/124, `oracle13-fix-after-unit.log` 58/58, `oracle13-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 13차 반영 코드로 다시 만들었다. 장면과 결과는 12차와 같다.
- **패키지 검증(Oracle 13차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test-before-oracle14.log`, `count-tests.py`, 1,067초).
  - suite 124개 기준 **1,582 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다. 12차 반영 후와 같다.
  - 실행이 남긴 Spec034 toolchain 사본 4개는 끝난 뒤 지웠다. 디스크 여유가 다시 2.4 GiB까지 줄어 `crates/target/debug/incremental`(7 GiB, 다시 만들 수 있는 build cache)도 지웠다.
  - 13차 반영 코드의 전체 workspace 실행도 사용자 승인 뒤로 남겼다.
  - 코드 diff는 `oracle13-fix.patch`(13차 검토 시점 대비)다. 전체 diff `prd040.patch`는 14차 반영 뒤 다시 만들었다. (`opaque_tool_ids`는 14차에서 `quiet_ledger`로 이름을 바꿨다.)

### Oracle 14차 검토 반영 (BLOCKED → 수정)

열네 번째 독립 Oracle gate는 수용 조건 1–3의 실제 CLI 증거와 부분별 판정은 충족한다고 보고, 차단 항목 하나를 냈다. 바꾼 테스트를 14차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle14-fix-before-agentloop.log`: 1개 실패). 저장된 실행 기록에 중간 계산 단계의 호출 ID "The-artwork-is-yellow"(`correlation_id`와 `tool:…:The-artwork-is-yellow`)와 모델의 질문 "Is the artwork blue?"가 그대로 있었다.

- **방식 변경으로 거친 중간 단계의 실행 기록:** 이미지·복합 요청을 계산·일반 방식으로 바꿨다가 `/method auto`로 되돌리면, 중간 단계의 실행에서는 13차의 digest 표시가 꺼져 있었다. 그래서 그 단계의 원래 호출 ID가 실행 기록에 남았다. 재진입 정리도 대화 기록만 다뤘다.
  - 대기 중인 하위 작업은 turn이 바뀌어도 실행 기록에 남으므로, 중간 단계에서 `spawn`했다면 원래 호출 ID가 그 원인(`causation_id`)에 계속 남는다. durable event의 출처 정보도 그 기록에서 나온다.
  - 바꾼 테스트를 돌리다가 같은 기록에 `ask_user` 질문이 원문으로 남는 것도 찾았다. runner는 질문 interrupt의 내용(모델이 쓴 질문)을 실행 기록의 `detail`로 둔다.

고친 내용은 다음과 같다.

- 실행 기록 표시를 `opaque_tool_ids`에서 `quiet_ledger`로 이름을 바꾸고 범위를 넓혔다(`runner.rs`). 이 표시가 켜지면 실행 기록은 도구 호출을 ID의 digest로 부르고, 모델이 한 질문은 digest로만 둔다.
- 요청이 이미지·복합 방식이었던 적이 있어 보관 metadata를 가지고 있으면, 지금 방식이 계산·일반이어도 끝날 때까지 이 표시를 켠다. 본 실행(`hold_model_text`)과 승인 뒤 실행 경로(`agent_loop_method_image.rs::quiet_ledger`) 모두 같다.
- 방식 변경으로 요청이 다시 이미지·복합 방식이 되면, 저장된 실행 기록에 남은 원래 호출 ID(도구 결과의 식별자와 하위 작업의 원인)와 질문을 digest로 바꾼다(`quiet_tools.rs::quiet_execution_record`, `quiet_request_history`에서 부름). 처음부터 계산 방식이던 요청이 나중에 복합 방식이 된 경우도 이것으로 다룬다. 다만 그 전 turn이 이미 쓴 durable event는 되돌리지 않는다.

검증 기록은 다음과 같다.

- **테스트:**
  - `composed_reentry.rs`의 재진입 테스트: 중간 계산 단계에서 모델이 호출 ID "The-artwork-is-yellow"로 `spawn`을 부르고(승인 뒤 실행), 질문을 한다. `/method auto` 직후 저장된 실행 기록에는 `sha256:` 식별자가 있고 "artwork"가 없다. 다음 turn 뒤에도 없다.
  - 새 단위 테스트 1개(`quiet_tools`): 실행 기록 정리가 도구 결과의 식별자, 질문, 대기 중인 하위 작업의 원인을 digest로 바꾸고, 이미 digest인 식별자, runtime 문구("Permission approval required."), provider 식별자는 그대로 둔다.
  - 이 harness에서는 `spawn`이 process 실행 권한이라 늘 승인을 거친다. 승인 뒤 실행 경로는 하위 작업을 대기로 기록하지 않으므로(이전부터의 동작), 실행 중 바로 시작한 하위 작업의 원인은 단위 테스트로 확인했다.
  - 결과는 `oracle14-fix-after-agentloop.log` 124/124, `oracle14-fix-after-unit.log` 59/59, `oracle14-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
  - CLI 테스트의 첫 실행에서는 `spec037_image_verbose`의 gateway 테스트 하나가 30초 동안 gateway WebSocket에 연결하지 못해 실패했다(`oracle14-fix-after-cli-first-attempt.log`). 코드를 바꾸지 않고 다시 실행하자 모두 통과했다. 디스크와 메모리가 빠듯할 때의 일시적인 실패로 판단했다.
- **실제 표면 재확인:** Docker 빌드부터 QA 기록 전체를 14차 반영 코드로 다시 만들었다. 장면과 결과는 13차와 같다.
- **패키지 검증(Oracle 14차 반영 후 코드):** 같은 명령을 `CARGO_INCREMENTAL=0`으로 실행했다(`.omo/evidence/spec037/prd040/shacs-core-test.log`, `count-tests.py`, 1,185초). incremental cache가 디스크를 채우지 않게 하려는 것이며, 테스트 내용은 같다.
  - suite 124개 기준 **1,583 passed / 1 failed / 3 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 13차 반영 후 1,582 대비 +1이며, 새 단위 테스트 1개와 일치한다.
  - 실행이 남긴 Spec034 toolchain 사본 4개와, 이 설정으로 따로 만들어진 build 산출물(2.4 GiB)은 끝난 뒤 지웠다.
  - 14차 반영 코드의 전체 workspace 실행도 사용자 승인 뒤로 남겼다.
  - 코드 diff는 `oracle14-fix.patch`(14차 검토 시점 대비)다. 전체 diff `prd040.patch`는 15차 반영 뒤 다시 만들었다.

### Oracle 15차 검토 반영 (BLOCKED → 수정)

열다섯 번째 독립 Oracle gate는 수용 조건 1–3과 실제 CLI 증거가 충족됐다고 보았다. 실행 기록 대조, 승인 뒤 재개, 하위 작업 결과 연결도 digest 변환으로 깨지지 않는다고 확인했다. 차단 항목 둘은 모두 14차의 실행 기록 정리에 있었다. 이미 가려진 값인지를 앞부분 문자열만 보고 판정했다. 바꾼 단위 테스트를 15차 검토 시점 코드(scratch 사본)에서 먼저 실행해 실패를 확인했다(`oracle15-fix-before-unit.log`: 1개 실패). 위장한 값 `sha256:The-artwork-is-green`, `sha256:The-artwork-is-blue`, `<withheld: Is the artwork blue?>`가 그대로 남았다.

- **위장한 digest가 그대로 남음:** 호출 ID는 `sha256:`로 시작하기만 하면, 질문은 `<withheld:`로 시작하기만 하면 이미 가려진 값으로 보았다. 그래서 모델이 그런 모양의 ID나 질문을 쓰면 원문이 남았다.
  - 이제 정확한 모양만 이미 가려진 값으로 본다(`quiet_tools.rs::is_opaque_call_id`, `is_quiet_text`). 호출 ID는 `opaque_call_id`가 만든 모양(`sha256:` 뒤 64자리 소문자 16진수)이어야 한다. 질문은 `quiet_text`가 만든 모양(`<withheld: N chars, sha256:` 뒤 12자리 16진수와 `>`, 앞의 `Error: `는 허용)이어야 한다.
  - 그 밖의 값은 다시 digest로 바꾼다. 정확한 모양에는 숫자와 16진수만 있으므로 모델의 말이 담기지 않는다.

검증 기록은 다음과 같다.

- **테스트:** `quiet_tools` 단위 테스트(`an_execution_record_names_earlier_calls_by_digests`)에 위장한 호출 ID, 위장한 질문, 위장한 하위 작업 원인을 더했다. 한 번 정리한 기록을 다시 정리해도 바뀌지 않음도 확인한다. 테스트 수 변화는 없다.
  - 결과는 `oracle15-fix-after-agentloop.log` 124/124, `oracle15-fix-after-unit.log` 59/59, `oracle15-fix-after-cli.log`(이미지 CLI 2/2·verbose 2/2·REPL 4/4·command 3/3), `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)다.
- **패키지 검증과 실제 표면:** 15차 반영은 재진입 때만 쓰는 실행 기록 정리의 두 판정 함수와 그 단위 테스트만 바꿨다. 그래서 패키지 전체와 QA는 다시 실행하지 않았다.
  - 저장소 지침은 고친 뒤에는 실패했던 테스트와 영향받는 테스트만 다시 실행하라고 한다. 또 디스크 여유가 4–6 GiB인데, 패키지 실행 한 번이 최대 약 8 GB를 쓴다.
  - 바뀐 함수는 QA 장면에서 쓰이지 않는다(재진입 장면이 없음).
  - 마지막 패키지 실행과 QA 기록은 14차 반영 코드의 것이다(`shacs-core-test.log`: 1,583 passed / 1 failed / 3 ignored).
  - 코드 diff는 `prd040.patch`(PRD030 종료 기준 대비 전체), `oracle15-fix.patch`(15차 검토 시점 대비)다.

## 종료 확인 (2026-10-23)

- 범위: 계산·보고서·이미지 중 둘 이상이 든 요청을 `composed` 방식으로 처리한다.
  - 부분마다 PRD010·020·030 규칙으로 따로 기록·판정한다. 한 부분의 성공이 다른 부분의 실패나 미검증을 통과시키지 않는다. 쓸 수 없는 부분은 `unavailable`로 남기고 나머지만 제한적으로 전달한다.
  - 답이 먼저 나온다. 부분별 상태 한 줄과 요약, 확인되지 않은 줄의 runtime 표시를 보인다. 이미지 부분이 있으면 이 요청에서 읽은 파일을 그대로 옮긴 줄만 보인다. `[calculation]`·`[image]` 자리는 runtime이 채운다. 코드·실행 기록·출처 상세는 `/method status`에서 본다.
  - 진행 중 방식 변경과 부분 취소 뒤에도 같은 목표와 이미 확인한 근거를 잇고, 새 판에서 다시 판정한다.
  - 이미지·복합 요청에서 모델과 provider가 쓴 글은 아래 예외 밖으로 나가지 않는다. 답, 대화 기록, 진행 이벤트, hook, 복구 checkpoint, SDK 결과, 실행 기록, provider 오류가 모두 해당하며, 방식 변경을 거쳐도 같다.
  - 새 workflow engine, 자동 병렬화, 이미지 내용 확인은 포함하지 않는다.
- 최종 검증:
  - 바꾼 package(`shacs-core`)의 테스트는 suite 124개 기준 **1,583 passed / 1 failed / 3 ignored**다(14차 반영 코드, `shacs-core-test.log`). 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - 15차 반영(두 판정 함수와 단위 테스트) 뒤에는 대상 테스트를 다시 실행했다. 결과는 AgentLoop 124/124, 단위 59/59, CLI 2/2·2/2·4/4·3/3이다.
  - 9차 반영부터는 저장소 작업 지침(`AGENTS.md`: 전체 workspace 테스트는 사용자 승인 뒤)에 따라 바꾼 package로 확인했다. 커밋한 최종 코드(`968697a`)에서 사용자 승인을 받아 전체 workspace를 다시 실행했다(`workspace-test.log`, `CARGO_INCREMENTAL=0 cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast -q`, 1,454초).
    - suite 198개 기준 **3,308 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
    - 8차 반영 코드의 전체 실행 3,304(`workspace-test-before-oracle9.log`) 대비 +4다. 9차 이후 순증 테스트 4개와 일치한다(AgentLoop +3: 9차 2개와 10차 1개, 단위 +1: 9차 +1, 10차 −1, 14차 +1).
    - suite 수가 327개에서 198개로 준 것은 저장소 소유자 커밋 `c603fa3`이 통합 테스트를 crate마다 하나의 실행 파일로 합쳤기 때문이다.
    - `CARGO_INCREMENTAL=0`은 incremental build cache가 디스크를 채우지 않게 하려는 것이며, 테스트 내용은 같다. 실행이 남긴 Spec034 toolchain 사본 4개는 끝난 뒤 지웠다.
  - fmt와 workspace clippy `-D warnings`가 통과했다.
  - 커밋하면서 Spec037 통합 테스트를 저장소의 `tests/it` 구조로 옮겼다. 저장소 지침이 통합 테스트 실행 파일을 늘리지 말라고 하기 때문이다.
    - shacs-core와 shacs-cli의 테스트는 각 crate의 `it` 모듈이 됐다. shacs-command의 `/method` 테스트는 `router` 테스트의 하위 모듈(`tests/router/method.rs`)이 됐다.
    - 옮긴 뒤에도 같은 테스트가 통과했다(`--test it`의 `spec037_method_selection` 124개, shacs-cli `spec037_*` 8개, `--test router`의 `method` 3개). clippy도 다시 통과했다.
    - 위 기록에 나오는 `--test spec037_method_selection` 같은 실행 대상 이름은 옮기기 전 이름이다.
- 실제 표면: 공식 container와 같은 조건(Docker 빌드)의 compiled REPL과 macOS native REPL로 확인했다(`qa-docker-composed.txt`, `qa-native-composed-gate.txt`, `qa-native-composed-no-image.txt`).
  - 담은 장면은 다음과 같다. 보고서+계산의 확인과 불일치, 보고서+이미지의 실제 PNG와 runtime 이미지 줄, 계산+이미지의 답 보류, 방식 변경과 부분 취소, native process gate의 `unavailable`, 이미지 생성이 꺼진 환경, provider API 오류와 오류 응답의 digest 표시다.
  - 저장 기록, 파일, 도구 결과, provider 오류 기록은 따로 확인했다(`qa-docker-history-check.txt`, `qa-docker-artifacts.txt`, `qa-docker-tool-outputs.txt`, `qa-native-provider-error-check.txt`).
- Oracle 독립 검증: **PASS** (16차).
  - 1–15차에서 다음 결함을 차례로 찾았다. 모두 고친 뒤 수정 전 실패 로그와 회귀 테스트로 고정했다.
    - 이미지에 관한 모델 문장의 노출(낱말 목록 → 인용 규칙 → 문장·줄 단위 인용), `[calculation]` 줄의 문장 판정(→ runtime이 값을 적는 자리), 방식 변경 뒤의 이월 자료·취소 기록·수용 기록.
    - 진단 hook과 진행 이벤트의 도구 인자·결과, 모델이 고른 이름(경로, 호출 ID, 제공되지 않은 도구 이름), 보관 결과의 정리와 재진입, 같은 호출 ID의 결과.
    - Anthropic thinking의 보관과 재생(→ 이미지·복합 요청에서 thinking을 요청하지 않음), provider 오류(실패, 오류 응답, 실행 중 도구 오류).
    - provider 고유 필드(호출의 확장 필드, 오류 문자열, 종료 이유, 사용량, 이미지 후보 ID), 실행 기록의 호출 ID와 질문(중간 단계, 재진입, 위장한 digest).
- 남은 제한:
  - 이미지의 내용과 모양은 확인하지 않는다.
  - 인용은 파일에 그렇게 적혀 있다는 뜻일 뿐이다. 파일이 바뀌었는지는 수정 시각으로 판단하므로, 이전 요청에서 만든 파일이나 수정 시각을 되돌린 파일은 가려내지 못한다. 약어 뒤가 대문자로 시작하는 인용은 약어 앞뒤로 나뉘어도 통과할 수 있다.
  - 이미지·복합 요청에서는 확장 추론(thinking)을 요청하지 않는다. 질문·메시지·하위 작업·맡김 작업 제안 도구를 쓸 수 없고, skill 사용 알림이 나오지 않는다.
  - 요청이 끝난 뒤의 다음 요청과 checkpoint 복구 뒤의 모델은 이전 도구 결과의 digest만 본다.
  - 다음은 원문을 담는다(문서에 적은 예외): `/method status`, 승인 화면, 제공된 도구의 plugin `tool:before` 정책 hook, 대화 기록과 checkpoint의 제공된 도구 호출(인자·ID·연속성 필드), 요청이 열린 동안의 보관 metadata와 method 기록. `session export --yes`는 경고 후 원본 metadata를 내보내는 명시적 진단 기능이다.
  - 이미 쓰인 durable event는 되돌리지 않는다. 처음에 계산 방식이던 요청이 나중에 복합 방식이 되면, 그 전 turn의 durable event에는 원래 호출 ID가 남을 수 있다.
