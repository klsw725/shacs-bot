# PRD 020. file tool의 workspace 밖 대상

Status: Planned

Depends on: [PRD 000](000-continue-after-refusal.md)의 닫힌 결과(거절 사건과 그 종류, 시작 하나 단위의 셈, 고칠 수 있는 실패와 안내), [PRD 010](010-command-outside-workspace.md)의 닫힌 결과(`Tool::outside_workspace_targets`, 판정과 process gate가 함께 쓰는 `OutsideAccess`, auto mode 표 14·15번 제외, auto가 아닌 mode의 묻기 줄, 비대화형 run의 거절, 자동화 출처). 004·010의 file tool과 경로 검사(`PathContext`, `resolve_path`, `resolve_creatable_path`), 022의 기억된 규칙 matcher, 048의 비밀 파일·보호 경로·목록에서 숨기기를 소비한다. 뒤 번호 PRD에 의존하지 않는다.

## Goal

`restrictToWorkspace`가 꺼진 run에서 `read_file`·`write_file`·`edit_file`·`list_dir`·`glob`·`grep`·`notebook_edit`가 허용 범위(workspace, media 폴더, `extra_allowed_dirs`) 밖 경로를 받아도 tool이 거절하지 않는다. 그 호출은 PRD 010과 같은 판정("workspace 밖 접근")을 지난다. 남는 file tool 거절(symlink 구성 요소가 있는 쓰기, learned-method store, 목록·검색이 symlink를 따라가지 않는 것, 장치 파일)은 그대로다.

## 지금 상태

- `crates/shacs-core/src/tools/filesystem.rs:33` `PathContext { workspace, allowed_dir, media_dir, extra_allowed_dirs }`. `:52` `resolve_path`는 경로를 canonicalize한 뒤 `allowed_dir`가 있으면 허용 범위 밖을 `Path … is outside allowed directory …`로 거절하고, learned-method store를 거절한다. `:85` `resolve_creatable_path`는 이미 있는 symlink 구성 요소를 거절하고(`reject_existing_symlink_components`), 있는 부모를 canonicalize해 부모와 대상이 모두 허용 범위 안인지 본다.
- 모든 file tool의 `preflight_refusal`과 실행이 이 두 함수를 부른다(`filesystem.rs:287,344,480,513,590,661,810,858`, `search.rs:85,159,252,317`, `notebook.rs:85,162`). 그래서 `restrictToWorkspace`와 상관없이 밖 경로는 거절되고, auto mode 표 5번(`ToolRefusal`)이 같은 거절을 미리 낸다.
- production 등록(`crates/shacs-cli/src/spec035_cli_media/tool_registry.rs:46`)은 `allowed_dir`를 늘 workspace로 둔다. `exec`만 `bundle.config.tools.restrict_to_workspace`를 받는다(`:117`). 하위 작업(`runtime/subagent.rs:1502`)과 memory(`runtime/memory.rs:528`)도 `PathContext`를 만든다.
- 목록(`ListDirTool::list_dir`, `collect_recursive`)과 검색(`search.rs:482`)은 symlink 항목을 건너뛰고, `ToolCallExecutionContext::hides`(`tools/base.rs:143`)로 비밀 파일과 `read` deny·ask 규칙이 덮는 경로를 뺀다. `hidden_from_scans`(`runtime/auto_mode/permission_environment.rs:120`)는 경로 기준이라 workspace 밖에서도 같다.
- 048의 비밀 파일 판정(`runtime/auto_mode/permission_materials.rs:163-176`, `secret_file.rs:143`)과 보호 경로 쓰기 판정(`:177-180`)은 data 폴더 기준의 경로 비교라 workspace 밖 경로에도 걸린다. 지금은 file tool이 먼저 거절해 이 판정에 닿지 않는다.
- 022 기억된 규칙의 `broader_matcher`(`runtime/permission_remembered.rs:76-93`)는 file tool 경로를 workspace 기준(`WorkspacePath`)으로만 넓힌다. workspace 밖 경로는 넓힌 matcher가 없어 정확한 action으로 기억된다.
- PRD 010의 판정(`runtime/outside_access.rs`, `auto_decision.rs` 표 14·15번 제외, 승인 질문의 밖 대상)은 tool의 `outside_workspace_targets`만 보므로, file tool이 대상을 내면 그대로 쓰인다.
- 기존 시험: `tests/it/spec048_auto_mode/decision_table.rs`의 "read outside the file tool range" 줄(모든 칸 `Refuse`), `tests/tools.rs`의 허용 범위 밖 거절 시험들(`PathContext::workspace`로 만든 tool).

