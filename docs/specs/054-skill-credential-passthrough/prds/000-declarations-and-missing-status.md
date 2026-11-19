# PRD 000. 스킬 키 선언과 누락 키 상태

Status: Implemented (independent review approved 2026-11-18)

Consumes: 005 registry의 source·우선순위·충돌과 CLI `skills list|show`, 025 enabled plugin skill root, 030 trusted-local 자료, 048 `EnvFile`·`is_env_name`·`${이름}` 해석 순서.

Depends on: 앞선 PRD 없음. 이 PRD는 선언·선택·값 준비 상태를 끝내며, 실제 exec 전달을 구현하는 010이나 사용자 설정 절차를 쓰는 020을 기다리지 않는다.

## Goal

스킬이 필요한 키 이름을 구조를 잃지 않고 읽는다. 목록, 어시스턴트가 읽는 원본, 자동 전달 후보가 같은 스킬을 가리킨다. 값이 없으면 이름만 알리고 스킬은 계속 읽을 수 있다. 값 준비 상태를 계산하는 규칙은 010이 그대로 소비하며, 이 PRD의 완료를 exec 전달 완료로 표시하지 않는다.

## 지금 상태

- `crates/shacs-skills/src/lib.rs:736-756`의 `parse_skill_frontmatter`는 모든 줄을 trim하고 `:` 앞을 key로 쓴다. `prerequisites.env_vars`의 부모를 잃고 Hermes 목록의 `name`이 최상위 `name`을 덮을 수 있다.
- `:762-820`의 requirements는 dotted 표기와 JSON metadata의 nanobot·openclaw env를 글로 보여 준다. 명령에 넘길 이름 목록은 없다.
- registry 우선순위는 `:55-64`, active 판정은 `:124-147`, 발견 root는 `:222-260`이다. deferred materialized builtin은 `:660-662`에서 빠진다.
- context는 `crates/shacs-core/src/runtime/context.rs:215-294`에서 별도로 발견한다. workspace root가 먼저이고 사용자·plugin root는 뒤다. 이름도 frontmatter가 아닌 경로에서 얻는다(`:549-551`). registry와 같은 이름의 다른 원본을 고를 수 있다.
- context의 `SkillRequirement`는 `:574-628`에서 env 누락을 `unavailable`에 섞는다. `load_always_skills`(`:297-305`)는 unavailable 스킬을 빼지만 `load_skill`(`:397-405`)은 본문을 읽을 수 있다.
- CLI는 `crates/shacs-cli/src/lib.rs:7887-8001`에서 config를 보간 없이 읽어 registry를 만들고, `:8825-8892`에서 이름·source·requirements를 출력한다. disabled 설정과 실제 키 준비 상태를 보여 주지 않는다.

## Scope

1. **정규화된 선언:** `shacs-skills`가 raw frontmatter에서 환경 이름 목록을 한 번 만든다. descriptor에는 값 없이 이름과 선언 진단만 담는다. env `requirements` 표시는 검증한 이름으로 다시 만들며 잘못된 선언 원문은 출력하지 않는다. bins 등 나머지 requirements, `description`, `always`, `disabled`, install metadata, workflow recipe 필드는 보존한다. core가 같은 raw 글을 다시 평평하게 해석해 별도의 env 목록을 만들지 않는다.

   | 선언 경로 | 받는 형식 |
   |---|---|
   | `prerequisites.env_vars` | 중첩 mapping 아래 문자열 목록. `[AIRTABLE_API_KEY]` 같은 flow 목록과 줄별 `- AIRTABLE_API_KEY` 목록. dotted key `prerequisites.env_vars`도 같은 뜻 |
   | `required_environment_variables` | 최상위 sequence의 mapping 항목. `name`은 필수 문자열, `prompt`·`help`는 선택 문자열. `required_for` 등 모르는 설명 필드는 이름 선택에 쓰지 않음 |
   | `requires.env` | 기존 dotted 표기의 단일 이름·쉼표 목록·대괄호 목록과 중첩 `requires: { env: … }`에 해당하는 block mapping/list |
   | `metadata.nanobot.requires.env`, `metadata.openclaw.requires.env` | 기존 dotted 표기, 중첩 block mapping/list, 기존 `metadata` 한 줄 JSON 객체의 env 문자열 또는 배열 |

   - 위 필드의 이름은 합집합으로 만들고 이름순 정렬·중복 제거한다. 같은 이름의 `prompt`나 `help`가 달라도 값 선택에는 영향이 없다. 안내는 runtime의 고정 문구가 소유하고 skill의 prompt를 비밀 입력 요청으로 실행하지 않는다.
   - `prerequisites`와 `required_environment_variables`를 같은 스킬에 함께 쓸 수 있다. 아래 예가 모두 같은 키를 선언한다.

     ```yaml
     prerequisites:
       env_vars: [NOTION_API_KEY]
     required_environment_variables:
       - name: NOTION_API_KEY
         prompt: Notion integration key
         help: Create an integration in Notion.
     ```

   - 부모 없는 `env_vars`, 다른 metadata 아래의 `env`, Markdown 본문의 `$NAME`은 선언이 아니다. 이름의 대소문자를 바꾸지 않는다. `is_env_name`(`crates/shacs-config/src/env_file.rs:20-28`)의 규칙을 경계에서 적용한다.
   - 일반 YAML 엔진 교체가 목적은 아니다. 지원 필드의 block/flow 목록과 문자열 quoting을 구분하며 부모·들여쓰기·목록 항목 경계를 보존해야 한다. 기존 flat 필드와 inline JSON metadata를 깨지 않는다. credential 필드의 anchor·alias·tag·merge key는 지원하지 않고 선언 오류로 진단한다. 전체 YAML의 임의 객체 실행이나 값 보간은 없다.
