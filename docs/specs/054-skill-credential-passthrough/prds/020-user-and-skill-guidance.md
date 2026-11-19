# PRD 020. 사용자 설정과 내장 스킬 안내

Status: Implemented (independent review approved 2026-11-18)

Consumes: 048 `.env.json` 형식·비밀 파일 규칙, 049 Chromium·viewer의 빈 환경, 052 Docker data mount·재시작 운영, 030 raw content 고지.

Depends on: [PRD 000](000-declarations-and-missing-status.md)의 선언·누락 상태와 [PRD 010](010-exec-environment-passthrough.md)의 실제 전달이 닫힌 결과. 다른 스펙의 남은 작업을 기다리지 않는다.

## Goal

사용자가 실제 읽히는 파일에 키를 한 번 적고 runtime을 다시 시작하는 절차로 스킬을 쓸 수 있게 한다. Docker host의 data 폴더와 container 안 경로를 혼동하지 않게 설명하고, 다섯 내장 원본의 잘못되거나 불분명한 env 안내를 고친다.

## 지금 상태

- 구현 전 `docs/USAGE.md:97`은 `.env.json` 값을 환경에 내보내지 않는다고 했다. 010에서 사용자 지시로 남긴 이 일반 설명의 정정도 이 PRD에서 스킬 키 절과 함께 완료한다. 기존 `:36`, `:111`의 browser/viewer 빈 환경 규칙은 유지한다.
- `crates/shacs-skills/airtable/SKILL.md:32-35`는 `~/.shacs-bot/.env`와 `KEY=value` 예를 쓰고 `:228`은 스킬을 읽으면 자동 전달된다고 한다.
- `crates/shacs-skills/gif-search/SKILL.md:29-33`은 같은 `.env`와 shell 형식이다.
- `crates/shacs-skills/linear/SKILL.md:26`은 `config/env`라는 안내만 있고 실제 파일·JSON 예가 없다. `.env`라는 글이 있다는 전제로 바꾸지 않는다.
- `crates/shacs-skills/notion/SKILL.md:32-35`는 `.env`를 안내하고, `:50-56`은 `NOTION_API_TOKEN`·`NOTION_KEYRING` export를 profile 또는 `.env`에 넣으라고 한다. 이 스킬의 선언 이름은 `NOTION_API_KEY` 하나다.
- `crates/shacs-skills/teams-meeting-pipeline/SKILL.md:49-55`는 Graph 세 값을 `.env`에 넣으라고 한다. 이 파일은 `disabled: true`(`:18`)이고 `deferred_builtins.txt:18`에 있어 기본 활성 대상이 아니다.
- `docker-compose.yml:8`의 기본 mount는 host `~/.shacs-bot` → container `/home/shacs/.shacs-bot`이다. config 옆 파일이라는 기준은 `crates/shacs-config/src/env_file.rs:13-17`이다.
- `sync_builtin_skills`(`crates/shacs-skills/src/lib.rs:187-219`)는 이미 있는 파일을 덮지 않는다. source 문서를 고쳐도 기존 materialized 파일이 자동으로 갱신된다고 말할 수 없다.

## Scope

