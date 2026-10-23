# PRD 030. 생성 이미지 확인과 사용자 수정

Status: Complete (Scoped)

Depends on: [PRD 000](000-method-selection-and-user-override.md) 완료. 뒤 번호 PRD에는 의존하지 않는다.

## Goal

한 이미지 요청의 실제 산출물을 사용자에게 보여주고 요구 조건 확인·수정 요청을 같은 작업으로 이어간다. **Production artifact 저장 경계를 통과한 이미지 생성 성공과 미설정/미지원 실패를 모두 검증해야 종료한다.** 보고서나 계산 방식은 필요하지 않다.

## Scope

1. 선택된 이미지 수행 방식이 [034](../../034-generated-media-and-rich-file-context-expansion/SPEC.md)의 provider capability·credential status·artifact store를 확인하고 실제 생성 결과 또는 unsupported/unavailable/failed를 반환한다.
2. 사용자가 요구한 크기/형식/내용 등 확인 가능한 조건과 생성 artifact의 출처·상태를 함께 보여준다. 확인 가능한 검사는 증거로 남기되 미관측 미감·의미 품질을 자동 PASS라고 주장하지 않는다.
3. 사용자가 결과를 보고 변경을 요청하면 기존 목표·원본 artifact와 새 요청을 결속해 수정 결과를 구분한다. Provider가 지원하지 않는 edit/mask/variation은 다른 동작으로 성공을 합성하지 않는다.

## Non Scope

- Image editor UI, provider 간 edit/streaming 완전 동등성, 보고서·계산·복합 요청.
- 새로운 media root, raw provider payload 영구 저장, 외부 CDN/gallery, 전면 sandbox·privacy 보장.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 3 중 이미지 결과 검토/수정, 범위 4의 이미지 artifact·상세 조회를 소유한다.
- 부모 수용 조건 3의 실제 artifact·사용자 수정·미지원 상태와 조건 5의 이미지 경로를 소유한다.

## Acceptance Criteria

1. 구성된 provider 또는 production 경계에 주입한 결정론적 호환 provider의 제어 입력에서 실제 도구 호출과 artifact commit을 거쳐 이미지 산출물을 만들고, 안전한 참조와 요구 조건 확인 상태를 사용자가 볼 수 있다. 테스트용 빈 응답이나 mock 화면만으로 대체하지 않으며 검사가 불가능한 조건은 미확인으로 남는다.
2. 사용자 수정 요청은 원래 목표와 이전 artifact를 유지하면서 새 artifact/실패 사실을 남긴다. 미지원 edit 계열은 정확한 unsupported로 표시된다.
3. 같은 production 호출·artifact 경계에서 provider 미설정, credential unavailable, 생성 실패, artifact commit 불확실 상태도 검사하고 성공 이미지를 합성하지 않는다. 실제 사용자 표면 또는 호환 provider가 주입된 실행 표면에서 성공과 거절을 각각 확인한다. 외부 계정의 image entitlement는 테스트의 선행 조건이 아니다.

## Closure Evidence

- 생성·검토·수정 요청과 실제 artifact lineage의 사용자-visible 기록.
- 미설정/미지원/실패·미확인 품질의 거절 테스트와 034 owner fact 소비 근거.

PRD 030은 **한 이미지 결과의 검토·수정 흐름**으로 닫힌다. 뒤 번호 PRD의 복합 결과나 별도 editor가 필요하지 않다.

## 구현 메모 및 검증 근거 (2026-10-22)

아래는 구현·실행 기록이다. 완료 판정은 독립 Oracle gate 후 종료 확인 절에 기록한다.

- **실제 산출물 확인(`task_method_image*.rs`):** 이미지 방식 요청에서 `image_generate` 호출마다 옵션(size, format, count, quality, background), 프롬프트(redaction 후 256자), 결과를 같은 요청의 호출 기록으로 남긴다. selection revision과 이미지 판에 귀속한다.
  - 생성으로 인정하려면 도구 결과가 보고한 파일이 media root 안에 실제로 있고, 그 파일의 sha256이 보고된 값과 같아야 한다. media root 밖이거나 digest가 다르면 `save not confirmed`이며 이미지를 보여 주지 않는다.
  - 읽은 파일 머리 부분에서 형식(PNG·JPEG·WebP)과 픽셀 크기를 직접 잰다. 읽는 파일은 canonical 경로가 media root 안이고 64 MiB 이하일 때만이다.
  - 관측 경로는 PRD010/020과 같다. 실행된 호출, hook 차단, 준비 단계 거절, 취소·건너뜀, 승인 후 실행, 사용자 거부가 모두 호출 기록으로 남는다.
- **034 owner fact 소비:** 이미지 생성·저장은 Spec034 `image_generate`와 그 저장 경계를 그대로 쓴다. 이 PRD는 결과만 소비한다.
  - 일반 provider 결과의 `artifacts[]`(`mediaRef`, `sha256`, `providerId`, `modelId`)를 읽는다.
  - `openai_codex` 결과는 `ArtifactStore`가 commit한 `GeneratedArtifactRef`(`mediaRootRelativePath`, `sha256`)를 읽는다.
  - Spec034 도구가 분류한 오류 범주를 상태로 옮긴다: `unsupported/config failure`(자격 증명 없음·provider 없음은 `unavailable`, 그 밖은 `unsupported`), `provider failure`(`failed`), `media write failure`(`failed`, 저장 실패).
  - provider 호출·재시도·저장 구현은 새로 만들지 않았다.
- **요구 조건 확인:** 요청 문장에서 크기(`WxH`), 형식(`png`, `jpeg`/`jpg`, `webp`), 장수(`N images`, `N장`)를 읽어 저장된 파일과 비교한다. 결과는 `met`·`not met`·`not checked`다.
  - 하나라도 `not met`이면 "stated requirements" 확인은 `failed`다. 모두 맞아도 내용·모양은 확인하지 않았으므로 `not verified`로 남긴다.
  - 결과에는 항상 "content and look — not checked automatically; please review the image"를 붙인다. 미관측 품질을 PASS로 표시하지 않는다.
