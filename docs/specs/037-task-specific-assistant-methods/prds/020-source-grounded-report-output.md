# PRD 020. 출처가 확인되는 보고서 산출물

Status: Complete (Scoped)

Depends on: [PRD 000](000-method-selection-and-user-override.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

한 요청의 자료를 실제로 읽고 근거·모르는 점이 구분된 보고서를 작성·전달한다. **계산 검증이나 이미지 생성 없이도 보고서 사용자 흐름으로 단독 완료한다.**

## Scope

1. 지원되는 파일·웹/도구 자료를 사용자가 지정한 목적에 맞춰 읽고 자료의 출처·시점·가용 상태를 기록한다. 읽지 않은 자료를 출처로 인용하지 않는다.
2. 보고서 구성·초안·최종 산출물의 결론을 확인한 출처와 연결하고 출처가 부족하거나 상충하면 해당 결론의 제한을 표시한다. 기존 파일 작성/첨부 전달과 024 recipe/verifier는 실행 수단으로 소비한다.
3. 결과 요약·핵심 근거·미해결 질문을 먼저 제공하고 상세 참고 자료·작성/검증 단계는 따로 확인할 수 있게 한다. 사용자 수정 요청은 같은 보고서 목적과 확인된 출처를 유지하며 새 판에서 다시 검증한다.

## Non Scope

- 수치 계산의 독립 검증, 이미지 생성/편집, 한 요청의 복수 방식 결합.
- 모든 외부 문서의 사실성 보증, 원격 문서 저장 서비스, 024 workflow engine/skill registry 재소유.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 3 중 보고서 출처 확인, 범위 4의 보고서 결과·상세 조회를 소유한다.
- 부모 수용 조건 3의 보고서 출처와 조건 5의 실제 보고서 표면을 소유한다.

## Acceptance Criteria

1. 사용자 소유 제어 자료를 읽고 짧은 보고서를 실제 CLI/대화·파일 산출물로 제공한다. 각 핵심 결론에서 실제 읽은 출처를 확인할 수 있고 요약이 상세 기록보다 먼저 보인다.
2. 존재하지 않는 출처, 읽기 실패, 상충하는 두 자료 또는 작성 후 수정된 자료를 입력하면 검증되지 않은 결론이 확인된 사실처럼 표시되지 않는다.
3. 사용자 수정 뒤 이전 결론의 확인 상태가 무조건 유지되지 않으며 변경된 부분의 새 근거와 미확인 범위를 명확히 보여준다. 도구가 unavailable이면 산출 완료를 합성하지 않는다.

## Closure Evidence

- 읽은 입력·출처·보고서 결과의 대응 및 파일/사용자 표면 transcript.
- 허구 인용·누락/상충 출처·수정 후 오래된 근거의 거절 테스트.

PRD 020은 **출처 기반 보고서 하나를 작성·수정·전달하는 결과**로 닫는다. 뒤 번호 PRD의 이미지·복합 결과 완료를 요구하지 않는다.

## 구현 메모 및 검증 근거 (2026-10-22)

아래는 구현·실행 기록이다. 완료 판정은 독립 Oracle gate 후 종료 확인 절에 기록한다.

- **읽은 출처 기록(`task_method_observe_report`, `task_method_report*`):** 보고서 방식 요청에서 `read_file`·`web_fetch` 결과를 같은 요청의 출처로 기록한다. 기록 항목은 위치, 읽은 시각, 내용 digest, 실패 사유이며 selection revision과 보고서 판에 귀속한다.
  - 작업 폴더 안 파일은 읽을 때 디스크 내용의 digest와 경로도 기록한다. 그래서 이후 변경을 다시 확인할 수 있다.
  - PRD010과 같은 관측 경로를 쓴다. 실행된 호출, hook 차단, 준비 단계 거절, 취소·건너뜀, 승인 후 실행이 모두 출처 성공·실패로 남는다.
- **보고서 결론과 출처 결속:** 결론 줄의 `[source: ...]`, `[unresolved: ...]`, `[conflict: ...]` 표시를 해석한다. 대상은 답변 본문과 `write_file` 내용, 그리고 `edit_file` 뒤 디스크에서 다시 읽은 파일 내용이다.
  - 각 인용은 이 보고서를 위해 읽은 출처에 연결되는지 판정한다. 상태는 읽음, 읽기 실패, 읽지 않음, 읽은 뒤 변경, 이전 판에서 가져옴(변경 없음 또는 확인 불가) 중 하나다.
  - 판정은 다음 중 하나다: `source-linked`, `citation problems`, `partly cited`(Oracle 1차 반영으로 추가), `no citations`, `no sources read`, `unavailable`, `not written`. 연결은 사실성 보증이 아님을 표면과 projection에 명시한다.
- **결과 우선 표면:** 답 다음에 판정, 읽은 출처, 연결되지 않은 결론과 이유, 충돌·미해결 수, 출처 표시 없는 문장 수, 사용자 수용 여부를 보여 준다. `/method status`는 출처별 기록과 결론별 인용 상태를 보여 주며, 표시할 때 작업 폴더 파일의 변경 여부를 다시 확인한다.
- **사용자 수정(`/method revise`):** 다음 메시지를 마지막 보고서 요청의 새 판으로 만든다. 같은 목적과 읽은 출처를 가져오되, 검증 상태는 새 판에서 다시 판정한다.
  - 가져온 파일 출처는 변경이 없을 때만 연결로 인정한다. 바뀐 파일과 웹 출처는 다시 읽어야 연결된다.
  - 새로 생기거나 바뀐 결론 수를 보여 준다.
  - 사용자 아닌 turn과 진행 중 요청에서는 거절한다.
- **도구 없음:** `read_file`이 없으면 PRD000의 가용성 판정으로 실행 전에 차단한다. 보고서 글이 있고 읽기가 모두 실패하면 `unavailable`이며 보고서 완료를 합성하지 않는다. 글이 없으면 먼저 `not written`이다.
- **기존 기대 변경:** PRD000 테스트 중 "보고서 방식은 validation claim이 비어 있다"는 기대를 "not verified claim 3개"로 바꿨다. PRD020 요구에 따른 의도적 변경이다.
- **테스트:**
  - 단위 4개: 표시 해석, 연결·실패·미독 판정, 미독·무인용·무본문, 변경·이전 판 출처.
  - AgentLoop 시나리오 7개: 연결된 보고서, 미독·실패·변경 출처, 충돌·미해결·무표시 문장, 무출처·무인용, 판 수정, 파일 보고서, 수정 대상 없음.
  - 사용자 아닌 turn의 `/method revise` 거절도 확인한다.
- **실제 표면:** `.omo/evidence/spec037/prd020/qa-native-report-final.txt`는 macOS native compiled REPL에서 실제 `read_file`·`write_file`로 확인한 기록이다(`native-qa.sh`로 재현).
  - 확인 항목: 미독·읽기 실패 출처, 미해결 표시, `/method status` 상세, 읽은 뒤 파일 변경 감지, `/method revise`로 만든 2판의 재읽기와 연결, 승인 후 파일 보고서인 3판, history.
  - 이 QA에서 승인 대기 질문 텍스트(인자 JSON 포함)를 보고서 본문으로 오해석하던 결함을 찾았다. 실패를 재현한 테스트 뒤에 고쳤다(`qa-fix-before-approval-prompt.log`). 이제 승인·질문 대기 중인 응답은 보고서 본문으로 보지 않는다.
- **표시 보강:** 새 판에서 새로 생기거나 바뀐 결론을 최대 5개까지 연결 여부와 함께 보여 준다. 자료 사이의 내용 충돌은 runtime이 판단하지 않고, 모델이 표시한 `[conflict: ...]`만 드러낸다는 한계를 USAGE에 명시했다.
- **1차 전체 검증(Oracle 검토 전 코드):** `cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast`를 실행했다(`.omo/evidence/spec037/prd020/workspace-test-before-oracle1.log`, `count-tests.py`, 1,358초).
  - 전체 실행 결과는 suite 325개 기준 **3,189 passed / 2 failed / 13 ignored**다. PRD010 종료 시점 3,178 대비 통과 수는 +11이다.
  - 실패 하나는 baseline과 같은 Spec034 fixture 테스트(`mismatch_uses_isolated_tree`)다.
  - 다른 하나는 `shacs-projection`의 `spec030_release_runner_rejects_unbound_cleanup_receipts`(`SourceMismatch`)다. 이 테스트는 실행 중 저장소 원본이 바뀌면 실패한다. 실행 도중인 06:24:28에 저장소 소유자가 다른 작업으로 `intent.md`를 고쳤다. 이 변경은 Spec037 작업과 무관하며 건드리지 않았다.
  - 이 실패는 전체 실행과 별도로, 같은 코드에서 해당 target만 다시 실행해 20/20 통과를 확인했다(`spec030-release-runner-rerun.log`). 전체 실행 수치에 합산하지 않는다.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.

### Oracle 1차 검토 반영 (BLOCKED → 수정)

독립 Oracle gate가 BLOCKED로 판정한 네 항목과 문서 지적을 고쳤다. 각 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle1-fix-before-agentloop.log`: 5개 실패, `oracle1-fix-before-unit.log`: 3개 실패). 수정 후 결과는 `oracle1-fix-after-agentloop.log`(51/51)와 `oracle1-fix-after-unit.log`(24/24)다.

- **실패한 `web_fetch`가 읽은 출처로 기록됨(P0):** `web_fetch`는 URL 검사·네트워크·redirect·HTTP 오류를 `{"error": ...}` JSON으로 돌려준다. 이전 판별은 `Error` 같은 접두사만 봤다.
  - 공통 판별 함수 `failure`를 만들었다. 오류 접두사와 최상위 `error`가 있는 JSON 결과를 모두 실패로 본다. 읽기·쓰기·편집 기록과 사전 거절 기록이 이 함수를 같이 쓴다.
  - 실패 사유는 JSON의 `error` 문구로 표시한다(예: `HTTP status 404`, `URL validation failed: ...`).
- **다른 경로를 같은 출처로 연결함(P0):** 이전에는 경로 끝부분이 같으면 연결했다. 그래서 `notes.md`를 읽고 `imaginary/notes.md`를 인용해도 연결됐다.
  - 이제 인용은 읽을 때 쓴 위치와 정확히 같아야 한다. 앞의 `./`, 감싼 따옴표, 끝의 `/`만 정규화한다.
  - 작업 폴더 파일은 실제로 해석된 경로의 작업 폴더 기준 상대 경로와 절대 경로도 같은 출처로 인정한다.
  - 비교는 256자로 줄이기 전 전체 값의 digest(`match_keys`, citation `key`)로 한다. 그래서 앞부분이 같은 긴 URL끼리 섞이지 않는다.
- **표시 없는 문장이 있어도 `source-linked`(P1):** 새 상태 `partly cited`를 추가했다. 인용이 모두 연결돼도 표시 없는 문장 줄이 있으면 `source-linked`가 아니다.
  - 이 경우 "claim-source linkage" 확인은 `not verified`로 남는다. 근거 목록에 해당 줄을 넣는다.
  - 인용 문제와 표시 없는 줄이 함께 있으면 `citation problems` 사유에 줄 수를 덧붙인다.
  - 표시 없는 줄은 결과에 수와 처음 5개를, `/method status`에 전체(최대 32개)를 보여 준다.
  - 네 단어 미만 줄, 제목, `:`로 끝나는 머리말 줄은 세지 않는다. 보고서 protocol에는 파일 저장 시 답변을 "Saved: report.md"처럼 짧게 쓰라는 안내를 추가했다.
- **수정판에서 빠진 결론이 보이지 않음(P1):** 이전 판에 있었지만 새 판에 없는 결론을 `removed_claims`로 기록한다. 결과에 "N earlier conclusions removed or reworded"와 처음 5개를 표시한다. 새 판의 글이 아직 없으면 세지 않는다.
- **수치 표기(문서):** 위 1차 전체 검증 절에서 전체 실행 수치(3,189 / +11)와 별도 재실행(20/20)을 분리했다.
- **함께 보강한 경로:**
  - hook이 어떤 문구로 막았든 막힌 읽기는 실패로 기록한다. 이전에는 막힌 메시지가 `Error`로 시작할 때만 실패였다. runtime이 오류로 분류한 호출도 실패로 기록한다.
  - 사용자가 승인을 거부한 `read_file`·`web_fetch`를 "Sources that could not be read"에 `Permission denied by user.`로 남긴다.
  - 파일 변경 재확인 때 저장된 경로가 더 이상 자기 자신으로 해석되지 않으면(예: symlink로 교체) 따라가 읽지 않고 변경으로 본다.
- **새 테스트(8개):**
  - 단위 3개: JSON 오류 판별, 긴 URL 구분, hook 차단 읽기.
  - AgentLoop 5개: 승인 후 `web_fetch` JSON 오류, 승인 거부된 `web_fetch`, 다른 경로 인용, 표시 없는 문장, 수정판의 빠진 결론.
  - 기존 테스트 중 표시 없는 문장이 있는 보고서를 `source-linked`로 기대하던 것은 `partly cited`로 바꿨다. 경로 끝부분 비교를 기대하던 단위 검사는 정확 비교 검사로 바꿨다.
- **실제 표면 재확인:** `.omo/evidence/spec037/prd020/qa-native-report-final.txt`를 새 시나리오로 다시 만들었다(`native-qa-setup.py`로 격리 QA 폴더 생성, `native-qa.sh`로 실행). macOS native compiled REPL과 실제 `read_file`·`web_fetch`·`write_file`을 썼다.
  - 실제 `web_fetch`가 loopback 주소를 막은 JSON 오류를 읽기 실패로 표시한다.
  - `imaginary/notes.md` 인용은 never read다. `Here is the report:` 머리말은 세지 않고 표시 없는 결론 문장만 나열한다.
  - 2판에서 빠진 결론 5개를 보여 준다. 파일 보고서의 짧은 답변 `Saved: report.md`는 세지 않는다.
  - 승인을 거부한 `web_fetch`는 읽지 못한 출처로 남는다.
- **2차 전체 검증(Oracle 1차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test-oracle1.log`, `count-tests.py`, 1,325초).
  - suite 325개 기준 **3,198 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD010 종료 시점 3,178 대비 +20이다. PRD020의 새 테스트 20개(1차 12개, Oracle 1차 반영 8개)와 일치한다.
  - 1차 실행에서 소유자 편집 때문에 실패했던 `spec030_release_runner_rejects_unbound_cleanup_receipts`도 이번 전체 실행에서 통과했다.
  - 당시 대상 테스트는 spec037 51/51(`oracle1-fix-after-agentloop.log`), core 단위 24/24(`oracle1-fix-after-unit.log`)였고 fmt·clippy도 통과했다. `targeted-*.log`, `fmt.log`, `clippy.log`는 아래 최종 검증 때 다시 만들었다.

