# Spec035 구현 및 closure 차단 기록

Status: Open

Closure verdict: BLOCKED

이 문서는 최종 완료 선언이 아니다. 2026-09-17 최종 인계는 기존 F2/F3/F4와 F1-R01/R01-A01 제한적 재검토, [최종 compiled QA](../../../.omo/evidence/spec035/closure/final-compiled-qa.json), 보존된 todo10·11 및 runner 원본과 남은 차단을 구분한다. 이번 작업에서 Rust 소스와 실제 사용자 설정은 수정하지 않았으며 이전 증거의 hash 목록도 갱신하지 않았다. 새 격리 config/auth/HOME/workspace에서 수행한 smoke와 문서 정정만으로 release를 통과시키지 않는다.

구현된 범위는 코드 경로의 존재를 뜻하며 모든 invariant의 충족이나 결함 부재를 뜻하지 않는다. 원래 네 F2 결함은 scoped confirmed이며, runner 실행 증거 수용 경로와 lexical/canonical alias 보정도 제한적 재검토에서 확인됐다. Encoded WebSocket query는 최종 바이너리에서 snapshot generation 1·2를 확인했다. 이 구현 보정과 smoke는 전체 F1 승인 또는 실제 전체 release 증거의 생산이 아니다.

## 구현된 범위

| 범위 | 읽은 구현 위치 | 확인한 경계 |
|---|---|---|
| 공통·revised projection | `crates/shacs-projection/src/spec035/`, CLI/API/channel의 `spec035_revised` adapter | Owner fact의 조회 표현이며 durable approval, ephemeral confirmation, hook denial 및 runtime/disclosure를 혼동하지 않는다. |
| Tasks source | [spec035_tasks_source.rs](../../../crates/shacs-core/src/runtime/spec035_tasks_source.rs), [action.rs](../../../crates/shacs-core/src/runtime/spec035_tasks_source/action.rs) | Goal/child/workflow/automation/app/recovery를 집계한다. 별도 DB가 없으며 현재 행·locator·광고된 action을 검증한다. |
| CLI/API/TUI | [Tasks CLI](../../../crates/shacs-cli/src/spec035_tasks_cli.rs), [Tasks API](../../../crates/shacs-api/src/spec035_tasks.rs), [TUI live source](../../../crates/shacs-tui/src/live_source.rs) | `tasks --json`, `GET /v1/tasks`, TUI가 같은 projection을 소비한다. API query는 URL 인코딩된 `session_id` 하나만 받는다. |
| TUI media | [TUI live source](../../../crates/shacs-tui/src/live_source.rs), [media store](../../../crates/shacs-core/src/runtime/video_analyzer_spec035/store.rs) | Config parent data directory의 `media/projections/current.json`을 읽는다. Session metadata의 `media_capability`가 live 원천이 아니며 부재·읽기·검증 실패는 unavailable이다. |
| Capability 협상 | [API transport](../../../crates/shacs-api/src/spec035_transport.rs), [hello 모델](../../../crates/shacs-projection/src/spec035/transport_hello.rs) | Mutation은 hello를 요구하며 지원 검사 후 owner로 전달한다. Capability는 인증·permission·approval이 아니다. |
| Reconnect/accounting | [reconnect](../../../crates/shacs-api/src/spec035_reconnect.rs), [accounting](../../../crates/shacs-api/src/spec035_accounting.rs) | Snapshot 먼저, 이후 같은 generation의 delta를 검사한다. Progress/final 관측과 누락 counter를 구분한다. |
| Release evidence | [runner CLI](../../../crates/shacs-projection/src/bin/spec031-release-runner.rs), [current execution admission](../../../crates/shacs-projection/src/spec031/release_runner/spec035_execution.rs), [카탈로그](../../../crates/shacs-projection/src/spec031/release_runner/spec035_catalog.rs) | 기존 Spec031 66행과 별도 Spec035 80행을 보존한다. 별도 current execution schema는 source/command/owner/gate/cleanup/incident receipt를 검증한다. 실제 canonical classification v2의 구조·inventory 검증은 실행 receipt가 아니므로 required command 전에 차단한다. |
| Wizard owner 입력 | [IO loop](../../../crates/shacs-cli/src/onboard_wizard/io_loop.rs), [owner mapping](../../../crates/shacs-cli/src/onboard_wizard/owner_facts.rs) | 명시적 config/profile 선언과 실제 local Spec030 credential/disclosure 관측을 소비한다. Runtime credential 해석은 실행하지 않아 미관측 상태는 unavailable이며 선언을 credential 존재·readiness·권한으로 승격하지 않는다. |