- **결과 우선 표면:** 이미지 방식의 답은 runtime이 쓴다. 모델 답변은 성공·실패·호출 없음 어느 경우에도 보여 주지 않는다(아래 Oracle 4차 검토 반영). 그 다음에 판정, 저장된 이미지의 상대 참조와 형식·크기·바이트 수·digest 앞부분·provider/model, 요구 조건 결과, 사용자 수용 여부를 보여 준다. `/method status`는 호출별 옵션·프롬프트·전체 digest·저장 확인 여부·오류와 이전 판 이미지를 보여 준다.
- **사용자 수정(`/method revise`):** 마지막으로 끝난 보고서 또는 이미지 요청을 다음 메시지의 새 판으로 만든다. 이미지라면 원래 요청(purpose)과 이전 판에서 저장이 확인된 이미지 참조를 가져온다.
  - 새 판은 수정된 요청으로 새로 생성한 결과다. 이미지 편집·마스크·변형은 지원하지 않으므로 "image edit, mask and variation are unsupported here, so an earlier image is never edited"로 표시한다. 새 이미지가 저장되지 않았으면 "No new image was saved in this version"으로 표시한다.
  - 요구 조건은 원래 요청의 조건에 새 메시지의 조건을 덮어써서 다시 확인한다. 이전 판의 확인 결과는 물려받지 않는다.
  - 사용자 아닌 turn과 진행 중 요청에서는 거절한다.
- **미설정·실패 상태:**
  - `image_generate`가 등록되지 않으면(side-effect 도구 꺼짐, `tools.imageGeneration.enable` 꺼짐) PRD000 가용성 판정으로 provider 호출 전에 막는다.
  - 이미지 생성을 지원하지 않는 provider를 설정하면 Spec034 설정 단계가 CLI 시작을 거절한다("provider ... does not support image_generation").
  - 자격 증명 없음은 `unavailable`, provider 오류는 `failed`, 저장 실패는 `failed`, 저장 확인 불가는 `save not confirmed`, 사용자 거부·취소·hook 차단은 `not generated`다. 어느 경우에도 이미지나 요구 조건 충족을 합성하지 않는다.
- **함께 고친 점:** 관측 효과의 결과 분류에서 OS 권한 오류(`Permission denied (os error 13)`)를 사용자·gate 거부로 보던 문제를 고쳤다. 이제 `(os error` 오류는 `Failed`다. 실제 QA의 저장 실패 기록에서 발견했다.
- **기존 기대 변경:** PRD000 테스트 중 "이미지 방식은 validation claim이 비어 있다"는 기대를 "not verified claim 3개"로 바꿨다. `/method revise` 대상이 없을 때 문구는 "No completed report or image request to revise."로 바꿨다. PRD030 요구에 따른 의도적 변경이다.
- **테스트:**
  - 단위 6개: 요구 조건 읽기와 덮어쓰기, PNG·JPEG·WebP 머리 읽기, media root·digest 확인, 실패 범주와 효과 분류, 측정과 미검증 내용, 수정판 계보.
  - AgentLoop 시나리오 7개: 생성·측정·검토 남김, 측정 불일치, 미설정·실패·저장 실패·저장 확인 불가, 사용자 거부, 수정판(원래 목표·이전 이미지 유지, 편집 미지원 표시), `ArtifactStore` commit 확인, `/method revise` 대상 선택.
  - 실제 CLI 2개(`crates/shacs-cli/tests/it/spec037_image_repl.rs`): 운영 설정의 `openai` image provider가 로컬 호환 HTTP API를 호출해 실제 파일을 저장하고 확인하는 경로, 같은 경계에서 provider 오류와 자격 증명 없음.
- **실제 표면:** macOS native compiled REPL에서 운영 설정의 `openai` image provider가 로컬 결정론적 호환 API(`fake-image-api.py`, 실제 PNG 반환)를 부르게 해 확인했다(`native-qa-setup.py` + `native-qa.sh`로 재현). 외부 계정의 image entitlement는 쓰지 않았다.
  - `qa-native-image-main.txt`: 요청 크기·형식 충족(256x192 png), `/method status` 상세, `/method revise`로 만든 2판(128x128, 원래 요청의 형식 유지, 이전 이미지 참조와 편집 미지원 표시), 측정 불일치(512x512 요청에 64x64 → `not met`), provider 오류(500 → `failed`), 저장 실패(쓰기 금지 폴더 → `failed`, 관측 효과도 `Failed`), 승인 거부(`not generated`), history.
  - `qa-native-image-no-credential.txt`: 자격 증명 없음 → `unavailable`. `qa-native-image-not-configured.txt`: 이미지 생성 꺼짐 → provider 호출 전 차단. `qa-native-image-unsupported-provider.txt`: 이미지 생성을 지원하지 않는 provider → CLI 시작 거절(EXIT=1).
  - `qa-native-image-artifacts.txt`: 저장된 PNG를 `file(1)`과 `shasum`으로 따로 확인했다. 크기와 digest가 화면 표시와 같다. `fake-image-api-requests.log`는 provider가 받은 요청(크기·형식, 프롬프트와 키는 기록하지 않음)이다.
- **1차 전체 검증(Oracle 검토 전 코드):** `cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast`를 실행했다(`.omo/evidence/spec037/prd030/workspace-test-before-oracle1.log`, `count-tests.py`, 1,369초).
  - suite 326개 기준 **3,227 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD020 종료 시점 3,212 대비 +15다. PRD030의 새 테스트 15개(단위 6, AgentLoop 7, 실제 CLI 2)와 일치한다. suite 하나(`spec037_image_repl`)가 늘었다.
  - 당시 대상 테스트는 spec037 64/64, core 단위 38/38, 이미지 CLI 2/2였고 fmt·clippy도 통과했다.

### Oracle 1차 검토 반영 (BLOCKED → 수정)