### Oracle 2차 검토 반영 (BLOCKED → 수정)

두 번째 독립 Oracle gate가 BLOCKED로 판정한 세 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle2-fix-before-agentloop.log`: 2개 실패, `oracle2-fix-before-unit.log`: 1개 실패). 수정 후 결과는 `oracle2-fix-after-agentloop.log`(53/53)와 `oracle2-fix-after-unit.log`(25/25)다.

- **수정판 비교에서 출처 표시 없는 결론이 빠짐:** 이전 판과 새 판을 비교할 때 표시가 있는 결론만 봤다. 그래서 실제 QA에서 지운 `"The outlook looks stable overall."`이 "removed"에 나타나지 않았다.
  - 이제 비교 대상은 표시가 있는 결론과 표시 없는 문장 줄을 합친 목록이다(`ReportVerdict::conclusions`). 새 판의 표시 없는 줄은 "New or changed"에 `(no source marker)`로, 빠진 줄은 "Removed or reworded"에 나타난다.
  - 결과에는 처음 5개만 보이므로 `/method status`에 빠진 결론 전체 목록을 추가했다.
- **symlink로 읽은 출처가 다른 파일을 가리켜도 연결 유지:** 읽을 때 canonical 대상 경로만 저장했다. 그래서 symlink가 다른 파일로 바뀌어도 원래 대상이 그대로면 "변경 없음"이었다.
  - 이제 읽을 때 쓴 경로(작업 폴더 기준으로 이은 절대 경로, `requested_path`)도 저장한다. 다시 확인할 때 그 경로가 여전히 같은 canonical 대상으로 해석되는지 먼저 본다. 아니면 따라가 읽지 않고 "source changed after it was read"로 표시한다.
- **쉼표로 출처를 나눔:** 문서는 `;`만 구분자로 정했지만 파서는 쉼표로도 나눴다. 그래서 `data,2026.md`나 쉼표가 든 URL을 정확히 인용해도 연결되지 않았다. 이제 `;`만 구분자다. protocol과 USAGE에도 명시했다.
- **새 테스트(3개):** AgentLoop 2개(표시 없는 줄의 수정판 비교, symlink 재지정), 단위 1개(쉼표가 든 파일 경로와 URL).
- **한계 명시:** 024 workflow로 실행된 보고서 요청은 child가 읽은 자료와 파일로 쓴 보고서를 검사한다. workflow 진행 요약은 보고서 본문으로 보지 않는다. 따라서 파일 보고서가 없으면 `not written`으로 남는다(성공을 합성하지 않는 쪽). USAGE에 적었다.
- **실제 표면 재확인:** `qa-native-report-final.txt`를 최종 코드로 다시 만들었다. 2판 결과는 "6 earlier conclusions removed or reworded"로, 지운 표시 없는 줄까지 센다.
- **3차 전체 검증(Oracle 2차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test-oracle2.log`, `count-tests.py`, 1,334초).
  - suite 325개 기준 **3,201 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 `mismatch_uses_isolated_tree`다. PRD010 종료 시점 3,178 대비 +23(1차 12개, Oracle 1차 반영 8개, Oracle 2차 반영 3개)이다.
  - 당시 대상 테스트는 spec037 53/53(`oracle2-fix-after-agentloop.log`), core 단위 25/25(`oracle2-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 3차 검토 반영 (BLOCKED → 수정)

세 번째 독립 Oracle gate가 BLOCKED로 판정한 두 항목을 고쳤다. 검토자가 검토 중 짚었지만 차단하지 않은 한도 초과 경로도 같은 원칙으로 함께 막았다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle3-fix-before-agentloop.log`: 1개 실패, `oracle3-fix-before-unit.log`: 2개 실패). 수정 후 결과는 `oracle3-fix-after-agentloop.log`(54/54)와 `oracle3-fix-after-unit.log`(28/28)다.

