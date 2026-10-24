# PRD 000. 작업 방법 후보와 근거의 구분

Status: Complete (Scoped)

## Goal

실제 작업 결과나 사용자 피드백에서 재사용 가능한 방법 **후보**를 만들고 근거·적용 조건·확신 수준을 사용자가 조회한다. **후보 조회만으로 닫히며 이후 승인·기본 적용은 요구하지 않는다.**

## Scope

1. 기존 단일 session의 실제/기록된 실행, 024 recipe 결과 또는 사용자가 설명한 절차에서 방법 후보를 만든다. 사용자 선호/개인 사실·작업별 임시 조건과 재사용 절차를 별개로 분류한다. 재사용할 절차를 뒷받침하는 근거가 없으면 후보를 만들지 않으며, 이를 오류가 아닌 정상 결과로 둔다.
2. 후보 ID, 제안한 단계, 적용할 작업/전제, 실제 관찰·피드백·미확인 사항의 구분, 근거별 출처 ref와 신뢰 구분(024 quarantine 기준), 요청·자료 digest, 버전과 작성 시각을 사용자 소유 로컬 자료에 저장한다. 요청·자료는 원문을 복제하지 않고 digest로 식별한다. 결과가 좋았거나 채택됐다는 사실을 특정 방법이나 단계의 효과로 기록하지 않고, 원인을 가를 수 없는 효과는 미확인으로 남긴다.
3. CLI 또는 현재 대화 표면에서 후보와 근거를 조회한다. 한 번 성공했거나 모델이 좋다고 평가한 사실만으로 활성 기본 방법이나 skill 권한을 생성하지 않는다.
4. 후보 생성 단계는 실행 기록·산출물·외부 자료 안의 지시를 근거 데이터로만 읽고, 실행·외부 발송·secret 접근 도구 없이 후보만 기록한다. 모델에는 018/033의 redacted trajectory 경계를 통과한 bounded 근거만 넘기며, raw secret이나 불필요한 full transcript를 생성 입력이나 후보에 복제하지 않는다.

## Non Scope

- 확인·승격·실제 다음 요청에 자동 적용·수정/롤백, 새 skill registry나 workflow engine.
- 036 지속 위임, 전문 분야 하네스 구현, remote marketplace, 권한 부여.

## Parent Requirement Mapping

- [038 SPEC](../SPEC.md)의 소유 범위 1과 수용 조건 1 전체를 소유한다.
- 사용자 선호/방법 분리, 단일 성공으로 무음 활성화 금지, 확인되지 않은 source를 성공 근거로 포장하지 않는 불변식을 소유한다.
- 근거 속 지시를 데이터로만 읽는 도구 없는 후보 생성, 효과 미확인 표시와 후보 없음 결과를 소유한다.

## Acceptance Criteria

1. 격리된 workspace의 작업 결과와 사용자 피드백에서 후보를 하나 만들고 절차·전제·출처와 신뢰 구분·미확인 부분을 CLI 또는 대화에서 조회한다. 같은 대화의 선호 사실은 별도 종류이며 이 후보에 섞이지 않는다.
2. 후보 작성 뒤 일반 유사 요청을 실행해도 해당 방법이 기본값으로 조용히 활성화되지 않는다. 출처 없는 텍스트나 실패 결과를 검증된 성공으로 표시하지 않고, 좋은 결과만 있고 원인을 가를 수 없는 효과는 미확인으로 표시한다.
3. 재시작 뒤 후보 ID/버전/원래 출처를 다시 읽을 수 있고, raw secret/불필요한 full transcript가 후보 표시나 후보 생성 입력으로 퍼지지 않는다.
4. 근거 안에 저장·발송·명령 실행을 요구하는 문장을 넣어도 후보 생성 단계에서 도구 실행이나 발송이 일어나지 않고, 그 문장이 검증된 절차로 표시되지 않는다. 재사용할 근거가 없는 작업은 후보 없이 끝나며 오류로 표시되지 않는다.

## Closure Evidence

- 후보 생성·조회·재시작 사용자 표면과 근거/선호 분류 결과.
- 단일 성공·모델 자기평가·잘못된 source에 의한 자동 활성화 거부 테스트.
- 근거 속 지시 주입·효과 귀속·후보 없음 사례 테스트와 후보 생성 입력의 redaction 검증.

PRD 000은 **검토 가능한 비활성 방법 후보**라는 자체 결과로 닫힌다. 뒤 번호의 승인·실행·롤백이 없어도 완료할 수 있다.

## 구현 메모 및 검증 근거 (2026-10-23)

아래는 구현·실행 기록이다. 완료 판정은 종료 확인 절에 따로 기록한다.