첫 번째 독립 Oracle gate가 BLOCKED로 판정한 세 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle1-fix-before-unit.log`: 2개 실패, `oracle1-fix-before-agentloop.log`: 1개 실패). 수정 후 결과는 `oracle1-fix-after-unit.log`(41/41)와 `oracle1-fix-after-agentloop.log`(65/65)다.

- **`/method revise`가 실행 전부터 새 이미지와 연결을 단정함:** 명령 응답이 "A new image is generated and linked to the earlier one"이라고 말했고, 이전 요청에 저장된 이미지가 없어도 그렇게 말했다.
  - 이제 "Nothing is generated yet: the next turn asks for a new image, and only a saved image counts"라고 조건부로 알린다. 저장된 이전 이미지가 있으면 "If a new image is saved, it is linked to the earlier image …", 없으면 "… has no saved image to link"다. 결과 요약의 수정 안내 문구도 같은 뜻으로 바꿨다.
- **같은 판에서 성공 뒤 실패해도 앞의 성공을 결과로 유지함:** 판정이 마지막 성공 호출을 먼저 찾아서, 그 뒤의 실패·거부·자격 증명 없음·저장 불확실을 무시했다.
  - 이제 가장 최근 호출이 판정을 정한다. 앞서 저장된 이미지는 현재 결과가 아니라는 설명과 함께 "Earlier saved image in this request (not the current result)"로만 보여 준다. 다시 저장에 성공하면 회복한다. 수정판의 계보는 마지막으로 저장이 확인된 이미지(`saved_artifacts`)를 쓴다.
- **8장을 넘는 결과의 장수 판정:** 확인 전에 8장으로 자르고 그 수로 장수를 판정해, 9장 결과가 8장 요구를 `met`으로 보일 수 있었다. 이제 8장을 넘게 보고되면 나머지를 확인할 수 없으므로 `save not confirmed`다.
- **구조:** 헤더 측정 코드를 `task_method_image_measure.rs`로 분리했다(파일 크기 규칙).
- **새 테스트(4개):** 단위 3개(성공 뒤 실패와 회복, 성공 뒤 저장 불확실·거부·자격 증명 없음, 9장 결과), AgentLoop 1개(거부된 첫 요청 뒤 `/method revise` 문구).
- **2차 전체 검증(Oracle 1차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle1.log`, `count-tests.py`, 1,343초).
  - suite 326개 기준 **3,231 passed / 1 failed / 13 ignored**다. 유일한 실패는 `mismatch_uses_isolated_tree`다. PRD020 종료 시점 3,212 대비 +19다.
  - 당시 대상 테스트는 spec037 65/65(`oracle1-fix-after-agentloop.log`), core 단위 41/41(`oracle1-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 2차 검토 반영 (BLOCKED → 수정)

두 번째 독립 Oracle gate가 BLOCKED로 판정한 세 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle2-fix-before-agentloop.log`, `oracle2-fix-before-cli.log`, `oracle2-fix-before-unit.log`: 각 1개 실패). 수정 후 결과는 `oracle2-fix-after-agentloop.log`(66/66), `oracle2-fix-after-cli.log`(이미지 CLI 2/2·REPL 4/4), `oracle2-fix-after-unit.log`(42/42)다.

- **실패한 이미지 요청이 모델의 "Here is your image."로 시작함:** footer는 `failed`·`unavailable`이라고 말하지만 답의 첫 줄이 성공을 주장했다.
  - 이제 이미지 방식 요청에서 판정이 `generated`가 아니면 모델 답변을 앞에 내세우지 않는다(`guarded_answer`). `image_generate`를 불렀는데 저장된 새 이미지가 없으면 답변을 "No new image was saved for this request (image: …)"로 바꾸고, 원래 답변은 `withheld_reply`로 `/method status`에 남긴다. 한 번도 부르지 않았으면 답변 앞에 "No image has been generated in this request yet … not backed by a saved image"를 붙인다(질문이나 설명은 그대로 보이게).
  - 승인·질문 대기 화면은 바꾸지 않는다(6차 반영 후 이미지 요청에서는 질문 도구를 쓸 수 없다). 복합 요청은 PRD040 범위다.
- **보존 한도를 넘으면 마지막 저장 이미지의 계보가 사라짐:** 호출 기록 16개 한도로 오래된 성공 호출이 밀려나면 `/method revise`가 이전 이미지를 찾지 못했다. 이제 마지막으로 저장이 확인된 이미지를 `last_saved`로 따로 보관하고, 판정 설명·요약·수정판 계보가 이를 쓴다.
- **문서의 저장 범위 축소 설명:** USAGE가 session metadata에 "참조와 digest, 잰 값만" 저장한다고 했다. 실제 저장 항목(옵션, redaction한 프롬프트, 상태·오류, 호출 ID·시각, artifact ID, provider/model, 보류한 답변)으로 고치고, 이미지 바이트와 provider 원본 응답은 저장하지 않는다고 한정했다.
- **새 테스트(2개, 기존 2개 보강):** 단위 1개(보존 한도를 넘는 실패 뒤 계보 유지), AgentLoop 1개(이미지 호출 없는 답변 앞 표시). 기존 실패 시나리오(AgentLoop)와 실제 CLI 실패 시나리오에 "Here is your image."가 보이지 않는다는 확인을 더했다.
- **3차 전체 검증(Oracle 2차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle2.log`, `count-tests.py`, 1,355초).
  - suite 326개 기준 **3,233 passed / 1 failed / 13 ignored**다. 유일한 실패는 `mismatch_uses_isolated_tree`다. PRD020 종료 시점 3,212 대비 +21이다.
  - 당시 대상 테스트는 spec037 66/66(`oracle2-fix-after-agentloop.log`), core 단위 42/42(`oracle2-fix-after-unit.log`)였고 fmt·clippy도 통과했다.

### Oracle 3차 검토 반영 (BLOCKED → 수정)

세 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle3-fix-before-agentloop.log`: 1개 실패). 수정 후 결과는 `oracle3-fix-after-agentloop.log`(67/67)와 `oracle3-fix-after-unit.log`(42/42)다.

- **원래 성공 문구가 진행 출력과 대화 기록에 남음:** 2차 수정은 최종 답만 바꿨다. 그래서 WebSocket의 기본 진행 출력(`sendProgress`)이 판정 전에 모델의 "Here is your image." text delta를 그대로 보냈고, session에 저장된 assistant 메시지도 원문이었다.
  - 이제 이미지 방식 turn에서는 provider의 text delta를 진행 출력으로 내보내지 않는다(`hold_answer_stream`; 다른 진행 이벤트는 그대로). 판정이 끝난 답만 보낸다.
  - 답을 바꾸면 session의 해당 assistant 메시지도 같은 답으로 바꾼다. 원문은 `withheld_reply`에만 남는다.
- **새 테스트(2개):** AgentLoop 2개. 하나는 text delta를 흘리는 provider로 실패 응답이 진행 출력과 저장된 대화 기록 어디에도 나오지 않음을 확인한다. 다른 하나는 이미지가 아닌 요청의 진행 출력이 그대로 흐르는지 확인한다.
- **전체 실행에서 잡은 회귀:** 첫 수정본은 진행 출력을 막지 않는 요청에서도 callback을 꺼내 버려, 일반 요청의 provider 진행 출력이 사라졌다(`workspace-test-oracle3-stream-regression.log`: 기존 테스트 4개 실패). 이미지 요청일 때만 callback을 감싸도록 고치고 위 두 번째 회귀 테스트로 고정했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`를 3차 반영 코드로 다시 만들었다(4차 반영 후 다시 만들었다. 아래 참조). `qa-native-image-history-check.txt`는 저장된 대화 기록에서 보류한 모델 답변이 metadata(`withheld_reply`)에만 있고 assistant 메시지에는 runtime 문구만 있음을 확인한 결과다.
- **4차 전체 검증(Oracle 3차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle3.log`, `count-tests.py`, 1,299초).
  - suite 326개 기준 **3,235 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다. 위 회귀로 실패했던 기존 테스트 4개도 통과했다.
  - PRD020 종료 시점 3,212 대비 +23이다. PRD030의 새 테스트 23개(1차 15개, Oracle 1차 4개, 2차 2개, 3차 2개)와 일치한다.
  - 대상 테스트: `targeted-spec037.log` 68/68, `targeted-lib.log` 42/42(`runtime::task_method`, `runtime::agent_loop::method_finish`), `targeted-cli-command.log` 이미지 CLI 2/2·REPL 4/4·command 3/3.
  - `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`, 기존 `imap-proto` future-incompatibility 경고만 있음)도 통과했다.