1. **USAGE 스킬 키 절:** 아래 절차와 제한을 한 곳에서 설명하고 `${이름}`·exec env 설명에서 참조한다.
   - 실제 data 폴더의 `config.json` 옆 `.env.json`을 사용자가 직접 편집한다. 기존 파일이 있으면 다른 키를 보존하며 JSON 객체에 항목을 추가한다.
   - JSON 예는 다음처럼 문자열 이름·값 한 쌍이다. 아래 값은 예시이며 실제 키는 사용자 자신의 편집기에서 넣는다.

     ```json
     {
       "NOTION_API_KEY": "ntn_example_replace_locally"
     }
     ```

   - `KEY=value`, `export`, shell 변수 확장을 JSON 안에 적지 않는다. 직접 만든 파일의 권한은 048처럼 `chmod 600`으로 안내한다. 실제 키를 포함한 명령줄·채팅 예는 만들지 않는다.
   - config env 항목을 따로 추가할 필요가 없다. 활성 스킬이 선언한 키만 자동 전달한다. process의 비어 있지 않은 값이 파일보다 우선하고, 명시 top-level `env`·`tools.exec.env`가 자동 전달을 덮는다는 순서를 적는다. 빈 명시 값이 파일 값을 가릴 수 있음도 적는다.
   - `.env.json`을 고친 뒤 gateway/API 등 실제 실행 중 runtime을 재시작한다. `skills list`·`skills show notion`은 현재 파일 기준의 이름 상태이고 장기 실행 runtime의 값 확인이 아니라는 차이를 적는다.
   - 값 확인은 키를 출력하는 `env`·`printenv`·`echo $KEY`로 하지 않는다. `skills show`의 missing 이름과 키를 출력하지 않는 실제 작업 성공으로 확인한다. CLI 조회의 ready만으로 API 권한·유효성까지 확인됐다고 말하지 않는다.
2. **Docker 경로:**
   - 기본 host 파일은 `~/.shacs-bot/.env.json`, container에서는 `/home/shacs/.shacs-bot/.env.json`이다. 실제 mount source를 바꿨으면 그 host 폴더 안 파일을 편집한다. container의 일회용 다른 폴더에 만들지 않는다.
   - config 경로를 직접 바꿨으면 그 파일 옆이라는 규칙이 우선한다. 실제 경로를 알 수 없는 채팅에서는 host 경로를 만들어 내지 말고 사용자가 mount한 data 폴더를 가리킨다.
   - Compose 치환용 저장소 `.env`와 shacs-bot 비밀 파일 `.env.json`을 구분한다. 스킬 키를 Compose env 항목으로 복사할 필요가 없으며 모든 process 환경을 상속시키는 예도 넣지 않는다.
   - 재시작 명령은 USAGE의 현재 Docker 운영 절차를 참조한다. 사용자가 실행하지 않은 deploy·image rebuild를 이 문서 작업의 검증으로 기록하지 않는다.
3. **다섯 원본의 변경 범위:**

   | 파일 | 고칠 안내 |
   |---|---|
   | `crates/shacs-skills/airtable/SKILL.md` | `AIRTABLE_API_KEY` JSON 예·data 경로·재시작. “스킬을 읽으면 전달”을 “활성 스킬의 선언 키가 모든 exec에 전달”로 고침 |
   | `crates/shacs-skills/gif-search/SKILL.md` | `TENOR_API_KEY` JSON 예와 같은 절차 |
   | `crates/shacs-skills/linear/SKILL.md` | 모호한 `config/env`를 `LINEAR_API_KEY` JSON 예와 같은 절차로 구체화 |
   | `crates/shacs-skills/notion/SKILL.md` | `NOTION_API_KEY` JSON 예. ntn 별칭은 그 CLI를 부르는 **같은 exec 명령** 안에서 `NOTION_API_TOKEN="$NOTION_API_KEY" NOTION_KEYRING=0 ntn …`처럼 지정. `.env.json`에 `$NOTION_API_KEY`를 넣거나 다른 exec의 export가 남는다고 안내하지 않음 |
   | `crates/shacs-skills/teams-meeting-pipeline/SKILL.md` | 세 Graph 이름의 JSON 예와 data 경로를 정정하되 deferred·disabled 표시는 그대로 둠. 키를 넣어도 shacs-bot의 Teams pipeline이 활성화·지원되지 않음을 명시 |

   - API 사용법·권한 범위·네트워크 절차·script 코드는 이 PRD 때문에 고치지 않는다.
   - 각 안내에는 비밀을 채팅으로 받지 말고 사용자가 로컬에서 직접 설정하게 하라는 문장을 둔다. 기존 선언 필드는 유지한다.