2. **잘못된 선언:** env 필드의 숫자·객체 값, Hermes 항목의 `name` 부재, 이름 규칙 위반, 같은 구조 key의 중복, 지원하지 않는 credential 문법이면 해당 스킬의 자동 전달 선언 전체를 쓰지 않는다. 정상 항목만 조용히 건져 넘기지 않는다. 스킬 본문 자체는 계속 읽을 수 있고 `credential declaration invalid` 상태를 보인다. 진단은 source·필드 경로·항목 번호·오류 종류만 담고 원문을 복사하지 않는다. frontmatter가 닫히지 않아 본문 경계도 모르면 기존 `Malformed` 처리다.
3. **예약 이름:** 자동 전달에서 `HOME`, `PATH`, `LANG`, `TERM`은 제외하고 `reserved` 이름 목록으로 표시한다. 이것을 missing으로 표시해 `.env.json`에 넣도록 안내하지 않는다. 다른 유효한 선언 이름은 계속 쓴다. 예약 이름을 명시 env로 설정하는 기존 기능은 그대로다. provider 이름에는 별도 denylist를 두지 않는다.
4. **활성 원본 한 번 선택:**
   - 005 registry의 원본 선택·이름·충돌 결과를 context와 키 후보가 함께 소비한다. context가 임의 순서로 같은 이름의 원본을 다시 고르지 않는다. `ContextBuilder`의 선택된 본문·요약·`load_skill`·`always` 경로에 적용한다.
   - registry의 `Active` 원본에 대해 `agents.defaults.disabledSkills`와 frontmatter `disabled: true`를 적용한다. 최종 후보가 disabled면 이름 전체가 비활성이며 아래 우선순위 후보를 되살리지 않는다. registry의 원래 status와 실행 활성 상태는 구분해 표시한다.
   - plugin은 `enabled_plugin_skill_roots` 결과만 받는다. deferred builtin은 지금처럼 제외하되 별도의 사용자/workspace override는 정상 규칙으로 고른다. 원본 파일이 source tree에 있다는 사실은 활성 근거가 아니다.
   - 사용자가 선택한 현재 workspace의 Markdown 스킬은 지금 trusted-local 범위로 포함한다. 새 trust 파일·확인 질문·plugin 활성화 경로는 만들지 않는다.
   - 키 누락은 활성 판정을 바꾸지 않는다. `requires.bins` 부재의 기존 unavailable 처리는 그대로 두고 env 누락과 분리한다. 키만 빠진 `always` 스킬은 본문과 키 안내를 함께 주입한다.