- **표면:** `shacs-command`의 non-priority builtin `/learn`(후보 생성), `/learn list`, `/learn show <method-id>`. 같은 `AgentLoop`를 쓰는 `agent` REPL과 channel 대화에서 동작한다. 사용자 turn이 아니면(system·cron·automation, `automation_route`, 다른 대상을 위해 주입된 `injected_event` 등) 아무것도 바꾸지 않고 거절한다. 명령 응답은 대화 기록에 넣지 않는다. 사용법은 [USAGE](../../../USAGE.md#작업-방법-후보와-learn)에 적었다.
- **근거 수집(`learned_method_evidence`):** session 전체를 결정적으로 읽고, 최근 사용자 turn 6개와 그 뒤 메시지만 근거로 만든다. 근거는 사용자 메시지, 도구 호출(이름, 인자 digest, 결과 판정 `ok`/`error`/`incomplete`, 결과 digest), assistant 답변이며 최대 40개다. 도구 이름은 `/learn` 시점 tool registry에 있는 이름일 때만 남기고, 그 밖의 이름(모델이 지어낸 이름 등)은 글이 실릴 수 있으므로 `unregistered tool`로 바꾼다. 각 근거에 원래 메시지의 시각을 남긴다. 창 밖으로 빠진 수(앞선 turn과 40개 초과분)를 함께 기록한다. 출처 ref는 018 session evidence와 같은 `session://` 형식이되 session key 대신 그 digest 12자리를 쓰고, 도구 호출에는 call ID digest를 덧붙인다. 저장하는 session 식별자도 digest다.
  - 신뢰 구분은 024 quarantine을 따른다. 사용자가 직접 쓴 turn만 `owner`다. runtime은 사용자 turn을 저장할 때 `/learn`의 사용자 turn 판정과 같은 기준으로 `owner_turn: true`를 붙이고, 근거 수집은 이 표시가 있는 메시지만 `owner`로 본다. system·heartbeat·cron·automation channel turn은 metadata 없이 저장되므로 표시가 없다. 도구 결과·첨부, 표시가 없거나 사용자가 아닌 주입 메시지(`injected_event`·`goal_continuation`·`automation_route` metadata, runtime context 블록을 단 turn 중간 주입, runner가 저장한 출력 한도 복구 지시)는 `untrusted`다. 같은 session에서 그런 자료를 한 번이라도 읽은 뒤의 모델 인자·답변은 창 밖의 노출까지 따져 `untrusted`, 그 전의 모델 출력은 `model`이다.
  - 실행 판정은 runtime이 tool event의 성공·실패를 정하는 `runner::is_tool_unsuccessful_message`(`Error` 접두사, MCP 실패·시간 초과·취소, 작업 폴더 안전 차단; MCP 도구가 `isError: true`로 알린 실패는 stdio MCP 호출이 `(MCP tool call failed: …)`로 바꾼다), 기존 `task_method_report_source::failure`(실패 접두사와 최상위 `error` JSON), 마지막 줄의 `Exit code: N`(0이 아니면 `error`)을 쓴다. 도구가 이 밖의 자유로운 문장으로 알린 실패는 runtime의 tool event와 마찬가지로 알아보지 못한다. 결과가 없거나 비었거나 승인 대기·건너뜀·Spec 037에서 가려진 결과(`<withheld: …>`)이거나 종료 코드 줄이 없는 `exec` 결과이면 `incomplete`다. 그 밖만 `ok`다.
  - 결과는 같은 호출 ID의 tool 메시지 중, 뒤에서 같은 ID로 다시 호출하기 전의 마지막 것과 짝짓는다(승인 뒤 덧붙는 결과 포함). 같은 호출 묶음 안에서 ID가 겹치거나 비면 짝을 짓지 않아 `incomplete`다.
- **도구 없는 두 번의 생성(`learned_method_generate`):** 두 호출 모두 `tools: []`, `tool_choice: None`이며 AgentRunner·tool registry를 거치지 않는다. 응답의 도구 호출은 실행될 경로가 없고, 도구 호출이 담긴 응답은 실패로 보고하며 저장하지 않는다. provider 오류 글은 redaction 후 200자로 자른다.
  1. 분류 호출에는 `owner` 메시지의 글만 보낸다. 각 메시지를 request·feedback·procedure·preference·task_condition·mixed·other로 나누며, 분류하지 않은 메시지는 other다.
  2. 초안 호출에는 request·feedback·procedure 메시지의 글만 보낸다. 나머지 사용자 메시지는 아예 빼고, 도구 호출은 이름·실행 판정만, 답변은 있었다는 사실만 보낸다. 모델이 쓴 인자와 답변은 제외한 선호를 되풀이할 수 있으므로("Okay Bob") 신뢰 구분과 관계없이 글을 보내지 않는다. 글은 018/033 evidence 경계가 쓰는 `shacs-redaction` 변환 뒤 600자로 자른다. 도구 결과와 첨부 내용은 어느 호출에도 보내지 않는다. 초안 지시는 요청의 표현·이름·경로를 옮기지 말고 일반적으로 쓰라고 요구한다.
- **검증(`learned_method_build`, `learned_method_guard`):** 초안의 모든 줄은 근거와 대조한다.
  - 초안 호출에 보이지 않은 글을 되풀이하거나 제외한 메시지를 인용한 줄은 통째로 버리고 수를 미확인 사항에 적는다. 보인 글에도 있는 표현은 되풀이로 보지 않는다. 되풀이는 세 가지로 찾는다. 가려 둔 모든 글(제외한 사용자 메시지, `untrusted` 근거의 글, 남은 근거에 딸린 도구 결과 앞 4,000자)과 16자 이상 글자 그대로 겹치는 것(FNV 해시로 비교). 선호·작업 조건·섞임 메시지의 구절(문장부호·줄바꿈으로 나눔)이 낱말 단위로 그대로 들어 있는 것(한 낱말이라도). 그 밖의 가려 둔 글(기타·비사용자 메시지, 모델의 답변·인자, 도구 결과)의 두 낱말 이상 구절이 그대로 들어 있는 것. 그리고 요청(초안 호출이 보는 글)을 옮긴 것: 요청의 32자 이상, 요청 구절 하나(한 낱말 요청, 띄어 쓰지 않은 요청, 기호뿐인 요청 포함), 또는 요청에 든 구체적인 표시(경로·주소·가운데 점이 있는 파일 이름·숫자·대문자 코드)를 담은 것. 같은 표현이 피드백·절차에도 (구체적인 표시는 같은 낱말로) 있으면 요청을 옮긴 것으로 보지 않는다.
  - 목적·적용 작업은 보인 사용자 메시지를 인용해야 하며 모델의 문장으로 표시한다. 아니면 후보를 저장하지 않고 생성 실패로 보고한다. 전제는 인용한 feedback·procedure 메시지에 그대로 있을 때만 남고, 아니면 "Premise not in your own words"로 미확인 사항에 옮긴다.
  - 단계의 근거는 runtime이 계산하며 모델이 붙인 `support`/`verified` 같은 필드는 무시한다. 인용한 feedback·procedure 메시지에 단계 문장이 그대로(대소문자·공백·끝 문장부호 무시, 6자 이상) 있으면 `user_stated`이고(요청은 그 작업에만 해당하므로 인용으로 치지 않는다) 근거로는 문장이 들어 있는 메시지만 남긴다. 그렇지 않고 `ok` 도구 호출을 인용하면 `observed_run`이며, 근거로는 그 호출만 남기고, 단계 문장은 runtime이 쓴 "Run <도구>"(같은 도구는 한 번, 400자까지), 모델 문장은 확인하지 않은 `model_note`로 둔다. 그 밖은 `unverified`이며, 실패·미완료 실행만 인용했으면 이유를 적는다. 없는 근거 ID는 버리고 수를 적는다.
  - `user_stated`·`observed_run` 단계가 하나도 없거나, 분류 결과 보일 사용자 메시지가 없거나, 모델이 `candidate: null`을 주면 후보 없이 끝난다. 사용자 메시지가 전혀 없으면 provider를 부르지 않는다. 이 결과는 오류가 아니라 정상 결과로 표시한다.
  - 저장하는 모든 문자열은 redaction 후 자른다(단계 400자, 나머지 300자, 단계 12개, 목록 8개).
- **저장(`learned_method_store`):** workspace의 `.shacs-methods/methods.json`(schema `shacs.learned-methods.v1`, `0600`)에 lock·임시 파일 rename·fsync로 기록하며, 쓰기 전에도 읽을 때와 같은 검증을 해 다시 읽지 못할 파일은 만들지 않는다. 메모리(`memory/`, `USER.md`)와 `skills/`와 분리된다. 상태는 항상 `candidate`이고 `revision`·`current_version`·`versions`는 뒤 PRD의 CAS·버전 관리를 위한 자리다. 메시지 자체(요청·자료·대화)는 저장하지 않고 digest와 출처 ref만 둔다. 저장되는 글은 모델의 목적·적용 작업·전제·단계 문장과 runtime 표시이며, 위 검증이 놓치는 짧은 표현은 남을 수 있다. digest로 원래 메시지를 찾아 주는 기능은 없다. 모르는 schema, 깨진 파일, symlink 경로, 읽을 때 다시 확인하는 한도(200개, 16 MiB, 버전 64개, 단계 12개, 전제 8개, 목록·인용 40개, 글 1,000자, ref·시각·digest 200자)를 넘거나 버전 번호·근거 ID가 겹치는 파일은 바꾸지 않고 오류로 보고한다. 표시와 근거가 맞지 않는 파일도 같다(`learned_method_validate`): 인용한 근거가 모두 있어야 하고, 목적·적용 작업은 `owner` request·feedback·procedure 메시지를, 전제와 `user_stated` 단계는 `owner` feedback·procedure 메시지를, `observed_run` 단계는 `ok` 도구 호출을 인용하고 그 호출들로 runtime이 쓰는 "Run …" 문장과 같아야 하며, `user_stated`·`unverified` 단계에는 `model_note`가 없고, 제외 항목은 그 종류로 분류된 사용자 메시지를 가리켜야 하고 그런 메시지마다 제외 항목이 있어야 한다. 출처 ref는 `session://<origin digest 12자리>/<번호>`(도구 호출은 `#call-<12자리>`까지)여야 하고, origin·근거·결과 digest는 `sha256:<소문자 16진수 64자리>`, 근거 ID는 `e<번호>`여야 하며, 도구 호출·답변은 `owner`일 수 없고 사용자 메시지는 `model`일 수 없으며, 도구 이름·실행 판정은 도구 호출에만, 분류는 사용자 메시지에만 있어야 한다. 저장된 모든 글은 redaction을 다시 해도 바뀌지 않아야 한다(생성 때 redaction했으므로, 손으로 넣은 비밀값만 걸린다).
- **표시(`learned_method_render`):** 후보 상태(`candidate (inactive; not used by default)`), 인용이 붙은 목적·적용 작업·전제, 단계별 근거(`your own words`, `ran without a reported error`와 모델 설명, `unverified`), 모든 후보의 "Effect not attributed" 줄, 미확인 사항, 제외 항목, 근거별 종류·실행 판정·신뢰 구분·출처 ref·digest를 보여 준다. `/learn`·`list`·`show`는 저장된 모든 후보를 원래 session의 지금 기록과 다시 대조한다(`learned_method_recheck`). 원래 session은 한 번만 읽고 시각별로 색인하며, 신뢰 구분과 다시 만든 기록도 그 session을 인용한 후보들이 함께 쓴다. 같은 시각·digest의 메시지가 남아 있으면 다음을 확인하고, 어긋나면 store 전체를 "a method does not match the messages it cites"로 거절하고 바꾸지 않는다.
  - 근거 수집과 같은 함수로 그 메시지의 기록을 다시 만들어, 저장된 실행 판정·결과 digest·도구 이름이 같다. 사용자 메시지의 신뢰 구분은 메시지 자체로 정해지므로 정확히 같아야 하고, 모델·도구 기록은 다시 계산한 값보다 더 신뢰되지 않아야 한다(그 사이 앞선 untrusted 자료가 잘려 나가면 더 신뢰될 수는 있다). 다만 `ok`가 아니던 도구 호출의 결과가 그 뒤 붙거나 바뀌었으면(늦은 승인·답) 거절하지 않고 "changed after saving, not rechecked"로 낮춘다. 그런 호출은 어떤 단계도 뒷받침하지 않는다.
  - 전제와 `user_stated` 단계 문장이 인용한 각 메시지의 생성 때 보인 redaction·길이 제한 형태에 들어 있다.
  - 모델이 쓴 모든 줄(목적·적용 작업·전제·단계·`observed_run` 설명·모델의 미확인 사항, runtime의 "Run …"과 runtime 메모는 제외)이 남아 있는 사용자 메시지에 대해 생성 때와 같은 검사(요청 옮김, 제외 메시지 되풀이, 제외 메시지 인용)를 통과하고, 검사가 돌려준 글이 저장된 글과 같다. 사용자 메시지는 다시 계산한 신뢰 구분으로 넣는다. 도구 결과·답변의 글은 이 검사에 다시 넣지 않는다.
  - 저장된 위치(출처 ref의 번호)와 지금 위치가 다르면 내용은 확인하되 그 근거를 "saved position not rechecked"로 표시한다.
  - 메시지가 없으면 그 근거에 "no longer in that session"을, 끝 줄에 "some source messages changed or were cleared since"를 붙이고, 그 근거에 기댄 단계는 "… when saved"와 "not rechecked"로, 목적·적용 작업·전제는 인용 뒤에 같은 표시로, 목록에는 "some sources changed or cleared since, not rechecked"로 보여 준다. "(this conversation)"은 후보를 만든 session에서 근거가 모두 저장한 위치에 남아 있을 때만 붙는다.
  - session 목록을 읽지 못하거나, 목록이 건너뛴 session 항목(읽을 수 없는 파일, 링크·디렉터리)이 있는데 원래 session을 찾지 못하면 삭제로 보지 않고 "could not be checked against their sessions" 오류로 닫는다.
  - 200개 후보가 모두 1,998개 메시지 session의 근거 39개(4KB 도구 결과 38개)를 인용할 때 debug build에서 `list`·`show`는 process 시작을 포함해 0.44–0.71초, 같은 크기의 서로 다른 session 20개에서 온 200개 후보는 2.26초다(`qa8/PERFORMANCE.md`). 원래 session마다 한 번 읽으므로 시간은 후보 수가 아니라 서로 다른 원래 session 수에 따라 늘어난다(이 크기에서 session 하나당 약 0.07초). 일반 turn은 이 store를 읽지 않으므로 후보가 다음 요청의 문맥이나 기본값에 들어가지 않는다.
- **독립 Oracle gate 1차 판정(BLOCKED) 후 보정했다.** Oracle은 다섯 결함을 후보 커밋에서 failing-before 테스트로 재현했다.
  1. 제외한 선호가 목적·적용 작업·전제와, 다른 근거를 섞어 인용한 단계에 들어갈 수 있었다. 분류와 초안을 두 호출로 나누어 제외한 메시지를 초안 호출에 보이지 않게 했고, 위의 겹침·인용 검사를 더했다.
  2. `untrusted` 답변의 지시를 요청 하나만 인용해 `you said it`으로 바꿀 수 있었다. `untrusted` 글을 초안 호출에서 뺐고, `user_stated`는 사용자 문장을 그대로 옮긴 단계로, `observed_run` 단계 문장은 runtime 표시로 한정했다.
  3. 최상위 `error` JSON과 Spec 037에서 가려진 `Permission denied` 결과가 `ok`로 기록됐다. 위의 실행 판정으로 바꿨다.
  4. 근거 창보다 앞선 주입 메시지의 노출이 사라졌다. 노출을 session 전체에서 따지게 했다. 같은 계열로 runner가 저장한 출력 한도 복구 지시를 사용자 발언으로 보던 점은 자체 검토에서 찾아 실패를 재현한 뒤 고쳤다. 같은 방식으로 저장되는 turn 중간 주입(runtime context 블록을 단 메시지)도 함께 제외했으나 따로 재현한 테스트는 없다.
  5. session key가 출처 ref와 origin에 그대로 저장됐다. digest로 바꿨다.
  - 함께 지적된 창 밖 누락 수 과소 계산과 문서의 과장(분류 보장, digest 조회)도 고쳤다.
  - 각 결함은 `spec038_learned_methods`의 `leaks`·`records`·`boundaries` 회귀 테스트로 고정했다.
- **독립 Oracle gate 2차 판정(BLOCKED) 후 보정했다.** Oracle은 1차 결함이 모두 고쳐졌음을 failing-before 실험으로 확인한 뒤, 같은 불변식을 우회하는 네 결함을 재현했다.
  1. 16자보다 짧은 제외 선호(예: "Call me Bob")는 겹침 검사에 걸리지 않아 목적에 저장됐다. 제외 메시지의 구절을 길이와 관계없이 낱말 단위로 대조하게 했다.
  2. 뒤의 호출이 같은 ID를 다시 쓰면 그 결과가 결과 없는 앞 호출의 `ok`로 붙었다. 위의 짝짓기 규칙으로 바꿨다.
  3. `/new`로 원래 메시지가 지워져도 후보는 "(this conversation)"과 유효해 보이는 위치를 보여 줬다. 위의 출처 확인을 더했다.
  4. `injected_event`가 붙은 비사용자 turn이 public AgentLoop 경로에서 `/learn`을 실행해 후보를 저장했다. 근거 수집과 같은 주입 판정(`learned_method_source::injected`)으로 거절하게 했다.
  - 함께 지적된 큰 session의 겹침 검사 비용(1,998개 메시지, 4KB 도구 결과 998개에서 5.6초)은 남은 근거의 도구 결과만 검사하고 할당 없는 해시를 쓰도록 바꿨다. 같은 크기에서 debug build 측정은 `/learn` 0.77초, `/learn show` 0.35초다. 읽기 때 한도 미검사와 문서·테스트 누락도 고쳤다.
  - 새 회귀 테스트 다섯 개는 2차 후보 코드에서 모두 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate2-fixes/`).
- **독립 Oracle gate 3차 판정(BLOCKED) 후 보정했다.** Oracle은 2차 결함이 모두 고쳐졌음을 확인한 뒤 두 결함을 실제 compiled REPL에서 재현했다.
  1. 종료 코드가 0이 아닌 `exec` 결과(`Exit code: 1`)가 `ok`로 기록됐다. 위의 종료 코드 규칙을 더했다.
  2. 16자보다 짧은 `untrusted` 답변(예: "Email Bob")이 목적에 저장됐다. 구절 대조를 가려 둔 모든 글로 넓혔다.
  - 함께 지적된 인용 목록의 읽기 한도 누락, 버전·근거 ID 중복, `your own words`가 사용자 근거가 아닌 인용까지 보여 주던 점도 고쳤다.
  - 회귀 테스트(기존 테스트에 더한 assert 포함) 네 개는 3차 후보 코드에서 모두 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate3-fixes/`).