- **짧은 문장과 한국어 결론이 출처 없는 줄 검사에서 빠짐:** 공백 기준 네 단어 미만 줄을 세지 않아서 `비용은 감소했다`나 `Costs fell.` 같은 결론이 빠졌고, 보고서가 `source-linked`가 될 수 있었다.
  - 이제 비어 있지 않은 문장 줄은 모두 센다. 세지 않는 줄은 제목(`#`), `:`로 끝나는 머리말, 구분선·표 구분 줄·코드 펜스 같은 서식 줄, 파일 저장 안내 `Saved: <파일>` 한 줄뿐이다.
- **실패·취소 때 runtime 안내문을 보고서로 해석함:** `interrupt`만 없으면 최종 내용을 모두 보고서 본문으로 읽었다. 그래서 치명적 도구 오류 문구에 `[source: ...]`가 들어 있으면 실패한 요청에도 연결 판정이 붙을 수 있었다.
  - 이제 `report_answer`가 `stop_reason == "completed"`이고 오류와 interrupt가 없는 provider 최종 답변만 돌려준다. 도구 오류·취소·반복 한도·빈 응답·provider 오류의 안내문은 보고서 본문이 아니다. 파일 출력은 기존처럼 도구 관측으로 검사한다.
- **검사 한도를 넘은 부분이 조용히 빠짐(함께 보강):** 출력당 결론 32개·판정당 64개, 결론당 인용 32개, 출력당 본문 65,536자, 판당 출력 32개를 넘는 부분은 검사하지 않고 버렸다. 그래서 한도 뒤의 가짜 인용이 판정에 반영되지 않을 수 있었다.
  - 이제 넘은 부분을 `unchecked`로 센다. 하나라도 있으면 `source-linked`가 아니며("partly cited" 또는 "citation problems"), 결과에 "Parts beyond the checking limits (not checked): N"을 표시한다.