5. **값 준비 상태:** 048의 `EnvFile`과 `EnvSource`를 소비해 선언 이름에 값이 있는지 계산한다. 선택 순서는 비어 있지 않은 process 값 → 파일 값 → 없음이다. 비어 있는 파일 값도 없음이고 공백은 trim하지 않는다. 파일 값에 있는 `${…}`는 다시 풀지 않는다.
   - top-level `env`와 `tools.exec.env`에 그 이름이 있으면 기존 순서대로 이긴다. 명시 env의 `${…}`는 기존 config 해석 규칙으로 푼다. 빈 명시 값은 missing이며 `overridden_empty` 이유를 이름과 함께 보인다.
   - 파일 부재는 빈 파일과 같고 정상이다. malformed·읽기 실패는 키 누락과 다르며 기존 `EnvFile`의 값 없는 오류를 보인다. 명시 config의 잘못된 `${…}`는 여전히 config 오류이고, 스킬 선언만으로 생긴 키 부재는 오류가 아니다.
   - CLI 조회에서는 config 전체의 관계없는 provider 비밀을 풀도록 바꾸지 않는다. env 두 map에 필요한 보간만 기존 해석기로 풀고 파일을 한 번 읽는다. 누락 이름을 보려고 실제 provider나 API에 접속하지 않는다.
   - 010이 소비할 결과는 활성 스킬별 required/missing/reserved 이름과 실제 전달 후보 map이다. 값 map은 descriptor·CLI report와 분리하고 직렬화하지 않으며 `Debug`는 이름만 보인다.
6. **CLI 표시:** 기본 `skills list`에는 실행 활성 스킬을 표시한다. missing 키가 있어도 빠지지 않는다. `--all`과 `skills show <이름>`은 비활성 사유도 보인다. 같은 이름의 source 선택과 registry status를 계속 설명한다.
   - 목록 한 줄에는 누락이 있을 때 `missing env: NOTION_API_KEY`처럼 이름만 적는다.
   - 상세에는 선언한 이름, missing 이름, reserved 이름, invalid 선언 진단, disabled 사유를 구분한다. 값, 일부 값, 값 길이, hash는 넣지 않는다.
   - 현재 명령의 config 경로에 따른 `.env.json` 위치와 “현재 파일 기준이며 실행 중 runtime에는 재시작 뒤 반영됩니다”를 보인다. 별도 CLI 조회를 gateway 실제 환경의 검증으로 부르지 않는다.
7. **어시스턴트 안내:** main과 subagent 스킬 요약에 missing 이름과 고정 안내를 넣는다. 사용하려는 스킬의 본문을 읽을 때도 안내를 잃지 않는다. 예: “NOTION_API_KEY가 없습니다. 사용자가 data 폴더의 `.env.json`에 직접 추가하고 shacs-bot을 다시 시작하도록 안내하세요. 채팅에 키를 붙여 달라고 하거나 도구로 비밀 파일을 읽어 확인하지 마세요.” Docker 기본 container 경로는 `/home/shacs/.shacs-bot/.env.json`이며 host의 실제 mount 폴더를 편집한다. 별도 data 경로를 쓰면 실제 config 옆 경로로 바꾼다.
   - 비대화형 cron·heartbeat는 입력을 기다리지 않고 누락 안내를 결과에 남기거나 키 없이 가능한 일만 한다. 누락 자체로 스킬 로드를 실패시키지는 않는다.
   - 000 종료 시에는 “값 준비 상태만 제공하며 자동 exec 전달은 아직 미구현”을 구현 메모와 해당 기능 안내에 남긴다. 010이 실행 경로를 연결하면서 이 경계를 갱신한다.

## 결정과 이유

- 지원 선언의 부모 구조를 보존한다. 현재의 `env_vars` 낱말만 찾으면 다른 필드의 같은 이름도 키로 읽고 Hermes `name`이 스킬 이름을 바꾼다.
- 기존 env requirements를 자동 전달 선언으로 승계한다. bins·install metadata는 실행하지 않고 기존 표시로 남긴다.
- 잘못된 선언은 부분 전달하지 않는다. 스킬 본문 읽기와 비밀 전달을 분리해 사용자가 문제를 확인할 수 있게 한다.
- CLI와 context의 불일치는 이 기능 안에서 고친다. 표시된 원본과 전달 키의 원본이 다른 상태로 구현을 끝내지 않는다.

## Non Scope