## Scope

1. **판정으로 넘기는 표시:** `PathContext`에 `outside_by_decision: bool`(기본 `false`)을 더한다. `false`면 지금처럼 허용 범위 밖을 거절한다. production 등록은 `!bundle.config.tools.restrict_to_workspace`를 넣는다. 하위 작업, memory, 시험 등 다른 곳은 기본값(거절)을 그대로 쓴다(011 하위 작업은 늘 restrict).
2. **경로 검사:** `outside_by_decision`이 켜진 `PathContext`에서 `resolve_path`와 `resolve_creatable_path`는 허용 범위 밖이라는 이유로만 거절하지 않는다. 이미 있는 symlink 구성 요소가 있는 쓰기, learned-method store, 장치 파일, 부모가 없는 경로, canonicalize 실패의 거절은 그대로다.
3. **workspace 밖 대상:** file tool마다 `outside_workspace_targets`를 구현한다. 경로 인자(`path`, `glob`·`grep`은 없으면 `.`)를 지금 검사와 같은 방식으로 풀어(`raw_candidate_path`, 있으면 canonicalize, 없으면 있는 부모를 canonicalize하고 나머지를 붙인 경로) 요청한 경로(lexical 정규화)나 따라간 경로가 허용 범위 밖이면 대상으로 낸다. 대상 글은 따라간 경로(없으면 정규화한 요청 경로)다. `outside_by_decision`이 꺼져 있으면 빈 목록이다(그 run은 tool이 거절한다). 풀 수 없는 경로는 대상이 아니다(실행이 지금처럼 오류를 낸다).
4. **판정:** 새 판정 줄은 없다. PRD 010의 `OutsideAccess`가 file tool 대상을 그대로 싣는다. 그래서 auto mode는 표 14·15번을 건너 classifier로, auto가 아닌 대화형 mode는 묻기로(이번 승인·allow 규칙·기억된 허용은 그대로), 비대화형 run은 cron이 아닌 run의 `bypass_permissions`만 통과, 나머지는 PRD 000의 `UnattendedOutside` 거절이다.
5. **그대로 걸리는 048 판정:** 비밀 파일 읽기(data 폴더의 인증 저장소, `.env.json`)는 모든 mode에서 매번 묻고 기억 선택지가 없으며 묻지 못하면 거절, 쓰기는 거절이다. 보호 경로(`config.json` 등) 쓰기는 auto mode에서 allow 규칙으로 허락되지 않고 classifier가 본다. 목록·검색은 밖 폴더에서도 비밀 파일과 deny·ask 규칙 경로를 빼고 symlink를 따라가지 않는다.
6. **기억:** workspace 밖 file tool 대상은 지금 matcher대로 정확한 action으로 기억된다. matcher는 바꾸지 않는다.
7. **거절 사건:** `restrictToWorkspace`가 켜진 run의 허용 범위 밖 거절은 PRD 000의 `file_range` 거절 그대로다. 비대화형 거절은 PRD 010의 `UnattendedOutside`(대상은 3번 목록)다.
8. **문서:** `docs/USAGE.md`의 `restrictToWorkspace`·file tool 설명을 이 동작으로 고친다. 앞선 스펙 문서는 고치지 않는다.

## 결정과 이유