- **독립 Oracle gate 4차 판정(BLOCKED) 후 보정했다.** Oracle은 3차 결함이 모두 고쳐졌음을 확인한 뒤 네 결함을 실제 compiled `/learn`에서 재현했다.
  1. 제외한 선호를 assistant가 짧게 되풀이한 답변("Okay Bob")이 초안 호출에 보여 목적에 들어갔다. 초안 호출에서 모델이 쓴 답변·인자의 글을 모두 뺐다.
  2. MCP 실패(`(MCP tool call failed: …)`)와 cron 거절(`Cannot remove job …`, `Job … not found`)이 `ok`로 기록됐다. runtime의 tool 실패 판정과 cron 형식을 더했다. tool 결과에는 형식화된 성공·실패 값이 없어 runtime의 tool event도 같은 글 판정을 쓴다. 내장 도구의 결과를 조사해 접두사 없이 실패를 알리는 형식은 이 둘뿐임을 확인했다.
  3. 여러 실행을 인용한 단계의 "Run …" 문장이 읽기 한도를 넘어 저장 뒤 store를 읽지 못하게 됐다. 문장을 400자로 자르고 같은 도구를 한 번만 적으며, 저장 전에 읽기와 같은 검증을 하게 했다.
  4. 요청 원문(파일 경로 포함)이 목적과 `user_stated` 단계로 저장됐다. 요청은 인용으로 치지 않고, 요청을 옮긴 줄(32자 이상 또는 구체적인 표시)은 버리게 했다.
  - 함께 지적된 "아무 실행이나 인용한 전제"도 전제를 피드백·절차의 그대로 인용으로 한정해 고쳤다.
  - 회귀 테스트 다섯 개는 4차 후보 코드에서 모두 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate4-fixes/`).
- **독립 Oracle gate 5차 판정(BLOCKED) 후 보정했다.** Oracle은 4차 결함이 모두 고쳐졌음을 확인한 뒤 같은 불변식의 다른 입구를 지적했다.
  1. metadata 없이 저장되는 system channel turn이 사용자 말(`owner`)로 분류·인용될 수 있었다. 위의 `owner_turn` 표시로 바꿨다.
  2. 모델이 지어낸 도구 이름이 초안 호출과 저장 파일에 그대로 들어갔다. registry에 있는 이름만 남기게 했다.
  3. MCP 도구가 `isError: true`로 알린 실패가 성공 결과로 전달돼 `ok`로 기록됐다. stdio MCP 호출이 이를 재시도하지 않는 실패(`(MCP tool call failed: <도구의 글>)`)로 바꾸게 했다. 일반 turn에서 모델이 받는 도구 결과도 같은 형식이 된다.
  4. 32자 미만이고 구체적인 표시가 없는 소문자 요청을 목적에 그대로 옮길 수 있었다. 두 낱말 이상인 요청 구절을 담은 줄도 버리게 했다.
  5. 표시만 고친 저장 파일(예: 사용자 말 단계를 `observed_run`으로)이 그대로 읽혔다. 위의 표시·근거 일치 검사를 읽을 때와 쓰기 전에 하게 했다.
  - 회귀 테스트(기존 테스트에 더한 assert 포함) 네 개는 5차 후보 코드에서 모두 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate5-fixes/`). MCP 변환은 새 함수라 이전 코드에서 돌릴 수 없어 `tools::mcp` 단위 테스트의 assert로 고정했다.