### Oracle 4차 검토 반영 (BLOCKED → 수정)

네 번째 독립 Oracle gate가 BLOCKED로 판정한 두 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle4-fix-before-core.log`: 3개 실패, `oracle4-fix-before-cli.log`: 1개 실패). 수정 후 결과는 `oracle4-fix-after-agentloop.log`(71/71), `oracle4-fix-after-unit.log`(42/42), `oracle4-fix-after-cli.log`(이미지 CLI 2/2·REPL 4/4·command 3/3)다.

- **생성에 성공하면 모델의 내용 주장을 그대로 보여 줌:** 3차까지는 판정이 `generated`이면 모델 답변을 그대로 냈다. 실제 QA에서 호환 provider는 늘 빨간 그림을 만드는데도 답은 "Here is the blue version.", "Here is the cat."이라고 했다. 내용은 확인하지 않은 조건이다.
  - 이제 이미지 방식의 답은 판정과 상관없이 runtime이 쓴다(`guarded_answer`). 모델 답변은 보여 주지 않는다. 단순히 경고를 앞에 붙이면 바로 뒤의 반대 주장이 남으므로 그렇게 하지 않았다.
  - 저장 확인: "Image saved and checked on disk (see below)." 요구 조건 불일치가 있으면 "Image saved, but not every stated requirement is met (see below)." 둘 다 내용과 모양은 직접 확인해 달라고 덧붙인다.
  - 호출 뒤 저장된 새 이미지 없음: "No new image was saved for this request (image: …)". 호출 없음: "No image was generated for this request (image: not generated): no image_generate call ran." 다시 요청하거나 `/method use general`로 일반 답을 받는 방법을 안내한다.
  - provider 오류(`finish_reason: error`)처럼 정상 답 없이 끝난 turn은 "The turn ended early (…)"를 덧붙인다. 이때 최종 문장이 provider 문장일 수 있으므로 역시 보여 주지 않는다.
  - 원문은 진단용 `withheld_reply`(redaction 후 512자)에만 남기고 `/method status`에서도 보여 주지 않는다. 승인·질문 대기와 entrusted work 제안 화면은 바꾸지 않는다(6차 반영 후 이미지 요청에서는 질문·제안 도구 자체를 쓸 수 없다. 아래 참조).
  - 모델에게 주는 이미지 protocol에 "사용자는 모델 답변이 아니라 runtime의 기록을 본다"와 "변경 요청은 새 생성이며 편집이 아니다"를 적었다.
- **도구 호출과 함께 온 문장이 대화 기록에 남음:** provider는 `content`와 `tool_calls`를 함께 보낼 수 있다. 승인 대기 중인 이미지 요청의 저장 기록에 "Here is your image" 같은 문장이 남아 다음 모델 문맥으로 들어갔다.
  - 이제 이미지 방식 turn에서 runner가 새로 만든 assistant 메시지의 문장을 session에 붙이기 전에 지운다(`withhold_run_text`). 도구 호출은 그대로 둔다. 길이 초과 복구처럼 중간에 생긴 답 조각도 같이 지운다.
  - 마지막 답 자리에는 사용자에게 보인 runtime 답을 넣는다. 위치는 이번 turn이 붙인 메시지 범위 안에서만 찾는다. 이전 turn의 메시지는 건드리지 않는다.
  - 이미지가 아닌 요청은 바꾸지 않는다. 새 회귀 테스트로 일반 요청의 도구 호출 문장이 그대로 저장되는지 확인한다.
- **함께 고친 점:** 3차까지는 최종 답을 바꾼 뒤 session 전체를 거꾸로 훑어 같은 문장의 assistant 메시지를 찾았다. 또 활성 요청이 없으면 마지막 종료 요청을 대상으로 삼았다. 이제 turn 시작 때의 활성 요청 ID로만 찾고, 이번 turn이 붙인 메시지만 고친다.
- **구조:** 이미지 turn 처리(`hold_answer_stream`, `withhold_run_text`, 답 교체)를 `agent_loop_method_image.rs`로 옮겼다(파일 크기 규칙).
- **새 테스트(3개, 기존 4개 보강):**
  - AgentLoop 3개: 도구 호출과 함께 온 문장이 승인 대기·완료 후 기록과 답 어디에도 없음, 이미지가 아닌 요청은 그 문장을 그대로 저장, provider 오류로 끝난 답도 보여 주지 않음.
  - 보강: 성공 시나리오는 provider가 "The blue cat is ready."라고 답하게 바꿔 답·기록·`/method status` 어디에도 없고 metadata의 `withheld_reply`에만 있음을 확인한다. 호출 없음 시나리오는 답과 기록에 원문이 없는지, 측정 불일치 시나리오는 불일치 문구로 시작하는지 확인한다. 실제 CLI 성공 시나리오도 runtime 답과 원문 부재를 확인한다.
  - provider 오류 테스트와 측정 불일치 보강은 수정 후에 더했으므로 수정 전 실패 기록은 없다.
- **정적 검사:** `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`, 기존 `imap-proto` future-incompatibility 경고만 있음)를 4차 반영 코드로 다시 실행했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`를 4차 반영 코드로 다시 만들었다(5차 반영 후 다시 만들었다. 아래 참조). 첫 요청의 이미지 호출에는 "Here is your red square!"라는 문장을 함께 보내게 했다(`native-qa-setup.py`).
  - 화면에는 모델 답변이 하나도 나오지 않는다. 성공은 "Image saved and checked on disk (see below).", 불일치는 "Image saved, but not every stated requirement is met (see below).", 실패·저장 실패·자격 증명 없음은 "No new image was saved for this request (image: …)"로 시작한다.
  - `qa-native-image-history-check.txt`(`history-check.py`): 저장된 대화 기록에서 호출과 함께 온 문장은 어디에도 없고, 최종 답변 5개는 metadata(`withheld_reply`)에만 있다. 도구 호출 assistant 메시지 6개 모두 문장이 비어 있고, 답 자리 5개는 runtime 답이다.
  - `qa-native-image-artifacts.txt`와 `fake-image-api-requests.log`도 다시 만들었다(256x192·128x128·64x64 PNG, digest가 화면 표시와 같음).