- **표시는 `PathContext`에 둔다.** 모든 file tool이 이미 `PathContext` 하나로 범위를 정한다. tool마다 설정을 따로 받으면 한 곳을 빠뜨릴 수 있다. 기본값을 거절로 두어, 표시를 넣지 않은 생성(하위 작업, memory, 시험)은 지금과 같다.
- **허용 범위 밖이라는 이유만 없앤다.** SPEC 소유 범위 2가 남기는 거절(symlink 구성 요소 쓰기, learned-method store, 목록·검색의 symlink)은 경로 검사 안에서 따로 걸리므로 그 부분은 손대지 않는다.
- **대상은 따라간 경로로 적는다.** symlink를 따라가면 밖인 경로는 요청 글만 보면 안쪽처럼 보인다. 사용자가 실제로 닿는 곳을 질문과 classifier에 보이기 위해 따라간 경로를 쓴다.
- **판정 코드는 PRD 010 것을 쓴다.** SPEC 소유 범위 3은 명령과 file tool에 같은 순서를 요구한다. 대상만 내면 같은 자리에서 판정되고 process gate·질문·셈도 같아진다.

## Non Scope

- cron 작업의 목록과 승인 기록(PRD 030).
- 폴더 단위 기억, matcher 형식 변경.
- `exec`의 `extra_allowed_dirs`.
- MCP·플러그인 tool의 경로(그 tool이 `outside_workspace_targets`를 구현하지 않으면 빈 목록이다).

## Parent Requirement Mapping

- 053 소유 범위 1·2·3 중 file tool, 6 중 자기 변경과 그 문서.
- 독립 종료 조건: 수용 조건 3의 나머지(file tool), 4.7 중 file tool, 6.
- 앞선 스펙에서 바꾸는 것: 004·010의 file tool 허용 범위 거절(restrict가 꺼진 run), 048 PRD 000 판정 표 5번의 "file tool의 범위 밖 대상" 부분. 앞선 스펙 문서는 고치지 않는다.

## Acceptance Criteria

1. 경로 검사: `outside_by_decision`이 켜진 file tool은 허용 범위 밖 읽기·쓰기·편집·목록·검색을 `preflight_refusal`과 실행 모두에서 거절하지 않는다. 이미 있는 symlink 구성 요소가 있는 쓰기와 learned-method store는 그대로 거절되고, 목록·검색은 밖 폴더에서도 symlink를 따라가지 않는다. 꺼진 tool(기본값)은 지금처럼 거절한다.
2. 대상: 밖 절대 경로, `../`로 나가는 경로, workspace 안에서 밖을 가리키는 symlink 읽기가 대상을 내고, workspace·media 안 경로는 내지 않는다. 표시가 꺼져 있으면 빈 목록이다.
3. auto mode(048 판정 표에 restrict 꺼진 "read outside the file tool range" 줄을 더한다): 밖 읽기·쓰기가 classifier로 가고 허용이면 실행, 막음·판정 없음이면 실행하지 않는다. restrict 켬 줄은 지금처럼 거절한다.
4. 다른 대화형 mode: `default`의 밖 읽기와 `accept_edits`의 밖 쓰기를 묻고 질문에 밖 대상이 적힌다. 승인한 그 호출만 실행된다. `bypass_permissions`는 실행한다.
5. 비밀 파일과 보호 경로: data 폴더의 `.env.json`을 `read_file`로 읽으면 auto와 `default`에서 묻고 기억 선택지가 없다. `write_file`로 쓰면 거절된다. data 폴더를 `list_dir`로 보면 `.env.json`과 인증 저장소가 나오지 않는다.
6. 비대화형 run: production worker의 cron run에서 밖 `read_file`은 `bypass_permissions`에서도 PRD 000 안내와 함께 거절되고, heartbeat run은 `bypass_permissions`에서 실행된다.
7. fmt(Google 옵션), workspace clippy `-D warnings`, `shacs-core`·`shacs-cli` 패키지 테스트가 통과한다.

## 구현 메모 및 검증 근거