- **독립 Oracle gate 6차 판정(BLOCKED) 후 보정했다.** Oracle은 5차 보정이 표적 사례에 적용됐음을 확인한 뒤 두 우회를 지적했다.
  1. 한 낱말 요청(예: `zephyrcase`)이나 띄어 쓰지 않은 한국어 요청은 두 낱말 이상 구절 검사에 걸리지 않아 목적으로 그대로 저장됐다. 요청의 구절을 낱말 수와 관계없이 검사하게 했다. 함께 지적된 USAGE의 "원문 사본은 없다"는 과장도 실제 보장(메시지 자체는 저장하지 않고, 검사가 놓치는 짧은 표현은 남을 수 있음)으로 고쳤다.
  2. 저장 파일에서 `user_stated` 단계 문장만 바꾸면 인용한 메시지와 달라도 `your own words`로 보였다. 위의 원래 session 대조를 더했다. 같은 방식으로 실패한 실행을 `observed_run`·`ok`로 바꾼 파일도 거절되며, "Run …" 문장은 저장 검증에서도 runtime 문장과 비교한다. 근거가 사라진 단계는 확정적으로 표시하지 않는다.
  - 회귀 테스트(기존 테스트에 더한 assert 포함) 네 개는 6차 후보 코드에서 모두 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate6-fixes/`).
- **독립 Oracle gate 7차 판정(BLOCKED) 후 보정했다.** Oracle은 6차 두 결함이 고쳐졌음을 확인한 뒤 다섯 결함을 지적했다.
  1. 기호뿐인 요청(예: 이모지 하나)은 영숫자 구절만 보던 검사를 지나 목적으로 저장됐다. 비어 있지 않은 모든 요청 구절을 검사하게 했다.
  2. 저장 파일에서 목적을 요청 문장으로 바꾸거나 근거에 없는 전제를 넣어도 출처가 남은 채 통과했다. 위의 재대조에 전제 인용 확인과 모델 줄의 생성 때 검사를 더했다.
  3. `source_ref`를 바꿔도 원래 출처처럼 보였다. 저장 검증에 ref 형식·origin digest 검사를, 재대조에 위치 확인을 더했다.
  4. 승인 대기 중 `/learn`이 저장한 미완료 호출에 나중에 결과가 붙으면 정상 session 성장만으로 store가 거절됐다. 그런 경우를 "changed after saving, not rechecked"로 낮췄다.
  5. 읽을 수 없는 origin session이 삭제된 session으로 보였다. 위의 "could not be checked" 오류로 닫게 했다.
  - 함께 지적된 200개 후보 재대조 비용(같은 조건에서 고치기 전 23–24초)을 위의 session 색인과 공유로 줄였다. 문서의 "저장 파일만 고친 경우를 잡는다" 문구도 실제 범위로 고쳤다.
  - 7차 후보 코드에서 바뀐 테스트 여섯 개가 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate7-fixes/`). 다섯은 위 다섯 결함 때문이고, 하나(`/new` 뒤 표시)는 끝 줄 문구가 바뀌었기 때문이다.