- **성능 보강:** 긴 줄(수만 자)에 대한 redaction이 줄마다 수십 초 걸렸다. 저장 한도에 1,024자를 더한 앞부분만 redaction하도록 `bounded`와 goal `preview`를 고쳤다. 해당 단위 테스트 시간이 115초에서 1초 미만으로 줄었다.
- **새 테스트(4개):** 단위 3개(짧은·한국어 줄과 서식 줄, 검사 한도 초과, 최종 답변 선택 `report_answer`), AgentLoop 1개(도구 오류 문구 속 인용 — `report_terminal.rs`로 옮김).
- **기존 기대 변경:** PRD000 선택 테스트의 보고서 경우는 한 단어 답변 `prepared`도 표시 없는 줄로 세므로, 연결 확인의 근거 목록에 그 줄이 들어가는 것을 허용하도록 바꿨다. 상태는 여전히 모두 `not verified`다.
- **실제 표면 재확인:** `qa-native-report-final.txt`를 최종 코드로 다시 만들었다. 머리말 `Here is the report:`와 파일 저장 안내 `Saved: report.md`는 세지 않고, 표시 없는 결론은 그대로 센다.
- **4차 전체 검증(Oracle 3차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test-oracle3.log`, `count-tests.py`, 1,345초).
  - suite 325개 기준 **3,205 passed / 1 failed / 13 ignored**다. 유일한 실패는 `mismatch_uses_isolated_tree`다. PRD010 종료 시점 3,178 대비 +27이다.
  - 당시 대상 테스트는 spec037 54/54(`oracle3-fix-after-agentloop.log`), core 단위 28/28(`oracle3-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 4차 검토 반영 (BLOCKED → 수정)

네 번째 독립 Oracle gate가 BLOCKED로 판정한 세 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle4-fix-before-agentloop.log`: 1개 실패, `oracle4-fix-before-unit.log`: 2개 실패). 수정 후 결과는 `oracle4-fix-after-agentloop.log`(56/56)와 `oracle4-fix-after-unit.log`(30/30)다.

- **표시만 있는 줄과 빈 표시:** 표시가 있으면 본문이 실제 문장인지 보지 않고 결론으로 만들었다. 그래서 `Unsupported claim here [source: ]`가 무출처로 세지지 않았고, `[source: notes.md]`만 있는 글도 완성된 보고서로 판정됐다.
  - 이제 표시를 지운 본문이 비어 있거나 서식뿐이면 결론으로 만들지 않는다. 표시가 모두 비어 있는 문장(`[source: ]`)은 표시 없는 줄로 센다. 표시만 있는 글은 `not written`이다.
- **`/method history`와 `/method accept`의 오래된 판정:** `/method status`만 파일 변경을 다시 확인했다. 그래서 보고서 뒤 원본이 바뀐 뒤 곧바로 history를 보면 예전 `source-linked`가 나왔고, accept는 그 오래된 값을 수용 시점 판정으로 기록했다.
  - 이제 history는 표시할 모든 요청을, accept는 대상 요청을 먼저 다시 확인한다. 다음 turn의 모델 문맥(projection)도 활성 요청을 다시 확인한 뒤 만든다.
- **준비 전 취소·준비 거절된 읽기의 위치 손실:** 준비 전 취소의 skipped 이벤트에는 인자가 없었고, 준비 오류 이벤트에는 원래 호출이 연결되지 않았다. 그래서 실제 경로 대신 `<read call skipped/rejected before it ran>`이 기록됐다.
  - 이제 runner가 skipped 이벤트에 관측용 인자(`observable_tool_arguments`, redaction 적용)를 넣고, 준비 오류 이벤트에 원래 호출을 연결한다. 관측기는 후보가 없을 때 그 인자로 `read_file`·`web_fetch` 호출을 복원해 실제 위치의 실패로 기록한다. 인자를 알 수 없을 때(`exec`, `write_file`처럼 인자를 전부 가리는 도구 포함)만 sentinel을 쓴다.
  - 승인 대기 중 건너뛴 읽기는 이미 후보로 등록돼 위치가 유지됐다. 이 경로도 회귀 테스트로 고정했다.
- **문서(비차단 관찰):** 글이 없으면 읽기 결과와 관계없이 먼저 `not written`이라는 상태 우선순위를 USAGE와 구현 메모에 적었다.
- **새 테스트(4개):** 단위 2개(표시만 있는 줄·빈 표시, 준비 전 취소·준비 거절의 위치 유지), AgentLoop 2개(history·accept의 재확인, 승인 대기 중 건너뛴 읽기의 위치 유지 — 뒤의 것은 수정 전에도 통과하는 회귀 고정용).
- **실제 표면 재확인:** `qa-native-report-final.txt`를 최종 코드로 다시 만들었다. history가 1판에 대해 "4 of 4 citations ... source changed after it was read"를 보여 주는 것은 표시 전 재확인의 결과다.
- **5차 전체 검증(Oracle 4차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test-oracle4.log`, `count-tests.py`, 1,301초).
  - suite 325개 기준 **3,209 passed / 1 failed / 13 ignored**다. 유일한 실패는 `mismatch_uses_isolated_tree`다. PRD010 종료 시점 3,178 대비 +31이다.
  - 당시 대상 테스트는 spec037 56/56(`oracle4-fix-after-agentloop.log`), core 단위 30/30(`oracle4-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 5차 검토 반영 (BLOCKED → 수정)

다섯 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle5-fix-before-unit.log`: 1개 실패). 수정 후 결과는 `oracle5-fix-after-unit.log`(31/31)와 `oracle5-fix-after-agentloop.log`(56/56)다.