4. **기존 설치:** 사용자가 이미 가진 `workspace/builtin_skills/<이름>/SKILL.md`는 자동 덮어쓰지 않는다. 오래된 `.env` 안내가 보이면 사용자 변경을 보존하며 현재 배포 원본의 설정 절을 비교·수동 반영하도록 설명한다. 일반 `.env` 파일을 삭제하거나 자동 변환하지 않는다.
5. **한계 고지:** 활성 스킬 하나가 선언한 키는 그 runtime 실행의 다른 exec에도 보인다. shell과 자식 프로그램이 키를 출력하면 일반 tool 결과·모델 입력·transcript·로그에 원문이 남을 수 있다. 048의 classifier 가리기는 8자 이상인 알려진 값과 정해진 표면에 한정된다. 키가 있다는 사실은 명령 실행·workspace 밖 접근 승인을 생략하지 않는다.
6. **바꾸지 않는 안내:** Chromium·Xvfb·x11vnc·websockify의 빈 환경, viewer 비밀번호 처리, provider 인증 설정, MCP env, 기존 `.env`의 Compose 용도는 그대로 둔다. 앞선 numbered spec 문서는 고치지 않는다.

## 결정과 이유

- 새 setup 명령을 만들지 않는다. 파일 편집과 현재 skills 조회만으로 목표 경험을 설명할 수 있다.
- Notion 별칭은 명령 안에서 만든다. `.env.json`은 shell 파일이 아니며 자동 전달이 임의 별칭 확장을 하는 것도 아니다.
- Teams 안내도 고치되 계속 참고 자료로 남긴다. 잘못된 파일명을 바로잡는 것이 미지원 runtime을 구현했다는 뜻이 되면 안 된다.
- 기존 materialized 파일은 사용자가 고쳤을 수 있다. 자동 덮어쓰기 대신 차이를 확인하는 절차를 적는다.

## Non Scope

- Rust·config·Compose·script 변경, 스킬의 새 API 기능, Teams 활성화, 비밀 입력 UI.
- `.env` 자동 이전, 사용자 materialized 파일 덮어쓰기, 실제 외부 API 키를 사용하는 검증.

## Parent Requirement Mapping

- 054 소유 범위 7과 수용 조건 6을 소유한다. 010이 바꾼 실행 설명을 소비해 사용자 절차로 완성한다.
- 앞선 계약 변경은 054의 048 exec 전달 행에 해당하는 `docs/USAGE.md` 안내다. 048 원문은 고치지 않는다.
- 독립 종료: USAGE와 다섯 원본이 구현된 동작·형식·경로·한계와 일치하고 아래 문서 확인이 끝난다.

## 반드시 지킬 것

- `.env.json` 예는 유효한 JSON이며 기존 키를 보존하도록 안내한다.
- 키 원문을 채팅·명령줄 인자·출력에 넣게 하지 않는다.
- Docker host 경로와 container 경로, Compose `.env`와 runtime `.env.json`을 구분한다.
- Teams deferred 상태와 browser 빈 환경은 바뀌지 않는다.

## Acceptance Criteria

1. USAGE 한 절만 읽고 파일 위치·JSON 형식·수정 후 재시작·조회 명령을 알 수 있다. config env를 따로 쓰는 단계가 필수로 남지 않는다.
2. 다섯 원본의 설정 절을 읽어 실제 키 이름·JSON 예·Docker 경로·채팅 입력 금지가 맞는다. Airtable의 loaded-only 설명, Notion의 `.env`/shell-profile 상속 설명이 제거되고 Linear의 모호한 안내가 구체화된다.
3. Teams의 Graph 세 이름 예는 맞지만 파일의 disabled/deferred 표시가 유지되고, key 설정만으로 사용할 수 있다는 문장이 없다.
4. 가짜 키와 임시 data를 쓰는 010의 완료 증거를 설정 예에 대조한다. 예시 JSON은 파싱되며 canary 값 출력 없이 존재·작업 성공을 확인하는 절차다. 실제 서비스 키나 인터넷 API 호출을 문서 검증에 요구하지 않는다.
5. browser/viewer 빈 환경 문구와 Compose `.env`의 기존 용도가 보존된다. raw output의 한계를 키가 절대 노출되지 않는다는 약속으로 바꾸지 않는다.
6. `.md`만 변경하면 Cargo 검증을 생략한다. 문서 문구를 grep해 성공하는 새 Rust 시험은 만들지 않는다.