- **독립 Oracle gate 8차 판정(BLOCKED) 후 보정했다.** Oracle은 7차 표적 사례가 고쳐졌음을 확인한 뒤 다섯 우회를 지적했다.
  1. 제외한 선호 기록의 신뢰 구분을 `untrusted`로 낮추면 재대조 검사에서 빠져, 그 선호 문장을 목적에 넣어도 통과했다. 사용자 메시지의 신뢰 구분은 정확히 같아야 하게 했고, 검사에는 다시 계산한 신뢰 구분을 쓴다.
  2. 손으로 넣은 raw 비밀값이 검사 때만 가려지고 그대로 표시됐다. 저장 검증에 redaction 불변 검사를, 재대조에 검사 결과와 저장 글의 일치 검사를 더했고, 인용은 생성 때 보인 형태와만 비교한다.
  3. 피드백의 더 긴 숫자(`1234`)가 요청의 숫자(`123`)를 면제했다. 면제를 같은 낱말 일치로 바꿨다.
  4. origin과 출처 ref의 session 부분이 digest가 아니어도 받아들여 raw session key를 담을 수 있었다. digest·ID 형식 검사를 더했다.
  5. 링크·디렉터리인 origin session 항목이 삭제로 보였다. 목록이 건너뛴 모든 `.jsonl` 항목을 확인 불가로 본다.
  - 8차 후보 코드에서 바뀐 테스트 네 개가 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate8-fixes/`, 저장 파일 편집 테스트가 2·4를 함께 다룬다).
- **독립 Oracle gate 9차 판정(BLOCKED) 후 보정했다.** Oracle은 8차 다섯 보정이 유지됨을 확인한 뒤, 8차의 redaction 불변 검사가 `/learn` 자신이 쓴 글을 거절한다는 점을 재현했다. `shacs_redaction::redact_string`이 따옴표 없는 `키: 값`·`키=값`에서 이미 들어간 `[REDACTED]`를 다시 감싸(`[REDACTED]]`) 두 번 적용하면 결과가 달라졌고, 그래서 "Rotate the secrets: weekly" 같은 사용자 문장을 인용한 후보가 "malformed method record"로 저장되지 않았다(저장소가 망가졌다는 잘못된 안내 포함).
  - `shacs-redaction`에서 값이 정확히 `[REDACTED]`이면(뒤에 값의 끝이나 닫는 따옴표) 그대로 두게 해 redaction을 두 번 해도 같게 했다. 값 뒤에 다른 글이 이어지면 전처럼 다시 가린다.
  - 길이 제한으로 표시 중간이 잘리면(`token: [RED`) 다시 가릴 때 바뀌므로, `learned_method_evidence::bounded`가 자른 뒤 한 번 더 redaction해 저장하는 글이 항상 고정점이 되게 했다.
  - 회귀 테스트 두 개(`shacs-redaction`의 두 번 적용 테스트, `/learn` 저장·재시작 읽기 테스트)는 9차 후보 코드에서 실패하고 보정 후 통과한다(`.omo/evidence/spec038/prd000/gate9-fixes/`). 이 판정 무렵 테스트 파일이 길어져 저장 파일 편집 테스트를 `edits`, 요청 옮김 테스트를 `requests` 모듈로 옮겼다.
- **테스트(최소):** `shacs-command` router 2개. `shacs-core` `tests/it` `spec038_learned_methods` 19개: 근거 계산·제외·초안 입력 경계, 다음 요청 비사용과 재시작 읽기와 `/new` 뒤 출처·재대조 표시, 읽을 수 없는 origin session, 저장 뒤 결과가 붙은 미완료 호출, 후보 없음 세 경로, 사용자가 쓰지 않은 user 메시지(runtime의 출력 한도 복구 지시, system channel turn), 도구 요청 비실행과 실패 실행(성공으로 고친 파일의 거절 포함), 주입 turn·system channel turn의 `/learn` 거절, 한도를 넘거나 표시·문장·전제·출처 ref·위치를 고친 저장 파일, 요청 문장으로 바꾼 목적, 긴 선호·`untrusted` 글과 짧은 선호, 짧은 `untrusted` 답변·도구 결과, 선호를 되풀이한 답변, 요청 구절·한 낱말·기호뿐인 요청·구체적인 표시(피드백의 더 긴 숫자에 가려진 경우 포함)의 유출 시도, 신뢰 구분을 낮춘 선호·raw 비밀값·raw session key를 넣은 저장 파일, 링크로 바꾼 origin session, 비밀값 모양의 사용자 문장과 표시 중간에서 잘린 줄의 저장·읽기, 실행 판정(종료 코드·MCP·cron 포함)·노출·누락 수·session key·등록되지 않은 도구 이름, 같은 호출 ID의 재사용, 긴 "Run …" 문장의 저장 뒤 읽기. `shacs-core` `tools::mcp` 단위 테스트 assert 1개: `isError` 결과의 실패 변환. `shacs-redaction` 단위 테스트 1개: redaction을 두 번 해도 같음. `shacs-cli` `tests/it` compiled REPL 1개: 도구 요청 비실행 → 후보 → 재시작 뒤 같은 표시.
- **변경 package 테스트(5차 보정 후, 2026-10-24):** `shacs-core` lib 365 통과·1 실패(알려진 `mismatch_uses_isolated_tree`: 로컬 Spec 034 evidence 파일 없음), `tests/*` 1,220 통과·0 실패·3 무시, doc 2 통과. `shacs-cli` 474 통과·3 무시, `shacs-command` 17 통과. 사용자 turn의 저장 형식이 바뀌었으므로 session을 읽는 `shacs-api`도 돌렸다(121 통과). `test = false`로 일반 `cargo test`에서 빠지는 `spec034_release_runner`는 `--test '*'`로 함께 돌렸을 때 `InvalidConfig`로 실패했고, Spec 038 변경이 없는 기준 커밋 e48ce6f의 scratch clone에서도 똑같이 실패해 이 변경과 무관하다. fmt와 `clippy --workspace --all-targets -D warnings`는 통과했다.
- **실제 CLI QA:** 격리된 config·workspace와 fake provider로 compiled `agent` REPL을 tmux에서 실행했다.
  - 1차(`qa1/`): 작업·피드백·선호 → 도구를 요청한 생성 응답 거절(파일 미생성) → 후보 생성·조회 → 유사 요청 → 재시작 뒤 같은 조회 → 빈 대화와 인사만 있는 대화의 후보 없음.
  - 보정 후(`qa2/`): 같은 흐름에 더해, 선호만 인용한 단계와 근거 파일에 주입된 문장을 옮긴 단계가 버려지는 것, 같은 대화에서만 "(this conversation)"이 붙는 것을 확인했다. 준비한 응답 10개가 순서대로 모두 소비되어 예상하지 않은 provider 호출이 없었다. 저장 파일에는 근거 파일의 비밀값, 요청의 파일 이름, 선호 문장, 주입된 주소, session key가 없다.
  - 2차 보정 후(`qa3/`): 짧은 선호를 되풀이한 단계가 버려지는 것, `/new`·재시작·다른 session에서 원래 메시지가 지워졌다는 표시, 한도를 넘은 저장 파일의 오류와 파일 무변경을 확인했다. 준비한 응답 6개가 모두 소비됐다.
  - 3차 보정 후(`qa4/`): 직접 만든 session(종료 코드 1인 `exec`, "Email Bob" 답변)으로 `failed` 판정, 그 문장을 쓴 단계의 제외, 사용자 근거만 남는 `your own words`를 확인했다.
  - 4차 보정 후(`qa5/`): 직접 만든 session(요청의 코드명·경로, MCP 실패, cron 거절, 선호를 되풀이한 답변)으로 요청을 옮긴 초안의 거절, MCP·cron 실행의 `failed`, 경로·코드명을 담은 줄의 제외를 확인했다. 저장 파일에 코드명·경로·선호가 없다.
  - 5차 보정 후(`qa6/`): 실제 대화에서 사용자 turn에 `owner_turn` 표시가 붙는 것, 모델이 부른 등록되지 않은 도구가 `unregistered tool (failed)`로만 남는 것, 요청 문장("Check the release date.")을 그대로 쓴 목적의 거절과 그 문장을 쓴 단계의 제외를 확인했다. 직접 만든 session에서는 표시 없는 user 메시지(system channel turn의 저장 모양)가 `untrusted`로 남고 그 글을 옮긴 줄이 버려지는 것, 지어낸 도구 이름이 저장 파일에 없는 것, 표시를 고친 저장 파일이 `list`·`show` 모두 거절되고 파일이 바뀌지 않는 것을 확인했다. 데모 MCP 서버는 host의 MCP 시작 권한 경계에 막혀 시작되지 않았으므로 `isError` 경로는 단위 테스트로만 확인했다.
  - 6차 보정 후(`qa7/`): 한 낱말 요청 `zephyrcase`와 띄어 쓰지 않은 `보고서요약해줘`를 그대로 쓴 목적의 거절, 단계 문장을 바꾼 파일과 실패한 읽기를 성공 실행으로 바꾼 파일이 `list`·`show` 모두에서 거절되고 파일이 바뀌지 않는 것, 원래 파일로 되돌리면 다시 읽히는 것, `/new` 뒤 "your own words when saved … not rechecked"와 목록의 "some sources cleared since, not rechecked"를 확인했다. 준비한 응답이 모두 소비됐다.
  - 7차 보정 후(`qa8/`): 기호뿐인 요청 `🔒`을 그대로 쓴 목적의 거절, 목적을 요청으로 바꾼 파일·근거에 없는 전제를 넣은 파일의 거절("a method does not match the messages it cites")과 출처 ref를 위조한 파일의 거절("malformed method record"), 위치만 바꾼 파일의 "saved position not rechecked" 표시를 확인했다. 실제 승인 흐름에서 `write_file` 승인 대기 중 `/learn`이 저장한 미완료 호출에 승인 뒤 결과가 붙어도 store가 거절되지 않고 "its result changed after saving, not rechecked"로 보이는 것, 다른 session 파일을 읽을 수 없게 하면 "could not be checked against their sessions" 오류가 나고 되돌리면 다시 읽히는 것을 확인했다. 200개 후보의 재대조 시간은 `qa8/PERFORMANCE.md`에 남겼다.
  - 8차 보정 후(`qa9/`): 요청 `Summarize project 123`과 피드백의 `1234`가 있을 때 `Handle project 123` 목적의 거절, 선호 기록의 신뢰 구분을 낮추고 선호 문장을 목적에 넣은 파일의 거절("a method does not match the messages it cites"), raw 비밀값을 목적에 넣은 파일과 origin을 raw key로 바꾼 파일의 거절("malformed method record")을 확인했다. origin session 파일을 링크로 바꾸면 CLI가 시작 단계의 저장 자료 검사("stored-data migration plan is blocked")로 실행을 멈춰 `/learn`까지 가지 않으므로, 이 경우의 `/learn` 쪽 동작은 통합 테스트로만 확인했다.
  - 9차 보정 후(`qa10/`): "Rotate the secrets: weekly, then confirm the deploy"를 인용한 단계와 400자 제한이 redaction 표시 중간을 자르는 단계가 든 후보가 저장되고, 재시작 뒤 `your own words`·`[REDACTED] [truncated]`·"(this conversation)"으로 다시 읽히며 저장 파일에 `weekly`가 없음을 확인했다.
  - 기록은 로컬 `.omo/evidence/spec038/prd000/`에 있으며, 이 evidence가 없는 checkout에서는 실행 증명으로 보지 않는다.
- **남은 제한:**
  - 근거 창은 사용자 turn 수로 자르므로 오래된 요청은 빠질 수 있다(빠진 수를 표시).
  - 사용자 메시지의 종류는 모델이 메시지 단위로 분류한다. runtime은 분류된 제외 메시지가 초안 호출과 후보에 들어가지 않게만 보장하며, 잘못 분류된 메시지나 바꿔 쓴 표현까지 가려내지는 못한다.
  - 되풀이 검사는 글자 그대로만 찾는다(가려 둔 글 16자 이상, 가려 둔 글의 구절). 바꿔 쓴 표현이나 16자보다 짧은 구절의 일부는 찾지 못하며, 우연히 같은 표현은 버릴 수 있다(버린 수는 표시).
  - 종료 코드는 출력 마지막 줄의 `Exit code: N` 형식으로만 읽는다. 도구가 위 형식 밖의 자유로운 문장으로 실패를 알리면(예: plugin 도구) 알아보지 못한다.
  - 요청의 구체적인 표시는 경로·주소·파일 이름·숫자·대문자 코드 모양으로만 찾는다. 요청 구절을 통째로 옮기지 않고 그 안의 소문자 이름이나 띄어 쓴 한국어 고유명사 한 낱말만 옮기면 걸러지지 않는다. 반대로 요청에 한 낱말 구절이 있으면(예: "Summarize: notes"의 "summarize") 그 낱말을 쓴 줄은 버려진다.
  - 원래 session 대조는 저장 파일에서 남아 있는 근거로 다시 계산·검사할 수 있는 부분을 고친 경우를 잡는다. 근거 메시지가 사라진 뒤 고친 부분("not rechecked"로 표시), 근거 기록 자체를 지운 편집(예: 제외한 선호의 기록과 제외 항목을 함께 지우면 그 메시지가 선호였다는 사실을 다시 알 수 없다), 저장하지 않은 근거(인용하지 않은 답변·도구 결과)나 도구 결과·답변의 글을 옮겨 넣은 줄, 어떤 근거에도 없는 글로 바꾼 모델 문장(계속 "the model's wording"·"unverified"로 표시), 저장 파일과 session 파일을 함께 맞춰 고친 경우는 구별하지 못한다. 분류(`classified_as`)는 모델의 판단이라 다시 계산하지 않는다(분류를 바꾼 편집, 예를 들어 요청을 피드백으로 바꾼 편집은 그 메시지에 요청 옮김 검사를 다시 적용하지 않게 만든다). session 파일의 일부 줄만 깨진 경우에는 session 읽기가 그 줄을 건너뛰므로 그 근거는 "no longer in that session"으로 보인다.
  - 대조는 지금의 판정 규칙으로 다시 계산한다. 뒤 버전에서 실패 판정 규칙이 엄격해지면 이전에 저장된 실행 단계가 어긋남으로 거절될 수 있다.
  - `owner_turn` 표시는 이 기능부터 붙는다. 그 전에 저장된 사용자 메시지는 `untrusted`로 보고, 그 뒤의 모델 출력도 `untrusted`가 된다. 이전 대화를 이어 가면 새로 입력한 turn만 사용자 말로 쓰이며, 그런 turn이 근거 창에 없으면 후보 없음으로 끝난다.
  - 출처 확인은 메시지 시각과 digest로 한다. 원문 사본이 없으므로 지워진 근거는 되살릴 수 없다.
  - `observed_run`은 인용한 호출이 오류 없이 실행됐다는 사실뿐이다. 모델 설명이 그 호출과 같은 일을 가리키는지는 판정하지 않고 확인하지 않은 글로 표시한다.

## 종료 확인 (2026-10-24)

- 범위: `/learn`(도구 없는 두 번의 model 호출로 비활성 후보 하나를 만듦), `/learn list`, `/learn show <method-id>`. 사용자 말·실행·답변의 근거와 신뢰 구분, 선호·이번 작업 조건의 분리, runtime이 정하는 단계 근거, 효과 미귀속 표시, 후보 없음 정상 종료, workspace의 사용자 소유 저장과 재시작 뒤 조회, 원래 session과의 재대조를 포함한다. 확인·거절·승격·적용 조회(PRD 010)와 재사용·수정·되돌리기(PRD 020)는 포함하지 않는다.
- 최종 검증(변경한 package와 `shacs-redaction`을 쓰는 package, `--no-fail-fast`):
  - `shacs-core`: 1,608 passed, 1 failed, 3 ignored. 유일한 실패는 Spec034 로컬 evidence 파일 누락으로 baseline부터 실패하던 `runtime::generated_media_release::runner::fixture::tests::mismatch_uses_isolated_tree`다.
  - `shacs-cli` 474 passed / 3 ignored, `shacs-command` 17, `shacs-api` 121, `shacs-redaction` 13 passed.
  - `shacs-redaction`을 쓰는 `shacs-session` 105, `shacs-config` 85, `shacs-utils` 82, `shacs-projection` 359 passed / 7 ignored, `shacs-eval` 73, `shacs-app` 35 passed.
  - `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
  - 전체 workspace 실행은 사용자 승인을 받아 Spec 038 마무리 때 한다.
- 실제 표면: 격리된 config·workspace와 fake provider로 compiled `agent` REPL QA를 열 번 했다(`qa1/`–`qa10/`). 200개 후보 재대조 비용은 `qa8/PERFORMANCE.md`에 있다.
- Oracle 독립 검증: **PASS** (10차).
  - 1–9차는 BLOCKED였다. 지적과 보정은 위 구현 메모의 1–9차 항목에 있고, 각 차수의 회귀 테스트는 그 전 후보 코드에서 실패함을 확인했다(`gate*-fixes/`).
    - 제외 선호·untrusted 글·요청 원문의 유출 경로.
    - 실행 판정(JSON 오류, 가려진 결과, 종료 코드, MCP, cron).
    - 사용자가 쓰지 않은 user 메시지와 지어낸 도구 이름.
    - 저장 파일 편집과 원래 session 재대조.
    - redaction 고정점.
  - 10차는 차단 항목 없이 PASS였다. 비차단 의견은 고정점 테스트가 대표 입력을 나열한 방식이라는 점뿐이다.
- 남은 제한: 위 "남은 제한" 항목 그대로다. 근거 창, 모델의 분류, 글자 그대로만 찾는 되풀이 검사, 출력 형식 기반 실행 판정, 저장 파일과 session 파일을 함께 고치거나 근거 기록을 지운 편집은 구별하지 못한다는 점, `owner_turn` 이전 기록 등이 해당한다.

## 후속 변경 (PRD 010 작업, 2026-10-24)

PRD 010을 구현하며 이 PRD의 동작에 다음이 더해졌다. 위 종료 판정의 범위는 바뀌지 않는다.

- 실행 판정: `read_file`이 같은 실행 안의 읽기 캐시로 내용 대신 `[File unchanged since last read: …]`만 돌려준 결과는 `incomplete`다. 그 작업에서 아무것도 읽지 않았고, 결과 digest가 달라 같은 자료 판정을 피할 수 있기 때문이다.
- 저장: 방법마다 결정 목록(`decisions`)이 생겼고, 읽을 때 그 형식과 상태 계산도 확인한다([PRD 010](010-method-review-and-promotion.md)). 버전 출처에 원래 작업의 신원(`origin.task`: 근거 창 전체의 요청·메시지·도구 결과·호출 인자 digest)을 저장한다. `.shacs-methods/` 경로는 보호 대상이어서 모델의 파일 도구가 읽거나 바꾸지 못한다. 그 폴더로 가는 symlink나 저장 파일의 hard link로 가리켜도 마찬가지다. 폴더를 훑는 도구는 그 폴더를 건너뛴다. 그렇다고 저장 파일이 인증된 기록이 되지는 않는다(PRD 010 남은 제한).
- 표시: 목록에 상태를 만든 결정과 적용 작업이 보이고, `show`에 결정 절이 생겼다.

## 후속 변경 (PRD 020 작업, 2026-10-24)

- 저장: 사용자가 `/learn edit`로 만든 버전이 생겼다. 사용자가 쓴 글(`edited_in`, `user_edited` 단계)은 근거를 인용하지 않는다. 재대조의 요청 복사·인용 검사에서는 빠진다. 근거 기록과 원래 작업의 신원은 v1의 것을 그대로 둔다([PRD 020](020-method-reuse-edit-and-revert.md)).