- **출처 표시가 있는 머리말·파일 안내가 보고서 내용이 됨:** 4차 수정은 표시 없는 줄에만 `substantive` 판정을 적용했다. 그래서 파일을 쓰지 않고 `Saved: report.md [source: notes.md]`나 `Here is the report: [source: notes.md]`만 답해도 결론 하나가 생겨 `source-linked`가 될 수 있었다.
  - 이제 표시가 있는 줄도 `substantive`가 아니면(제목, `:`로 끝나는 머리말, `Saved: <파일>` 안내) 보고서 내용으로 세지 않는다(`ReportClaim.statement = false`). 그런 줄만 있으면 `not written`이다.
  - 그 줄의 인용은 여전히 검사한다. 예를 들어 `# Revenue [source: fake.md]`처럼 읽지 않은 자료를 가리키면 `citation problems`다. 수정판 비교(새로 생기거나 빠진 결론)에도 넣지 않는다.
- **새 테스트(1개):** 단위 1개(인용이 붙은 파일 안내·머리말은 `not written`, 인용이 붙은 제목의 가짜 인용은 `citation problems`).
- **실제 표면 재확인:** `qa-native-report-final.txt`를 최종 코드로 다시 만들었다.
- **6차 전체 검증(Oracle 5차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test-oracle5.log`, `count-tests.py`, 1,324초).
  - suite 325개 기준 **3,210 passed / 1 failed / 13 ignored**다. 유일한 실패는 `mismatch_uses_isolated_tree`다. PRD010 종료 시점 3,178 대비 +32다.
  - 당시 대상 테스트는 spec037 56/56(`oracle5-fix-after-agentloop.log`), core 단위 31/31(`oracle5-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 6차 검토 반영 (BLOCKED → 수정)

여섯 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 검토자가 검토 중 살폈지만 차단하지 않은 두 경로도 같은 원칙으로 함께 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle6-fix-before-agentloop.log`: 2개 실패, `oracle6-fix-before-unit.log`: 1개 실패). 수정 후 결과는 `oracle6-fix-after-agentloop.log`(57/57)와 `oracle6-fix-after-unit.log`(32/32)다.

- **방식 변경 전에 읽은 자료를 지금 읽은 자료처럼 표시함:** 판정은 현재 revision에서 읽었거나 이전 판에서 가져온 자료만 쓰는데, 요약의 "Sources read"는 모든 성공 자료를 보여 줬다. 그래서 보고서 → 일반 → 보고서로 바꾼 뒤 다시 읽지 않으면, 판정은 `no sources read`인데 같은 결과에 `Sources read: notes.md`가 함께 나왔다.
  - 이제 요약의 읽은 자료·읽지 못한 자료 목록을 판정과 같은 범위로 제한한다. `/method status`는 이전 revision 자료를 `revision N; superseded_not_inherited`로 표시한다.
- **함께 보강: 긴 파일 경로의 보고서 출력이 서로 덮어씀:** 같은 판의 파일 보고서를 256자로 줄인 위치로 구분해, 앞부분이 같은 긴 경로의 두 파일이 하나로 합쳐질 수 있었다. 그러면 앞 파일의 가짜 인용이 판정에서 빠졌다. 이제 줄이기 전 전체 위치의 digest(`ReportOutput.key`)로 구분한다.
- **함께 보강: 수정판의 원래 요청을 모델에 알리지 않음:** 수정판 요청의 원래 목적(`purpose`)을 저장만 하고 쓰지 않았다. 이제 모델 문맥에 새 판 번호, 이전 요청 ID, 원래 요청과 출처 재확인 규칙을 넣는다.
- **한계 명시:** 수정판 비교는 저장된 결론 문장(앞 256자)으로 한다. 그 뒤만 바뀐 긴 결론은 바뀐 것으로 잡히지 않을 수 있다고 USAGE에 적었다.
- **새 테스트(2개, 기존 1개 보강):** AgentLoop 1개(방식 변경 전 자료의 표시 범위), 단위 1개(앞부분이 같은 긴 경로의 파일 보고서), 기존 수정판 시나리오에 모델 문맥 확인을 추가했다.
- **최종 전체 검증(Oracle 6차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd020/workspace-test.log`, `count-tests.py`, 1,317초).
  - suite 325개 기준 **3,212 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD010 종료 시점 3,178 대비 +34다. PRD020의 새 테스트 34개(1차 12개, Oracle 1차 8개, 2차 3개, 3차 4개, 4차 4개, 5차 1개, 6차 2개)와 일치한다.
  - 대상 테스트: `targeted-spec037.log` 57/57, `targeted-lib.log` 32/32(`runtime::task_method`, `runtime::agent_loop::method_finish`), `targeted-cli-command.log` REPL 4/4·command 3/3.
  - `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`, 기존 `imap-proto` future-incompatibility 경고만 있음)도 통과했다.
  - `qa-native-report-final.txt`도 최종 코드로 다시 만들었다.