- **5차 전체 검증(Oracle 4차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle4.log`, `count-tests.py`, 1,321초).
  - suite 326개 기준 **3,238 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD020 종료 시점 3,212 대비 +26이다. PRD030의 새 테스트 26개(1차 15개, Oracle 1차 4개, 2차 2개, 3차 2개, 4차 3개)와 일치한다.

### Oracle 5차 검토 반영 (BLOCKED → 수정)

다섯 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle5-fix-before-agentloop.log`, `oracle5-fix-before-unit.log`: 각 1개 실패). 수정 후 결과는 `oracle5-fix-after-agentloop.log`(72/72), `oracle5-fix-after-unit.log`(45/45, workflow 단위 테스트 포함), `oracle5-fix-after-cli.log`(이미지 CLI 2/2·REPL 4/4·command 3/3)다.

- **이미지 요청이 workflow로 우회되면 모델 문장이 그대로 보임:** 80자 이상이고 "review" 같은 말이 들어 있는 이미지 요청은 자동 workflow로 들어갔다. 메시지 metadata로 workflow를 요청해도 그랬다. workflow 하위 작업에는 `image_generate`가 없고, workflow 답("Workflow completed/failed …")은 하위 작업의 요약 문장을 그대로 담는다. 그래서 하위 작업이 "The blue cat is ready."라고 쓰면 이미지가 없는데도 그 문장이 화면과 기록에 남았다(수정 전 로그에서 확인).
  - 이제 사용자 turn에서 활성 요청이 이미지 방식이면 자동·metadata workflow 진입을 건너뛰고 일반 실행으로 처리한다(`agent_loop_method_image.rs::skips_workflow`, `agent_loop.rs`의 `try_live_workflow_turn` 호출 전). 일반 실행이므로 위의 승인·확인·runtime 답이 그대로 적용된다.
  - 이미지가 아닌 요청의 workflow 진입은 바꾸지 않았다. 사용자 turn이 아닌 turn(시스템·자동화)도 그대로다.
- **새 테스트(2개):** AgentLoop 1개(긴 "review" 이미지 요청이 승인 → 저장 확인 → runtime 답으로 가고, 화면·기록에 모델 문장과 "Workflow completed"가 없음), 단위 1개(workflow metadata가 붙은 이미지 요청이 일반 실행으로 가서 "No image was generated …"로 답하고 모델 문장이 없음).
- **문서:** USAGE 이미지 절에 "이미지 방식 요청은 자동·metadata workflow로 보내지 않는다"를 더했다.
- **정적 검사:** `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)를 5차 반영 코드로 다시 실행했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`를 5차 반영 코드로 다시 만들었다(6차 반영 후 다시 만들었다. 아래 참조). `qa-native-image-long-review.txt`는 새 장면이다. 긴 "review" 이미지 요청이 workflow가 아니라 승인 → 저장 확인 → "Image saved and checked on disk (see below)."로 가고, 모델 답변("The blue cat is ready.")은 화면에 없다.
  - `qa-native-image-history-check.txt`: 기존 장면 결과는 4차와 같다. 새 장면도 모델 답변은 metadata(`withheld_reply`)에만 있고 답 자리는 runtime 답이다.
  - `qa-native-image-artifacts.txt`(새 장면의 64x48 PNG 포함)와 `fake-image-api-requests.log`도 다시 만들었다.
- **6차 전체 검증(Oracle 5차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle5.log`, `count-tests.py`, 1,422초).
  - suite 326개 기준 **3,240 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD020 종료 시점 3,212 대비 +28이다. PRD030의 새 테스트 28개(1차 15개, Oracle 1차 4개, 2차 2개, 3차 2개, 4차 3개, 5차 2개)와 일치한다.

### Oracle 6차 검토 반영 (BLOCKED → 수정)

여섯 번째 독립 Oracle gate가 BLOCKED로 판정한 두 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle6-fix-before-agentloop.log`, `oracle6-fix-before-unit.log`: 각 1개 실패). 수정 후 결과는 `oracle6-fix-after-agentloop.log`(74/74), `oracle6-fix-after-unit.log`(46/46), `oracle6-fix-after-cli.log`(이미지 CLI 2/2·REPL 4/4·command 3/3)다.

- **질문·메시지 도구가 runtime 답을 우회함:** 이미지 요청에서도 모델이 `ask_user`로 "The blue cat is ready. Want changes?"처럼 물으면 그 질문이 그대로 보였다(수정 전 로그에서 확인). 운영 설정에서는 `message`도 등록되므로, 모델 문장을 직접 보내고 runtime 답까지 생략시킬 수 있었다.
  - 이제 이미지 방식 turn에서 모델에게 주는 도구는 `image_generate` 하나다(`agent_loop_method_image.rs::image_tools`, `agent_loop.rs`의 `AgentRunSpec` 생성). 다른 도구(`ask_user`, `message`, `spawn`, `propose_entrusted_work`, 감싼 `tool_call` 포함)는 모델에게 보이지 않고, 불러도 "not found"로 실행되지 않는다. 승인 대기 중인 `image_generate` 실행과 승인 화면은 그대로다.
  - 그런 호출이 남긴 인자는 대화 기록에 붙이기 전에 지운다(`withhold_run_text`: 이름은 남기고 인자는 `{}`). 진행 출력에서도 `image_generate`가 아닌 도구 호출 이벤트는 내보내지 않는다(`hold_model_text`, 이전 이름 `hold_answer_stream`).
  - 이미지 요청에서는 맡김 작업 제안(`proposal_from_runner_messages`)을 만들지 않는다.
- **대기 중 workflow의 재개가 우회 방지를 거치지 않음:** 5차 수정은 새 workflow 진입만 막았다. 이미 저장된 pending workflow는 그보다 먼저 `try_resume_pending_workflow`가 이어 실행해, 하위 작업의 "- child-1: The blue cat is ready."가 다시 보였다(수정 전 로그에서 확인).
  - 이제 이미지 요청의 사용자 turn에서는 재개 전에 pending workflow를 취소한다(`cancel_image_workflow`). 방식 변경 때와 같은 절차로 durable `WORKFLOW_FAILED`(state `cancelled`, reason `image_request`)를 먼저 기록하고 대기 표시를 지운다(`agent_loop_method_cancel.rs::cancel_pending_workflow`로 공통화). 그 다음 일반 이미지 경로로 간다.