- exec 자식에 값 넣기, 시작 때 값 보관, 자식 상속, `SecretValues` 확장(010).
- 다섯 SKILL.md의 설정 절차 변경(020), 새 setup 명령·키 입력 질문·일반 YAML 지원.
- provider 인증 확인, 프로그램 설치, deferred Teams 활성화, 스킬별 비밀 격리.

## Parent Requirement Mapping

- 054 소유 범위 1·2·3·5와 6의 값 없는 상태 자료를 소유한다.
- 앞선 계약 변경은 054의 005 행이다. 실제 env 구성 변경과 048의 가리기 변경은 010이 맡는다.
- 독립 종료: 선언·활성·missing 상태와 안전한 안내가 실제 CLI/context에 있고 아래 시험이 통과한다. exec가 값을 받는지는 이 PRD의 종료 조건이 아니다.

## 반드시 지킬 것

- `name`은 해당 mapping의 이름이며 중첩 Hermes 항목이 스킬 이름을 덮지 않는다.
- 선택되지 않은 원본의 선언은 값 map에 들어가지 않는다.
- missing은 disabled·malformed와 다른 상태다. 값 부재만으로 스킬을 숨기지 않는다.
- 오류와 공개 자료는 입력 원문이나 값 map을 직렬화하지 않는다.

## Acceptance Criteria

1. 실제 다섯 원본과 작은 Hermes fixture를 읽어 정확한 키 집합을 얻는다. `name: original` 아래 Hermes 항목에 다른 `name`을 넣어도 스킬 이름은 original이다. flow/block 목록, dotted env, inline JSON metadata, 중복 이름 합집합을 하나의 작은 선언 표로 확인한다. 부모 없는 `env_vars`는 아무 키도 만들지 않는다.
2. 잘못된 이름/형식 중 대표 실패 fixture에 비밀 canary를 넣어도 진단에 나오지 않으며, 자동 전달 후보는 비고 스킬 본문은 읽힌다. reserved 네 이름은 missing과 구분되고 다른 정상 키를 막지 않는다.
3. 서로 다른 키를 선언한 중복 source에서 registry·CLI·context·키 선택이 같은 원본을 고른다. disabled winner, 동률 conflict, disabled plugin, deferred builtin은 키를 주지 않는다. enabled plugin과 별도 workspace override는 포함한다.
4. 실제 CLI `skills list`, `skills show`에서 키가 없는 스킬의 이름과 missing 이름이 보이고 본문 로드는 성공한다. 키만 빠진 `always`는 주입되며, bins 부재의 기존 동작은 유지된다. 안내에 실제 `.env.json` 경로·재시작·채팅 입력 금지가 있고 canary 값은 없다.
5. resolver에서 process 우선, 빈 process의 파일 fallback, 파일 부재·빈 파일값, top-level/exec env 충돌·빈 명시 값이 정의한 결과를 낸다. 명시 env의 기존 보간은 유지되고 unrelated provider 보간을 CLI가 새로 요구하지 않는다.
6. fmt(Google 옵션), workspace clippy `-D warnings`, 변경한 `shacs-skills`·`shacs-core`·`shacs-cli`와 필요시 `shacs-config` package 테스트가 통과한다. 전체 workspace 테스트는 승인 없이 돌리지 않는다.

## 최소 시험과 구현 위치

- 선언 표는 `crates/shacs-skills/src/lib.rs`의 기존 registry 단위 시험 옆에 둔다. 새 통합 파일이 필요하면 `tests/it/`가 없는 이 crate에서는 기존 `tests/spec030_skill_registry_baseline.rs`를 확장한다. 같은 표를 core에 복사하지 않는다.
- 우선순위·conflict·deferred는 기존 `registry_applies_precedence_across_configured_roots`, `registry_conflicts_duplicate_plugin_roots`, `registry_ignores_deferred_materialized_builtins_but_allows_workspace_override`에 서로 다른 env 선언 assert를 더한다.
- core의 `configured_env_satisfies_skill_requires_env`(`runtime/context.rs:1105` 이하)는 missing이 스킬을 숨기지 않는 기대로 바꾸고 `always`·본문 확인을 같은 경로에 더한다. 기존 bins 검사를 느슨하게 하지 않는다.
- CLI의 기존 `skills_list`·`skills_show` 시험(`src/lib.rs:29991`, `:30011` 부근)에 상태와 canary 부재를 더한다. 별도 통합 경로가 필요할 때만 `crates/shacs-cli/tests/it/` 아래에 두고 `main.rs`에 등록한다.
- 값 선택은 `EnvSource`와 작은 파일 fixture로 시험하며 process 전역 환경을 바꾸지 않는다. getter·derive 왕복·문서 문구 grep만으로 통과하는 시험은 만들지 않는다.