사용자 요청 문법과 오류 조건은 [Tasks 사용법](../../USAGE.md#tasks-조회와-owner-변경-요청)을 따른다. CLI는 `--json`을 요구하고, 변경은 `--owner`, `--action`, `--locator`, `--transport-hello`를 받는다. API `POST /v1/tasks/actions`도 매 요청의 `transport_hello`와 별도 loopback mutation opt-in이 필요하다. Goal pause/resume, app stop/recover, runtime recover 외 Tasks mutation은 지원하지 않는다.

## 보존된 검증 기록

아래 수치는 해당 실행 당시 기록이지 이 문서 작업에서 테스트를 다시 실행한 결과가 아니다. 로컬 `.omo` 증거는 checkout마다 없을 수 있으며 누락되면 문서만으로 대체하지 않는다.

| 기록 | 관측된 결과 | 해석 제한 |
|---|---|---|
| [Task3 parity](../../../.omo/evidence/spec035/prd000-006/task-3-parity.json) | Revised owner projection/adapter 검사 기록 | 현재 release seal이 아니다. |
| [Task9 accounting](../../../.omo/evidence/spec035/prd008/task-9-accounting.json) | Queue/slow-consumer/progress/final 관측 기록 | Remote ACK나 durable network delivery 증거가 아니다. |
| [역사적 Todo10 manifest](../../../.omo/evidence/spec035/prd000-009/historical-task10-manifest.json) | 당시 40행 PASS, focused/exact 133개 및 correction 14개 통과 기록 | F1에서 전체 요구사항 매핑의 불완전성이 확인되어 현재 closure 효력은 철회했다. 과거 실행 결과만 보존한다. |
| [현재 closure 분류](../../../.omo/evidence/spec035/prd000-009/manifest.json) | 실제 종료 조건 45행, PASS 0 / BLOCKED 45, 누락 0 | classification v2는 요구사항 분류이며 새로운 실행 증거 또는 release seal이 아니다. |
| [PRD008-009 보정 mapping](../../../.omo/evidence/spec035/prd000-009/closure-mapping-corrected.json) | Capability/ordering/실제 표면/owner audit 및 goal/locator/mixed Tasks/store audit를 구분 | 구현·과거 검증 증거이며 독립 차단 gate를 대체하지 않는다. |
| [외부 owner 감사](../../../.omo/evidence/spec035/prd000-009/external-owner-audits.json) | Specs029-034 정확한 fact 검사 7개 PASS 기록 | 외부 스펙 전체 완료나 현재 Spec035 release PASS가 아니다. |
| [Task11 runner](../../../.omo/evidence/spec035/closure/task-11-release-runner.json) | Focused 92개, fmt/clippy/build 통과 기록; success-fixture exit 0; current-worktree exit 1 | 당시 machine/human `BLOCKED`, coverage 66행 중 PASS 5/BLOCKED 61은 역사적 결과다. |
| [카탈로그 보정](../../../.omo/evidence/spec035/closure/f1-runner-remediation.json), [보존 library 실행](../../../.omo/evidence/spec035/closure/f1-runner-current/summary.md) | Spec031 66 + Spec035 80 = 146행, PASS 5/BLOCKED 141, Spec035 PASS 0/BLOCKED 80, commands 0 | Spec035 parent 35행과 종료 조건 45행의 열거를 확인한 것이며 전체 요구사항의 실행 증거 통과가 아니다. |
| [과거 F3 compiled current-worktree 원본](../../../.omo/evidence/spec035/closure/f3-runner-current/summary.md), [fixture 원본](../../../.omo/evidence/spec035/closure/f3-runner-fixture/summary.md) | Current는 동일한 146행·PASS 5/BLOCKED 141·commands 0·exit 1, fixture는 commands 17·exit 0 | 보정 전 실제 생성 원본을 그대로 보존한다. 현재 바이너리의 증거로 재결속하지 않는다. |
| [최종 compiled current-worktree 원본](../../../.omo/evidence/spec035/closure/final-compiled-runner-current/summary.md), [fixture 원본](../../../.omo/evidence/spec035/closure/final-compiled-runner-fixture/summary.md) | Current는 146행·PASS 5/BLOCKED 141, Spec035 80행 BLOCKED, commands 0·exit 1; fixture는 commands 17·exit 0 | Clean 후 locked build한 세 바이너리와 실행 당시 source inventory를 별도로 hash 결속했다. Fixture PASS는 runner mechanics뿐이며 workspace 또는 release PASS가 아니다. |
| [Wizard 보정](../../../.omo/evidence/spec035/closure/f1-wizard-owner-remediation.json) | Typed config/profile 선언과 local owner 입력, focused 13개 및 production IO-loop harness 3개 통과 기록 | Resolved/denied mapper fixture를 live credential 해석·승인 QA로 해석하지 않는다. 최신 F3의 wizard 취소 QA도 전체 PRD005 closure를 뜻하지 않는다. |

## 독립 차단 항목

| 항목 | 상태 | 근거와 필요한 확인 |
|---|---|---|
| Todo10 소스 결속 | `BLOCKED` | Todo11이 `crates/shacs-projection/src/bin/spec031-release-runner.rs` 등을 수정했다. 해당 파일의 todo10 SHA-256은 `3d8749c3a1c0b2400b8c8dd3e228d8e953f1516ba05c33fe4532abd845b7ec81`, task11 기록은 `ae139de293096848fbbc9bb2def9dc83a2a7b424e6e82586eb451a69e999da85`다. 이후 F2 보정도 과거 결속으로 검증되지 않는다. 과거 목록을 덮어쓰지 말고 현재 소스로 별도 재검증해야 한다. |
| Spec034 fixture 누락 | `BLOCKED` | [Workspace triage](../../../.omo/evidence/spec035/prd000-009/workspace-failure-triage.json)는 `.omo/evidence/spec034/task-12-integration.json` 누락과 `runtime::spec034_release::runner::fixture::tests::mismatch_uses_isolated_tree` 실패를 기록한다. 713개 통과 결과 뒤 1개 실패이며 격리 재실행도 실패했다. 임의 fixture로 대체하지 않았다. |
| 소유권 불명 임시 경로 | `BLOCKED` | [Cleanup registry](../../../.omo/evidence/spec035/prd000-009/cleanup-registry.json)에 `.shacs-spec034-tools-*` 288개 경로가 사전 inventory 없이 관측되었다고 기록되어 있다. Todo10 생성물인지 안전하게 판별할 수 없어 삭제하지 않았다. 이 숫자는 당시 기록이며 문서 작업에서 경로를 재탐색하지 않았다. |
| 이전 기본 설정 재작성 | `UNRESOLVED` | [Incident registry](../../../.omo/evidence/spec035/prd000-009/incident-registry.json)는 이전 default-path onboard의 `~/.shacs-bot/config.json` 재작성을 기록한다. 안전한 사전 상태가 없어 복구 완료를 주장할 수 없다. 문서 작업에서 해당 파일을 읽거나 쓰지 않았고 복원을 시도하지 않았다. |

Task11 자체 QA 경로·process cleanup 통과는 소유권 불명 288개 경로의 cleanup 통과와 다르다. Dirty worktree라는 관측과 source hash 불일치도 별개다. 문서 갱신은 source binding을 새로 만들거나 기존 manifest의 verdict를 변경하지 않는다.

Task12의 최초 읽기와 병행된 F1 증거 보정은 canonical manifest를 classification v2로 교체하고 이전 manifest를 역사 자료로 보존했다. 따라서 Task11에 기록된 이전 manifest digest를 현재 manifest의 결속으로 사용할 수 없다. [F1 교정 보고서](../../../.omo/evidence/spec035/closure/f1-evidence-correction.json)는 변경 provenance와 요구사항별 차단을 기록한다. 과거 transcript를 현재 소스에 재결속하거나 과거 테스트를 재실행한 것으로 취급하지 않는다.

## 독립 감사와 미완료 검증

| 기록 | 보존된 판정 | 남은 경계 |
|---|---|---|
| [역사적 F1 요구사항 감사](../../../.omo/evidence/spec035/closure/f1-plan-audit.json), [F4 내 F1 재감사](../../../.omo/evidence/spec035/closure/f4-scope-review.json) | 전체 F1 승인 없음 | 카탈로그·최신 runner 원본 부재와 F1-R01 수용 구현 미완료는 현재 구현 차단 사유가 아니다. Baseline 전체 증거와 정확한 owner fact의 producer/command/artifact/adapter 증거는 여전히 미완료다. |
| [F1-R01/R01-A01 재검토](../../../.omo/evidence/spec035/closure/final-r01-recheck.json) | `confirmed`, implementation remediation만 확인 | 31개 실행 검사가 통과한 제한적 기존 보고서다. Constructed receipt의 validator mechanics를 확인한 것이며 모든 owner/gate가 실제 실행됐다는 증거나 전체 F1 승인이 아니다. 이번 compiled QA에서 그 테스트를 다시 실행했다고 주장하지 않는다. |
| [F2 코드 검토](../../../.omo/evidence/spec035/closure/f2-code-review.json) | `confirmed` | 원래 네 결함인 실패 owner transcript 수용, reconnect identity 충돌, 이전 generation의 final 관측 덮어쓰기, durable child 누락의 빈 집합 변환 및 stale 비동기 queue 역방향 사례의 보정을 독립 재검토했다. 이 범위의 잔여 finding은 없으며 병행 wizard/runner 변경과 전체 release는 승인하지 않았다. |
| [과거 F3 실제 표면 QA](../../../.omo/evidence/spec035/closure/f3-manual-qa.json) | 역사적 `FAIL_DOCUMENTED_WEBSOCKET_RECONNECT_URL` | 당시 encoded query HTTP 400과 [이전 binary 부재 시도](../../../.omo/evidence/spec035/closure/f3-raw/prior-blocked-manual-qa.json)는 보존한다. 최신 바이너리의 실패로 재해석하지 않는다. |
| [최종 compiled QA](../../../.omo/evidence/spec035/closure/final-compiled-qa.json), [cleanup](../../../.omo/evidence/spec035/closure/final-compiled-cleanup.json) | 제한된 compiled QA 통과, release `BLOCKED` | 문서의 encoded URL에서 snapshot generation 1·2, CLI/API parity와 거절 시 owner 불변, TUI pause/resume/refresh/quit, REPL status/EOF와 wizard 취소를 확인했다. TUI pixel visual·모든 owner·전체 suite·독립 release 승인은 평가하지 않았다. 이번 소유 자원 cleanup은 과거 288개 경로를 해결하지 않는다. |
| [F4 범위 검토](../../../.omo/evidence/spec035/closure/f4-scope-review.json) | 보존된 `fail`, `finalApproval=false` | 이후 제한적 구현 재검토·compiled QA·문서 정정은 별도 기록이다. F4 원본을 덮어쓰거나 전체 최종 승인으로 승격하지 않으며 후속 독립 승인 gate는 남아 있다. |

외부 owner registry의 여섯 PASS entry와 일곱 선택 검사 통과는 보존된 사실이다. 하지만 예를 들어 `spec031_readiness_parity_uses_runtime_inspect_owner_source_for_api_cli_and_bundle` 하나로 config/auth-source/execution snapshot 전체를 검증했다고 할 수 없다. [PRD007 검사 범위](prds/007-release-runner-and-spec035-closure.md#외부-증거-위치와-유효-범위)를 따른다. 여섯 owner 종류의 mixed Tasks fixture 역시 여섯 live owner의 최종 QA가 아니다.

Canonical 45행 분류와 owner classification의 일부 wizard/child/catalog 설명은 보정 전 시점의 관측이다. Hash로 결속된 원본은 수정하지 않고 위 구현 보정·최종 compiled QA를 현재 상태 설명으로 함께 읽는다. 최종 current-worktree는 classification 검증 후 실행 receipt가 아니라는 이유로 required command 전에 차단됐으므로, 생성 owner audit의 command 부재를 외부 스펙 자체의 새 실패로 해석하지 않는다. Canonical 45개 종료 조건과 6개 owner는 새 전체 의미 증거가 없으므로 모두 `BLOCKED`를 유지한다.

## 보장하지 않는 것

- Tasks projection, owner locator, resource digest, capability 지원은 별도 mutation authority나 인증·권한 부여가 아니다.
- Snapshot-first ordering과 process-local bounded accounting은 durable replay, 끊긴 구간의 복원, 무손실 전달, remote ACK/read receipt, exactly-once를 보장하지 않는다.
- Accepted/emitted/coalesced/dropped와 final delivery는 독립적인 관측이다. 누락을 0으로 만들지 않고 `final_delivered`를 사용자 수신으로 해석하지 않는다. SSE는 pending/unknown일 수 있다.
- Projection 경계의 redaction은 session/log/tool output 전체의 완전한 redaction이나 universal sandbox/process containment가 아니다.
- Success-fixture는 runner 동작 검증이다. 과거 QA, 문서 감사, 외부 owner의 scoped 완료 어느 것도 현재 source-bound 최종 release 검증을 대신하지 않는다.

## 문서 검증과 후속 gate

최초 [Task12 문서 증거](../../../.omo/evidence/spec035/closure/task-12-docs.md)와 [이전 문서 정정](../../../.omo/evidence/spec035/closure/final-docs-correction.md)은 그대로 보존한다. 이번 [최종 인계](../../../.omo/evidence/spec035/closure/final-handoff.md)는 clean/locked build, focused fmt, workspace clippy, 실제 격리 표면 QA와 cleanup을 새 원본으로 기록한다. 알려진 실패가 있는 workspace suite는 재실행하지 않았다. QA 전후 source inventory는 같고, 이후 이 문서 등 네 문서만 변경한 delta를 별도 기록한다. 이는 문서까지 포함한 최종 committed release seal이 아니다. 실제 사용자 설정·인증과 소유권 불명 경로에는 접근하지 않았다.

후속 독립 감사와 현재 소스에 결속된 전체 필수 gate 검증이 남아 있다. 위 차단이 남거나 새 증거가 누락되면 Spec035는 `Open`, closure는 `BLOCKED`를 유지한다.