- **함께 고친 점(스스로 찾음):** provider 진행 이벤트 중 text delta만 막고 있었다. reasoning delta와 도구 호출 입력(`ToolCallDelta`·`ToolCallReady`)에도 모델 문장이 담겨, `ask_user` 질문이나 이미지 주장이 진행 이벤트로 나갈 수 있었다(`oracle6b-fix-before-agentloop.log`: 1개 실패). 이제 이미지 turn에서는 도구 호출 시작·종료와 미디어 이벤트만 내보낸다.
- **새 테스트(3개):** AgentLoop 2개(`ask_user`·`message`·`spawn`·`propose_entrusted_work` 호출이 실행되지 않고, 답·저장 기록·tool 이벤트 어디에도 모델 문장이 없으며, provider에 제시된 도구가 `image_generate`뿐; reasoning·도구 호출 입력 이벤트에도 모델 문장이 없음), 단위 1개(저장된 pending workflow가 있어도 이어 실행하지 않고 `image_request` 취소를 기록). 진행 이벤트를 흉내 내는 테스트 provider(`Streaming`)는 이제 reasoning delta와 도구 호출 입력도 보낸다.
- **구조:** 이미지 관련 단위 테스트를 `agent_loop_method_image_tests.rs`로 모았다(5차의 metadata workflow 테스트도 여기로 옮겼다. 파일 크기 규칙).
- **문서:** USAGE 이미지 절의 "질문은 그대로 보여 준다"를 "모델은 `image_generate`만 쓸 수 있다"로 바꾸고, 대기 중 workflow 취소를 더했다. 진행 출력 설명도 reasoning·도구 호출 입력까지 넓혔다.
- **정적 검사:** `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)를 6차 반영 코드로 다시 실행했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`를 6차 반영 코드로 다시 만들었다(7차 반영 후 다시 만들었다. 아래 참조). `qa-native-image-tools.txt`는 새 장면이다. 모델이 `ask_user`로 "The blue cat is ready. Want changes?"를 묻고 `message`로 같은 문장을 보내려 하지만, 화면에는 runtime 답("No image was generated for this request …")만 나온다.
  - `qa-native-image-history-check.txt`: 새 장면의 저장 기록에는 도구 호출 인자를 포함해 "blue cat"이 어디에도 없고, 최종 답변은 metadata(`withheld_reply`)에만 있다. 기존 장면 결과는 5차와 같다.
  - `qa-native-image-artifacts.txt`와 `fake-image-api-requests.log`도 다시 만들었다.
- **전체 검증(Oracle 6차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle6.log`, `count-tests.py`, 1,509초).
  - suite 326개 기준 **3,243 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트 `mismatch_uses_isolated_tree`다.
  - PRD020 종료 시점 3,212 대비 +31이다. PRD030의 새 테스트 31개(1차 15개, Oracle 1차 4개, 2차 2개, 3차 2개, 4차 3개, 5차 2개, 6차 3개)와 일치한다.
  - 진행 이벤트 수정 전 6차 코드의 실행 기록은 `workspace-test-oracle6a.log`(3,242 passed / 1 failed / 13 ignored)다.

### Oracle 7차 검토 반영 (BLOCKED → 수정)

일곱 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle7-fix-before-agentloop.log`: 1개 실패, `oracle7-fix-before-cli.log`: 1개 실패). 수정 후 결과는 `oracle7-fix-after-agentloop.log`(76/76), `oracle7-fix-after-unit.log`(46/46), `oracle7-fix-after-cli.log`(이미지 CLI 2/2·verbose 1/1·REPL 4/4·command 3/3)다.

- **`--verbose` 로그로 모델 답변이 나감:** runtime hook(`AgentHook`)은 모델 응답을 그대로 받았다. `run --verbose`(기본 Compose gateway도 이렇게 실행)의 `RuntimeVerboseLogHook`은 응답 앞 120자를 "LLM response preview:"로 stderr에 찍어, 화면과 기록에서 지운 "The blue cat is ready."가 terminal과 `docker compose logs`에 남았다(수정 전 로그에서 확인). observability hook과 plugin hook도 같은 값을 받았다.
  - 이제 이미지 turn에서는 runtime hook에 조용한 사본만 넘긴다(`agent_loop_method_quiet.rs::Quiet`, `hold_model_text`에서 감쌈). 응답의 본문·reasoning·thinking은 비우고, hook 문맥의 assistant 문장과 reasoning도 지우며, 쓸 수 없는 도구 호출의 인자는 `{}`로 바꾼다. text delta는 hook에도 보내지 않고, 최종 문장 변환(`finalize_content`)도 hook에 넘기지 않는다.
  - method 관측(`task_method_observe::attach`)은 그 바깥에서 원래 값을 그대로 본다(`agent_loop.rs`에서 `hold_model_text`를 `attach`보다 먼저 호출). 그래서 이미지 호출 기록·판정은 바뀌지 않는다.
  - 일반 요청의 hook은 그대로다.