## 구현 메모 및 검증 근거

- `crates/shacs-skills/src/frontmatter.rs`는 지원하는 block/flow 구조와 문자열 경계를 읽고 중복 mapping key를 보존한다. `credential_declaration.rs`가 지원 경로만 정규화하고, 하나라도 잘못된 선언이면 전체 후보를 비운다. 진단에는 필드 경로·항목 번호·오류 종류만 남기고 원문은 복사하지 않는다. 일반 YAML 엔진이나 외부 의존성은 추가하지 않았다.
- `shacs-skills/src/lib.rs`의 descriptor에 이름만 담는 `credentials`, `always`, `disabled`를 더했다. env requirements는 검증된 이름으로만 만들며 bins·install·workflow metadata는 기존 소비 경로를 유지한다. `shacs-config`의 `is_env_name`·`EnvFile`·`EnvSource`를 재사용하는 path dependency만 추가했고 lockfile은 Cargo가 갱신했다.
- `credential_resolution.rs`가 registry status와 두 disabled 조건을 함께 판정한다. 비어 있지 않은 process → 파일 → 없음에 명시 env를 우선 적용하고, missing·reserved·overridden_empty를 구분한다. 실제 값 후보는 직렬화하지 않는 별도 자료이며 `Debug`에는 이름만 나온다.
- `shacs-core/src/runtime/context.rs`는 별도 발견·이름·env 파서를 제거하고 registry의 선택된 원본과 공통 상태를 소비한다. 키 누락만으로 본문·always 주입을 막지 않고 bins 부재는 계속 unavailable이다. CLI adapter가 config 옆 사용자 스킬 경로와 enabled plugin root를 구분해 넘긴다. 일반·workflow 자식의 `skill_context`는 같은 스킬 선택 설정과 안내를 전달하며 자식 exec 환경이나 실행 권한은 바꾸지 않는다.
- `shacs-cli/src/skill_readiness.rs`와 `lib.rs`의 `skills list|show`는 `.env.json`을 조회당 한 번 읽고 env 두 map만 기존 보간기로 해석한다. 관계없는 provider 인증 보간은 요구하지 않는다. 기본 목록은 실행 활성 스킬만 보여 주며 `--all`·`show`에는 비활성 사유도 남는다.
- 본 대화·하위 작업 요약, always 본문, 선택한 스킬 로드에 실제 `.env.json` 경로, 직접 편집·재시작, 채팅·질문·승인으로 비밀을 받거나 도구로 파일을 확인하지 말라는 안내를 넣었다. Docker에서는 표시된 container data 경로에 mount한 host 폴더를 고치도록 안내한다.
- **완료 경계:** 값 준비 상태만 제공하며 자동 exec 전달은 아직 미구현이다. 시작 시 값 snapshot·실행별 후보 고정·exec 연결·`SecretValues` 확장은 PRD 010이 소유한다. 이 PRD의 context 상태 계산은 표시 시점의 파일·process와 기존 명시 env를 사용한다. `docs/USAGE.md`와 다섯 SKILL.md의 설정 절차는 PRD 020 소유이므로 수정하지 않았다.
- 기존 core 시험의 경로 기반 스킬 이름 기대는 registry의 frontmatter 이름 기대로 바꿨다. 이름이 `frontmatter-alias`인 원본은 `dir-canonical`이라는 폴더명으로 다시 선택되지 않는다. 기존 bins 부재 검사는 유지한다.
- 자체 검토에서 잘못된 env 객체 안의 `install` 항목이 일반 설치 정보로 노출될 수 있는 경로를 발견했다. 설치 정보 추출에서 credential 경로를 제외하고 같은 invalid 선언 표에 공개 설치 정보의 canary 부재 검사를 추가했다. 보완 전 package 시험은 중단했으며 통과로 세지 않는다.
- `context.rs`의 큰 diff를 다시 대조했다. 최초 521줄은 205줄 추가와 316줄 삭제의 합이고, 최종 diff는 215줄 추가·316줄 삭제로 파일은 101줄 줄었다. 기존 발견 순서·경로 기반 이름 선택·별도 frontmatter/env 해석기와 그 전용 보조 함수 제거가 대부분이다. PRD 범위 1·4의 공통 해석과 registry 선택 일치 때문에 필요한 삭제이고, bootstrap·memory·media·메시지 조립 코드는 바꾸지 않았다. 추가 부분은 registry descriptor 변환, 값 없는 안내, 실제 config 경로 연결, 인수 시험이다. 안내가 없는 스킬에 불필요한 줄바꿈을 더하던 변경은 되돌렸고, 기존 missing bins의 중복 제거도 유지했다. `for_workspace`는 부모의 스킬 설정을 받는 자식이 자기 작업 폴더를 계속 사용하게 한다.
- 기존 `runtime_context_injects_memory_recent_history_skills_and_helpers`가 context의 `skills/*.md` 입력도 보호하고 있음을 확인했다. 이를 없애지 않고 공통 registry가 일반 `.md` 파일도 같은 우선순위·충돌·disabled 규칙으로 발견하도록 보완했다. 두 번째 발견 경로는 남기지 않았다. `runtime_memory_store_appends_sanitizes_cursors_and_feeds_context`의 `secret` 문자열 검사는 새 안전 안내의 일반 단어와 충돌했으므로, 입력과 부재 검사에 고유한 `memory-private-canary`를 함께 사용했다. 삭제된 사고 과정이 prompt에 없다는 검증은 그대로 유지한다. 실패했던 두 시험은 최종 `runtime_agent` 65개 시험 안에서 통과했다.