## 최소 확인

- 변경한 여섯 문서를 직접 읽고 경로·키 이름·예시 JSON·비밀 입력 금지·재시작·deferred 상태를 확인한다. `.env`가 남은 곳은 Compose 설명인지 잘못된 runtime 안내인지 문맥으로 구분한다.
- JSON 예를 작은 파싱 확인으로 검증하고 010의 실제 canary 실행 증거를 참조한다. API 연결을 새로 돌리거나 이미 통과한 Cargo 시험을 반복하지 않는다.
- 아직 실제 Docker에서 따라 하지 않았다면 문서 검토와 Docker 실행 확인을 구분해 보고한다.

## 구현 및 검증 기록(2026-11-18)

변경 파일은 `docs/USAGE.md`, `README.md`, `crates/shacs-skills/{airtable,gif-search,linear,notion,teams-meeting-pipeline}/SKILL.md`, 이 PRD의 총 8개 Markdown 문서다. PRD 000·010의 기존 미커밋 코드, `SPEC.md`, 앞선 스펙, Compose, 스크립트, 생성 Rust 파일은 수정하지 않았다.

계획만 옮기지 않고 다음 구현을 대조했다.

- `credential_declaration.rs`는 지원 frontmatter 경로에서 이름을 읽고 `HOME`·`PATH`·`LANG`·`TERM`을 reserved로 분리한다. `credential_resolution.rs`는 registry의 Active와 disabled 조건을 적용하고 명시 env → 비어 있지 않은 process → 파일 순으로 값을 고른다. 명시 빈 값은 `overridden_empty`와 missing으로 표시한다.
- `RuntimeEnvironment::resolve`는 시작 때 process snapshot과 config 해석에 쓴 같은 파일을 보관한다. `ContextBuilder::select_skills`는 실행 시작의 원본·본문·상태·선언값을 고정하고 CLI adapter는 이 값을 실행 문맥과 자식에 연결한다. `SkillEnvironment`는 메모리에만 값을 보관한다.
- `configured_exec_env`는 top-level 뒤에 `tools.exec.env`를 덮고, `ExecTool::build_env`는 기본 환경·기존 허용 목록 뒤와 명시 env 앞에 스킬 값을 합친다. 스킬을 읽었는지는 조건이 아니다. deferred builtin 목록과 `sync_builtin_skills`의 기존 파일 보존도 확인했다.
- `skill_readiness::statuses`는 CLI 조회 때 현재 파일·process로 이름 상태를 계산한다. `PermissionEnvironment::with_skill_environment`와 `SecretValues::extend`는 실제 전달값을 기존 8자 이상 가리기 대상에 추가하며 일반 tool 출력·transcript·로그를 전면 정리하지 않는다.

수용 조건별 근거:

| 조건 | 문서와 확인 결과 |
|---|---|
| 1 파일·형식·재시작·조회, config 재등록 불필요 | USAGE의 `스킬 API 키 설정` 한 절에 로컬/Docker 경로, JSON, 권한, 재시작·container 재생성 구분, `skills list`·`skills show notion`, 현재 조회와 장기 실행 runtime의 차이를 모았다. 기존 `${이름}`·exec env 설명과 README에서 연결한다 |
| 2 다섯 원본의 올바른 안내 | 다섯 설정 절 모두 실제 config 옆 `.env.json`, 기존 키 보존, host/container 경로, 직접 편집·채팅 입력 금지·재시작을 설명한다. Airtable의 loaded-only 조건과 Linear의 `config/env` 모호함을 제거했다. Notion은 같은 exec의 `NOTION_API_TOKEN="$NOTION_API_KEY" NOTION_KEYRING=0 ntn …`를 쓰며 아래 축약 예와 Workers 호출에도 매번 적용하도록 적었다 |
| 3 Teams 제외 유지 | Graph 세 이름의 JSON 예를 파싱했고 다섯 frontmatter를 변경 전과 바이트 단위로 대조해 동일함을 확인했다. Teams의 `disabled: true`·deferred 설명과 생성기의 제외 목록은 그대로이며, 키 저장·재시작이 pipeline 지원을 만들지 않는다고 명시했다 |
| 4 예시와 실제 실행 근거 대조 | USAGE와 다섯 스킬의 설정 JSON 6개를 `JSON.parse`로 파싱하고 예상 키 이름·문자열 예시를 확인했다. PRD 010의 `production_main_and_spawn_use_startup_values_without_reading_skills`와 `production_exec_uses_process_fallback_and_explicit_winners` 코드 및 완료 기록에 대조했다. 두 시험은 임시 data와 가짜 값으로 존재·일치·재시작·우선순위를 확인하고 `credential-ok`만 출력한다. 이번 문서 작업에서 실제 API 호출이나 그 실행 시험을 반복하지 않았다 |
| 5 browser·Compose·출력 한계 보존 | 기존 Chromium/viewer 빈 환경 두 문단은 변경 전과 같음을 확인했다. Compose `.env`의 용도와 provider·MCP 설정은 유지했다. 다른 exec에도 키가 보임, 일반 출력·모델 입력·transcript·로그의 원문 가능성, 알려진 8자 이상 값과 정해진 표면에 한정된 가리기를 설명했다 |
| 6 검증 범위 | `.md`만 수정했지만 본문을 `include_bytes!`로 포함하고 기존 시험이 원본을 읽으므로, 이번 사용자 지시에 따라 아래 Cargo 시험을 실행했다. 문구 검색만 하는 Rust 시험은 추가하지 않았다 |

내장 자료 생성과 기준값:

- 실제 생성기는 `crates/shacs-skills/scripts/generate_builtins.py`다. `--check`가 exit 0으로 통과했다. 파일 목록·실행 비트·deferred 상태가 같고 본문은 `include_bytes!`로 읽으므로 `builtins_generated.rs` 재생성은 필요 없었다.
- `spec030_skill_registry_baseline.rs`는 고정 본문 hash를 비교하지 않고 registry 선택과 비어 있지 않은 hash를 검사한다. 기존 시험을 그대로 통과했으며 기준값 갱신은 필요 없었다.

이번에 종료까지 확인한 명령:

| 명령 | 결과 |
|---|---|
| `python3 crates/shacs-skills/scripts/generate_builtins.py --check` | exit 0, 생성 자료 일치 |
| `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-skills -q` | 단위 24 + 통합 2 passed, exit 0. `credential_declarations_read_five_builtin_sources`와 baseline·materialize·deferred 시험 포함 |
| `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-core --lib -q runtime::context::tests::` | 6 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli --lib -q skills_list_and_show_use_virtual_bundled_registry` | 1 passed, exit 0 |
| `cargo test --manifest-path crates/Cargo.toml --locked -p shacs-cli --test it -q spec054_skill_readiness::` | 1 passed, exit 0. 실제 CLI 조회와 context의 원본·값 없는 상태 일치 확인 |

합계 34개 시험이 통과했다. Rust 소스 변경이 없어 Cargo fmt·clippy는 생략했다. 전체 workspace 시험, build/clean, Docker 실행·배포·image rebuild, 실제 비밀 값·외부 서비스 API, 커밋은 실행하지 않았다. Docker 경로와 재시작 안내는 저장소 Compose·운영 문서에 대조한 결과이며 실제 Docker 재현 증거가 아니다.

CLI notice 정정: PRD 000이 남긴 `crates/shacs-cli/src/skill_readiness.rs:43`의 `Automatic exec forwarding is not implemented yet.`는 PRD 010 구현 뒤 사실과 달라 `Values of keys declared by active skills are passed to exec commands.`로 바꿨다(이 스펙의 미커밋 변경 안의 문자열이다). 문구를 고정하는 시험은 없고, 아래 shacs-cli 확인을 다시 실행했다.

추가 사용자 결정은 필요 없으며 독립 문서 리뷰를 기다린다.