- **새 테스트(3개):** AgentLoop 2개(이미지 turn의 hook이 응답 본문·reasoning·도구 인자·문맥 어디에서도 모델 문장을 받지 않음, 일반 turn의 hook은 그대로 받음), 실제 CLI 1개(`crates/shacs-cli/tests/it/spec037_image_verbose.rs`: 컴파일된 `shacs-bot run --verbose`에 WebSocket으로 이미지 요청을 보내, 답과 stderr 어디에도 모델 문장이 없고 "LLM response preview"는 찍히는지 확인).
- **문서:** USAGE 이미지 절에 runtime hook과 `--verbose` 동작을 더했다.
- **정적 검사:** `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)를 7차 반영 코드로 다시 실행했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`, `qa-native-image-history-check.txt`, `qa-native-image-artifacts.txt`, `fake-image-api-requests.log`를 7차 반영 코드로 다시 만들었다(8차 반영 후 다시 만들었다. 아래 참조). 결과는 6차와 같다(모델 문장은 화면과 저장 메시지 어디에도 없고 `withheld_reply`에만 있다). `run --verbose` 표면은 위 실제 CLI 테스트가 확인한다.
- **전체 검증(Oracle 7차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test-oracle7.log`, `count-tests.py`, 1,363초).
  - suite 327개(새 CLI suite `spec037_image_verbose` 포함) 기준 **3,244 passed / 3 failed / 13 ignored**다.
  - 실패 중 `mismatch_uses_isolated_tree`는 baseline부터 있던 Spec034 fixture 실패다. 나머지 2개(`runtime::context_resolvers::tests::context_resolver_oversized_url_is_skipped`, `context_resolver_url_artifact_marks_external_content`)는 이번 변경과 무관한 기존 테스트로, URL 안전 확인 중 `example.com` DNS 조회에 기대며 실행 중 일시적으로 `Denied`가 되었다. 같은 코드에서 두 테스트만 다시 실행하면 통과한다(`rerun-context-resolvers.log`: 2/2). 이전 전체 실행들에서도 모두 통과했다.
  - 이 두 테스트를 통과로 치면 3,246 passed이며, PRD020 종료 시점 3,212 대비 +34다. PRD030의 새 테스트 34개(1차 15개, Oracle 1차 4개, 2차 2개, 3차 2개, 4차 3개, 5차 2개, 6차 3개, 7차 3개)와 일치한다.

### Oracle 8차 검토 반영 (BLOCKED → 수정)

여덟 번째 독립 Oracle gate가 BLOCKED로 판정한 한 항목을 고쳤다. 수정 전에 실패를 재현하는 테스트를 먼저 추가했다(`oracle8-fix-before-agentloop.log`: 1개 실패). 수정 후 결과는 `oracle8-fix-after-agentloop.log`(77/77), `oracle8-fix-after-unit.log`(46/46), `oracle8-fix-after-cli.log`(이미지 CLI 2/2·verbose 1/1·REPL 4/4·command 3/3)다.

- **복구용 checkpoint에 모델 문장이 남음:** runner는 실행 도중 복구용 checkpoint(`runtime_checkpoint`)를 session 파일에 저장한다. 여기에 assistant 메시지의 본문(도구 호출과 함께 온 "Here is your blue cat!", 마지막 답 "The blue cat is ready.")과 reasoning·thinking이 그대로 들어갔다(호출 인자는 runner가 이미 가린다). 정상 종료 때는 지워지지만, 실행 중 프로세스가 멈추면(승인 없이 `image_generate`가 실행되는 동안의 중단·재시작 등) 남는다. 다음 turn의 복구(`materialize_checkpoint`)가 그 메시지를 대화 기록에 붙여 `/history`와 다음 모델 문맥에 보였다.
  - 이제 이미지 turn에서는 checkpoint를 저장하기 전에 assistant 본문을 비우고 reasoning·thinking을 뺀다(`agent_loop_method_image.rs::quiet_checkpoints`, `agent_loop.rs`에서 checkpoint callback을 감쌈). 일반 turn의 checkpoint는 그대로다.
  - 같은 테스트로 대화 기록의 reasoning도 찾았다. `withhold_run_text`가 본문만 비우고 `reasoning_content`를 남겼다. 이제 함께 지운다. 이 값이 필요한 thinking 모델에는 provider client가 빈 값을 채운다(`backfill_assistant_reasoning_content`).
  - Anthropic thinking block은 기록에 남긴다. 승인 뒤 도구 호출을 이어갈 때 provider가 직전 assistant 메시지의 thinking block을 요구하기 때문이다. 어떤 화면에도 보이지 않고, hook에는 넘기지 않으며(`Quiet`), checkpoint에서는 뺀다. USAGE에 이 예외를 적었다.
- **새 테스트(1개):** AgentLoop 1개(`image_checkpoint.rs`). 도구 실행 직전과 각 단계가 끝날 때 디스크의 session 파일을 읽는 hook으로, 그 순간 프로세스가 멈췄을 때 남을 내용에 모델 문장·reasoning이 없고 checkpoint 자체는 저장되는지 확인한다.
- **정적 검사:** `fmt.log`(`FMT_EXIT=0`), `clippy.log`(`CLIPPY_EXIT=0`)를 8차 반영 코드로 다시 실행했다.
- **실제 표면 재확인:** `qa-native-image-*.txt`, `qa-native-image-history-check.txt`, `qa-native-image-artifacts.txt`, `fake-image-api-requests.log`를 8차 반영 코드로 다시 만들었다. 결과는 7차와 같다.
- **최종 전체 검증(Oracle 8차 반영 후 코드):** 같은 명령을 다시 실행했다(`.omo/evidence/spec037/prd030/workspace-test.log`, `count-tests.py`, 1,351초).
  - suite 327개 기준 **3,241 passed / 7 failed / 13 ignored**다. `mismatch_uses_isolated_tree`는 baseline부터 있던 Spec034 fixture 실패다. 7차 때의 `context_resolvers` 2개는 이번에는 통과했다.
  - 나머지 6개는 `spec033_release_closure`의 closure 테스트로, 모두 `DigestMismatch`였다. 이 테스트는 시작과 끝에 작업 트리 전체의 source manifest를 비교한다. 실행 중 저장소 소유자가 이번 작업과 무관한 문서(Spec 042 초안, `docs/specs/README.md` 등)를 편집해 manifest가 달라졌다(파일 수정 시각 19:31–19:56, 실행 19:41–20:04). 같은 코드에서 이 suite만 다시 실행하면 7/7 통과한다(`rerun-spec033-release-closure.log`). 이전 전체 실행들에서도 모두 통과했다.
  - 이 6개를 통과로 치면 3,247 passed이며, PRD020 종료 시점 3,212 대비 +35다. PRD030의 새 테스트 35개(… 7차 3개, 8차 1개)와 일치한다.

## 종료 확인 (2026-10-22)

- 범위: 이미지 방식 요청에서 `image_generate` 호출마다 결과를 기록하고, 도구가 보고한 파일이 media root 안에 있고 digest가 맞을 때만 생성으로 인정한다. 저장된 파일의 형식·크기·장수를 요청 문장과 비교하고 내용·모양은 미확인으로 남긴다. 답은 runtime이 쓰고 모델 답변은 어떤 화면·기록·진행 출력·hook·복구 checkpoint에도 내보내지 않는다. `/method revise`로 원래 요청과 이전 이미지를 이어 새로 생성한 판을 만들고, 편집·마스크·변형은 지원하지 않는다고 표시한다. 미설정·자격 증명 없음·생성 실패·저장 실패·저장 불확실·거부는 성공 이미지를 만들지 않는다. 복합 요청은 포함하지 않는다.
- 최종 검증: 전체 workspace는 suite 327개 기준 **3,241 passed / 7 failed / 13 ignored**다. 실패는 baseline부터 있던 Spec034 fixture 테스트 1개와, 실행 중 저장소의 무관한 문서가 바뀌어 생긴 `spec033_release_closure`의 digest 불일치 6개다. 그 suite는 같은 코드에서 다시 실행해 7/7 통과했다(`rerun-spec033-release-closure.log`). fmt와 workspace clippy `-D warnings`도 통과했다.
- 실제 표면: macOS native compiled REPL과 운영 설정의 `openai` image provider가 로컬 결정론적 호환 API를 부르는 경로로 확인했다(`qa-native-image-*.txt`). 성공·측정 불일치·수정판·provider 오류·저장 실패·거부, 자격 증명 없음·미설정·미지원 provider, 긴 "review" 요청, 질문·메시지 도구를 쓰려는 모델을 담았다. 저장 파일과 기록은 따로 확인했다(`qa-native-image-artifacts.txt`, `qa-native-image-history-check.txt`). `run --verbose`는 실제 CLI 테스트로 확인했다.
- Oracle 독립 검증: **PASS** (9차).
  - 1–8차에서 다음 결함을 차례로 찾았고, 모두 고친 뒤 수정 전 실패 로그와 회귀 테스트로 고정했다.
    - `/method revise`가 실행 전부터 연결을 단정함, 성공 뒤 실패해도 앞 성공을 유지함, 8장을 넘는 결과의 장수 판정.
    - 실패한 요청이 모델의 성공 문구로 시작함, 보존 한도를 넘으면 마지막 저장 이미지 계보가 사라짐, 저장 범위 문서.
    - 실패 문구가 진행 출력과 기록에 남음, 성공 때도 모델의 내용 주장을 보임, 도구 호출과 함께 온 문장이 기록에 남음.
    - 긴 요청이 workflow로 우회됨, 대기 중 workflow의 재개, 질문·메시지 도구, reasoning·도구 입력 진행 이벤트, runtime hook과 `--verbose` 로그, 복구용 checkpoint.
  - 이 PASS는 PRD 040의 복합 결과를 포함하지 않는다.
- 남은 제한:
  - 이미지의 내용과 모양은 확인하지 않는다. 모델 답변을 보이지 않는 대신, 모델이 `ask_user`로 물어야 할 확인 질문도 이미지 방식에서는 쓸 수 없다. 일반 답이 필요하면 `/method use general`을 쓴다.
  - `image_generate` 인자(프롬프트)는 승인 화면·tool 이벤트·`/method status`에 "프롬프트"로 보인다. 결과에 대한 주장이 아니라 요청 내용이기 때문이다.
    - 이후 변경(2026-10-23, PRD040 Oracle 3차 반영): tool 이벤트와 진단용 hook(`--verbose` 로그, observability, plugin `llm:after`)에는 프롬프트 대신 길이·digest만 넘긴다. 승인 화면, plugin `tool:before` 정책 hook, `/method status`에는 그대로 보인다.
    - 이후 변경(2026-10-23, PRD040 Oracle 4차 반영): 도구 결과도 진행 이벤트, hook 문맥, 저장되는 대화 기록, 복구용 checkpoint에는 길이·digest만 남긴다. 모델은 같은 요청 안에서만 원래 결과를 본다(요청이 열린 동안 session metadata `task_method_held_results_v1`에 두었다가 끝나면 지운다).
    - 이후 변경(2026-10-23, PRD040 Oracle 6차 반영): 이미지 부분이 있는 요청에서는 방식 변경 응답과 `/method status`·`/method history`에 모델이 했던 질문을 "(the model's question is withheld)"로 적는다. 방식 변경으로 이미지 방식이 되면 그 요청에서 앞서 저장된 기록의 모델 문장과 도구 결과도 정리한다.
    - 이후 변경(2026-10-23, PRD040 Oracle 7차 반영): 진행 이벤트와 진단용 hook에는 인자의 경로·주소도 넘기지 않고(길이·digest만), 호출 ID도 digest로 바꾼다. 방식 표시의 "Observed written file"은 경로 대신 digest로 적는다.
    - 이후 변경(2026-10-23, PRD040 Oracle 8차 반영): provider의 스트리밍 호출 시작 이벤트는 제공된 도구일 때만 digest ID로 보낸다. 제공되지 않은 도구를 부른 호출은 runtime hook·대화 기록·복구용 checkpoint에 `unoffered_tool`이라는 이름으로만 남긴다.
  - Anthropic thinking block은 승인 뒤 도구 호출을 이어가는 데 provider가 요구하므로 저장된 기록에 남는다. 어떤 화면과 hook에도 보이지 않는다. `session export --yes`는 경고 후 원본 metadata(진단용 `withheld_reply` 포함)를 내보내는 명시적 진단 기능이다.
    - 이후 변경(2026-10-23, PRD040 Oracle 9차 반영): thinking block도 저장되는 대화 기록(SDK 실행 결과의 `messages` 포함)에서 뺀다. 요청이 열려 있는 동안만 도구 결과와 함께 `task_method_held_results_v1`에 두었다가 그 요청의 다음 실행에서 provider에게만 바꾸지 않은 채 되돌리고, 요청이 끝나면 지운다. provider API 오류로 끝난 이미지 요청의 답·대화 기록·실행 기록에는 오류 글 대신 상태 코드와 그 글의 길이·digest만 남긴다.
    - 이후 변경(2026-10-23, PRD040 Oracle 10차 반영): 이미지·복합 요청에서는 provider에 thinking을 요청하지 않는다(설정한 reasoning effort를 보내지 않고, 앞선 대화의 thinking block도 보내지 않음). 그래서 9차의 thinking 보관과 되돌림은 지웠고, provider가 보낸 thinking block은 기록하지 않고 버린다. provider가 오류를 응답(`finish_reason` "error")으로 알린 경우에도 답·기록에는 상태 코드와 digest만 남긴다.
    - 이후 변경(2026-10-23, PRD040 Oracle 11차 반영): runtime hook에 넘기는 응답과 호출 사본은 허용한 필드만으로 새로 만든다. provider 고유 필드(호출의 확장 필드, 응답의 오류 문자열)는 hook에 가지 않는다.
    - 이후 변경(2026-10-23, PRD040 Oracle 12차 반영): hook에 넘기는 응답과 provider 종료 이벤트의 종료 이유는 runtime이 아는 값이 아니면 `other`로, 사용량은 알려진 token 수 항목만 넘긴다. 이미지 요청이 방식 변경으로 다른 방식이 된 뒤의 실행 hook에도 이미지 단계에서 저장된 호출은 digest 사본으로만 넘긴다.
    - 이후 변경(2026-10-23, PRD040 Oracle 13차 반영): 실행 기록(`runtime_execution`)은 이미지 요청의 도구 호출을 원래 ID 대신 그 SHA-256 digest로 부른다. provider 이미지 진행 이벤트의 후보 ID도 digest로 보낸다.
    - 이후 변경(2026-10-23, PRD040 Oracle 14차 반영): 실행 기록은 모델의 질문도 digest로만 둔다. 이미지 요청이 방식 변경으로 다른 방식을 거치는 동안에도 끝날 때까지 같고, 다시 이미지 방식이 되면 남아 있던 기록도 바꾼다.