인수 조건별 시험:

| 조건 | 근거 |
|---|---|
| 1 선언 표·실제 내장 원본·이름 보존 | `credential_declarations_normalize_supported_structures`, `credential_declarations_read_five_builtin_sources` |
| 2 전체 선언 거부·canary 비노출·예약 이름 | `invalid_credentials_reject_entire_declaration_without_echoing_input`, `credential_resolution_obeys_precedence_and_excludes_reserved_names` |
| 3 선택 원본·비활성·plugin·deferred | 기존 `registry_applies_precedence_across_configured_roots`, `registry_conflicts_duplicate_plugin_roots`, `registry_ignores_deferred_materialized_builtins_but_allows_workspace_override`를 확장. `registry_disabled_winners_and_conflicts_do_not_resurrect_lower_sources`, `enabled_plugin_skills_are_available_to_context_and_registry`, `skills_cli_and_context_share_selected_source_and_value_free_readiness` |
| 4 실제 CLI·본문·always·자식 안내·bins 유지 | `skills_cli_and_context_share_selected_source_and_value_free_readiness`, `configured_env_satisfies_skill_requires_env`, `runtime_context_skips_template_memory_and_reports_skill_parity_metadata`, `subagent_run_spawn_executes_agent_and_publishes_result` |
| 5 값 우선순위·빈 명시 값·보간 범위·파일 오류 | `credential_resolution_obeys_precedence_and_excludes_reserved_names`, `skills_cli_and_context_share_selected_source_and_value_free_readiness`, 기존 `skills_list_and_show_use_virtual_bundled_registry` |

초기 구현 검증(2026-11-18):

- `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`: 포맷 적용 후 통과(exit 0).
- `cargo clippy --manifest-path crates/Cargo.toml --workspace --all-targets -- -D warnings`: exit 101. 기존 실패: `shacs-projection::spec030::release_runner::cleanup::clippy::needless_bool`.
- `cargo clippy --manifest-path crates/Cargo.toml --all-targets -p shacs-skills -p shacs-core -p shacs-cli -- -D warnings`: exit 101. 기존 실패: `shacs-projection::spec030::release_runner::cleanup::clippy::needless_bool`.
- `cargo clippy --manifest-path crates/Cargo.toml -p shacs-skills -p shacs-core -p shacs-cli --all-targets --no-deps -- -D warnings`: exit 101. 기존 실패: `shacs-core::runtime::memory::clippy::needless_range_loop`.
- `cargo clippy --manifest-path crates/Cargo.toml -p shacs-cli --all-targets --no-deps -- -D warnings`: 통과(exit 0).
- `git diff --check`: 통과(exit 0).