## 종료 확인 (2026-10-22)

- 범위: 보고서 방식 요청에서 실제로 읽은 자료(`read_file`, `web_fetch`)와 보고서 결론을 같은 요청에 결속한다. 결론별 인용이 이 요청에서 읽은 자료에 연결되는지 판정하고, 답과 판정·읽은 자료·연결되지 않은 결론을 먼저 보이며 자료별 기록과 결론별 상태는 `/method status`로 보인다. `/method revise`로 같은 목적의 새 판을 만들고 이전 판의 자료는 다시 확인한다. 계산·이미지·복합 결과는 포함하지 않는다.
- 최종 검증: 전체 workspace는 suite 325개 기준 **3,212 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline과 같은 Spec034 fixture 테스트다. fmt와 workspace clippy `-D warnings`도 통과했다.
- 실제 표면: macOS native compiled REPL과 실제 `read_file`·`web_fetch`·`write_file`로 확인했다(`qa-native-report-final.txt`). 읽기 실패·loopback JSON 오류·허구 경로 인용, 표시 없는 결론, 읽은 뒤 파일 변경, 2판의 재읽기와 빠진 결론, 승인 후 파일 보고서, 승인 거부된 읽기, 표시 전 재확인되는 history를 담았다.
- Oracle 독립 검증: **PASS** (7차).
  - 1–6차에서 다음 결함을 차례로 찾았고, 모두 고친 뒤 수정 전 실패 로그와 회귀 테스트로 고정했다.
    - 실패한 `web_fetch` JSON, hook이 어떤 문구로 막은 읽기, 준비 전 취소·준비 거절된 읽기의 기록과 위치.
    - 경로 끝부분·긴 위치 앞부분이 같은 인용, symlink 재지정, 쉼표가 든 위치의 연결 판정.
    - 표시 없는 짧은·한국어 결론, 빈 표시, 표시만 있는 줄, 인용이 붙은 머리말·파일 안내, 검사 한도를 넘은 부분이 `source-linked`를 만들던 문제.
    - runtime 실패·취소 안내문을 보고서로 읽던 문제와 history·accept·모델 문맥의 오래된 판정.
    - 수정판에서 빠진 결론(표시 없는 줄 포함), 방식 변경 전 자료의 표시 범위, 긴 경로 파일 보고서의 덮어쓰기.
  - 이 PASS는 PRD 030–040의 이미지·복합 결과를 포함하지 않는다.
- 남은 제한:
  - 연결 판정은 인용이 읽은 자료를 가리키는지까지만 확인한다. 결론이 그 자료의 내용과 맞는지는 판단하지 않으며, 자료 사이의 충돌은 모델이 표시한 `[conflict: ...]`만 드러낸다.
  - 수정판 비교는 저장된 결론 문장(앞 256자)으로 한다.
  - 024 workflow로 실행된 보고서 요청은 child가 읽은 자료와 파일로 쓴 보고서만 검사하고, workflow 진행 요약은 보고서 본문으로 보지 않는다.