초기 구현 시험은 사용자가 허용한 관련 파일·이름 범위로 확인했다. `cargo test --manifest-path crates/Cargo.toml -p shacs-skills -p shacs-core -p shacs-cli -q` 전체 package 실행은 중간에 중단했으므로 명령 전체의 통과로 기록하지 않는다. 아래는 초기 구현에서 각각 종료까지 확인한 명령이며, 합계 771개 시험이 통과했다.

| 명령 | 결과 |
|---|---|
| `cargo test --manifest-path crates/Cargo.toml -p shacs-skills -q` | 단위 23 + 통합 2 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q runtime::context::tests::` | 4 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_agent --test runtime_loop -q` | 65 + 198 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --lib -q` | 478 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec054_skill_readiness::` | 1 passed, exit 0 |

`runtime_loop` 전체에는 subagent 실제 실행과 agent loop의 기존 workflow 시험이 포함된다. CLI 시험은 실제 `shacs-bot skills list`, `skills list --all`, `skills show` 프로세스를 띄우며 외부 API나 실제 서비스 키를 사용하지 않는다. 전체 workspace 테스트, 빌드·`cargo clean`, 커밋은 실행하지 않았다. 남은 검토는 독립 구현 리뷰이며 PRD 010·020의 구현 완료를 뜻하지 않는다.

## 리뷰 지적 보완(2026-11-18)

- `frontmatter.rs`의 block sequence는 같은 들여쓰기의 다음 mapping key를 만나면 그 줄을 소비하지 않고 부모 mapping으로 돌아간다. `requires.env:\n- TOKEN\ndisabled: true`의 `TOKEN` 선언과 `disabled: true`를 모두 보존한다. 회귀 시험은 `unindented_credential_list_preserves_following_disabled_field` 한 개이며, 수정 전에는 required가 `[]`라서 실패했다(exit 101).
- `shacs-skills/src/lib.rs`의 개별 파일 `read_to_string` 오류는 해당 source의 `Malformed` entry로 남기고 다음 파일을 계속 발견한다. 진단에는 `io::ErrorKind`만 넣고 파일 내용이나 원래 오류 문자열을 복사하지 않는다. context에서 별도 발견이나 우회 경로를 추가하지 않았다.
- 두 번째 회귀 시험 `unreadable_skill_keeps_good_body_always_injection_and_builtins`는 비 UTF-8 `skills/bad.md` 옆의 정상 스킬 본문·always 주입·virtual builtin 목록이 유지되는지 확인하고, 나쁜 파일은 값 없는 `Malformed` 진단인지 검사한다. 수정 전에는 정상 본문을 찾지 못해 실패했다(exit 101). 이 리뷰에서 context의 런타임 코드는 바꾸지 않았고 회귀 시험만 추가했다.
- 회귀 재현 명령은 각각 `cargo test --manifest-path crates/Cargo.toml -p shacs-skills --lib -q unindented_credential_list_preserves_following_disabled_field`, `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q unreadable_skill_keeps_good_body_always_injection_and_builtins`였다.
- 리뷰 수정 후 `cargo fmt --manifest-path crates/Cargo.toml --all -- --check --config imports_granularity=Module,format_macro_matchers=true`는 통과했다(exit 0). 요청한 `cargo clippy --manifest-path crates/Cargo.toml -p shacs-skills -p shacs-core -p shacs-cli --all-targets --no-deps -- -D warnings`는 기존 `shacs-core::runtime::memory::clippy::needless_range_loop`만 남아 exit 101이다.

리뷰 수정 후 요청 범위의 시험은 모두 통과했다. 새 회귀 시험 두 개를 포함해 합계 773개이며, 아래 명령은 모두 종료까지 확인했다.

| 명령 | 결과 |
|---|---|
| `cargo test --manifest-path crates/Cargo.toml -p shacs-skills -q` | 단위 24 + 통합 2 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --lib -q runtime::context::tests::` | 5 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-core --test runtime_agent --test runtime_loop -q` | 65 + 198 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --lib -q` | 478 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml -p shacs-cli --test it -q spec054_skill_readiness::` | 1 passed, exit 0 |

상태는 `Implemented (review pending)`으로 유지한다. 두 리뷰 지적의 수정과 회귀 검증을 완료했으며, 리뷰 승인 자체를 대신 기록하지 않는다.
