# PRD 000. 요청별 수행 방식 선택과 사용자 변경

Status: Complete (Scoped)

## Goal

한 요청에서 일반 수행 또는 지원 가능한 작업 방식을 선택하고 이유·가용 상태를 보여주며, 사용자가 시작 전이나 진행 중 바꿀 수 있게 한다. **계산·보고서·이미지의 전문 검증 구현 없이도 선택/변경 결과만으로 완료한다.**

## Scope

1. 004 tool, 005 skill, 024 recipe/plan, 030 현재 실행 gate의 가용 상태를 읽고 사용자 요청·명시적 선택에 따라 기본 또는 전문 방식을 결정한다. 단순 요청에는 무거운 workflow를 강제하지 않는다.
2. 선택한 방식/이유, 필요한 도구의 missing/unsupported 상태, 사용자 override와 변경 전후의 목표·확인된 근거·미완료 항목을 같은 요청의 기록으로 보존한다.
3. 진행 중 변경은 기존 024 cancellation/resume 및 030 permission 경계를 소비한다. 이미 실행한 외부 효과를 자동으로 되돌리거나 이전 방식의 검증 PASS를 새 방식에 복사하지 않는다.

## Non Scope

- 전문 분야별 수치·출처·이미지 품질 검증, 복합 요청 결과 통합, 새로운 workflow engine/recipe registry.
- 036의 장기 관찰이나 038의 방법 학습 완료. 이 PRD는 현재 한 요청만으로 종료한다.

## Parent Requirement Mapping

- [037 SPEC](../SPEC.md)의 소유 범위 1, 수용 조건 1의 일반/전문 선택·사용자 지정/진행 중 변경을 소유한다.
- 선택 및 재선택 때 기존 근거 보존·외부 효과/권한 비승계 불변식을 소유한다.

## Acceptance Criteria

1. 실제 CLI 또는 대화 표면에서 일반 요청은 일반 방식으로, 지원 가능한 전문 요청은 선택 방식·이유·확인 항목으로 표시된다. 도구 없는 요청은 unavailable/unsupported로 표시되며 조용히 성공하지 않는다.
2. 같은 요청 중 사용자가 방식을 지정·변경하면 현재 목표·확인된 산출물·남은 질문은 유지되고 이전 방식의 미검증 결과가 완료로 표시되지 않는다.
3. 변경 직전 외부 효과가 있었던 경우 취소/재실행 상태를 보여주고, 사용자 승인이나 실행 gate가 자동 승계되지 않음을 확인한다. 구버전 선택 사실은 수정 후에도 inspect할 수 있다.

## Closure Evidence

- 일반·전문·미지원·사용자 변경의 실제 표면 transcript와 선택 기록.
- 024 plan/recipe 및 030 gate 재사용, 외부 효과 비소급·검증 비승계의 회귀 테스트.

PRD 000은 **선택과 사용자 변경**만으로 닫는다. 뒤 번호 PRD의 작업별 검증이나 복합 출력이 없어도 자체 결과가 관측 가능해야 한다.

## 구현 메모 및 검증 근거 (2026-10-21)

아래는 구현·실행 기록이다. 완료 판정은 마지막 종료 확인 절에 기록한다.

- `shacs-command`의 non-priority builtin `/method`가 status/list/use/auto/history 및 canonical ID/별칭을 처리한다. Plugin command 충돌은 기존 builtin 규칙을 따른다.
- `task_method*` 모듈이 deterministic 분류, 실제 `ToolRegistry` 기반 가용성, typed selection revision·validation claim 확장점, redacted bounded session metadata `task_method_requests_v1`를 소유한다. 자동 general은 provider context/footer를 추가하지 않는다. 전문 방식의 확인 항목은 항상 `not verified`이며 PRD000은 validation claim을 생성하지 않는다.
- `agent_loop_method*` 모듈이 기존 `SessionMutationGuard` 안에서 변경·재개·취소를 처리한다. 질문/승인/retry/action/workflow의 보류 상태는 취소하며, 이전 selection과 관측한 외부 효과는 보존한다. 보류 workflow 취소는 기존 `WORKFLOW_FAILED` event와 `WorkflowRunState::Cancelled`, `method_changed` 사유로 기록한다. Marker 삭제만으로 취소 성공을 합성하지 않는다.
- 024 admission scoring은 바꾸지 않는다. Method ID·revision·request ID와 제한된 context를 plan constraints에 넣어 기존 plan digest에 결속한다. Workflow child와 일반 실행의 관측은 기존 hook/tool-event 경계를 소비하는 수동적 collector로 모으며, 승인 재개 실행은 기존 execution report를 소비한다. Session/project remembered permission 정책은 유지하고 새 행동은 정상 gate로 다시 평가한다.
- Metadata 보존 한도와 한도 도달 시 동작, 변경 후 continuation 및 도구 활성화 뒤 재평가 절차는 [사용법](../../../USAGE.md#요청별-수행-방식과-method)에 명시했다. `/method auto`는 보존된 원문으로 재분류하고, 원문이 compaction으로 사라진 경우 원래 저장한 자동 분류를 사용한다.
- 첫 구현 시점에 새 command 테스트 3개, AgentLoop 시나리오 17개, classifier 테스트 4개, durable workflow 취소 테스트 1개, compiled `agent` REPL 테스트 3개를 추가했다. 최종 수는 아래 전체 workspace 항목에 적었다. REPL 테스트는 2.50초에 통과하여 ignored로 두지 않았다. 수동 QA에서 발견한 승인 후 process gate 거부의 `failed`/`denied` 오분류는 실패 재현 테스트 후 수정했다. 기존 테스트의 assertions는 변경하지 않았다.
- 리뷰 후 보정: `/stop`과 `/new`가 방식 변경 경로를 타서 보류 질문·승인을 `method_changed`로 취소하던 문제를 고쳤다. `/stop`은 보류 질문·승인·workflow를 건드리지 않고 보류 항목이 없는 활성 요청만 `cancelled`로 닫는다. `/new`는 방식 변경 처리 없이 활성 요청을 `cancelled`로 닫는다. 이를 고정하는 AgentLoop 시나리오 3개를 추가했다. Composed 안내의 내부 PRD 번호와 도구 활성화 안내 문구도 사용자 문구로 고쳤다. 방식 변경 응답은 보류 항목이 없어도 "모두 취소했다"고 표시하던 고정 문구 대신 실제로 취소한 질문·승인·workflow·retry 승인만 표시하며, 복합 요청은 `unsupported, unavailable`로 표시한다.
- 사용자 요청으로 로컬 `ask`/`agent`/`run`이 side-effect tool을 기본으로 등록하도록 바꿨다. `--no-side-effects`로 끌 수 있고 `--allow-side-effects`는 호환용으로 남긴다. `run --allow-remote`는 `--allow-side-effects` 없이는 끈 상태를 유지하며, `serve --allow-api-side-effects`와 channel `runtime start`의 opt-in은 바꾸지 않았다. Loopback `run`의 기본값은 allowlist에 등록한 channel sender turn에도 적용된다(allowlist 기본값은 deny-all). 등록된 tool의 실행 승인은 계속 permission mode와 process gate가 결정한다. 이 환경의 native host에서는 승인 후에도 기존 Spec030 process gate가 `exec` 실행을 `Denied`로 막으며, Spec037 이전 baseline binary에서도 같은 결과를 재현했다. 도구 부족 차단 REPL 시나리오는 `--no-side-effects`로 재현하고, 기본 REPL에서는 `exec`가 등록되어 계산 방식이 available로 선택됨을 확인한다. Parser 테스트 3개를 추가했다.
- 독립 Oracle gate 1차 판정(BLOCKED) 후 보정했다.
  - system·cron·automation 등 사용자 아닌 turn의 `/method use`·`/method auto`는 상태를 바꾸지 않고 거절한다.
  - Workflow plan 결속 constraint에 request ID를 넣었다. 사용자 아닌 turn이 결속된 보류 workflow를 재개해 끝내면 연결된 요청도 workflow 결과로 닫는다. 이전에는 요청이 `in_progress`로 남아 다음 사용자 메시지가 새 요청을 만들지 못했다.
  - 같은 계열의 경로도 보정했다. 사용자 아닌 turn이 진행 중 요청의 보류 질문 응답으로 처리되어 실행을 이어가면, 관측·완료 처리를 그 요청에 적용한다. 그 내용은 사용자 결정으로 기록하지 않는다.
  - 세 결함 모두 수정 전 실패를 재현한 회귀 테스트로 고정했다. AgentLoop 시나리오 2개와 crate 내부 workflow 테스트 1개다.
- Oracle gate 2차 판정(BLOCKED) 후 보정했다.
  - 실제 자동화 알림·검증 메시지는 사용자 channel(예: email)과 `automation`/`automation-verifier` sender로 들어오고 `automation_route` metadata를 가진다. 이를 사용자 turn으로 오판해 one-shot 지정을 소비하거나, 가용성으로 차단되거나, 보류 질문 응답을 사용자 결정으로 기록할 수 있었다.
  - `automation_route`가 있는 메시지를 사용자 아닌 turn으로 분류한다.
  - 실제 production 형태의 AgentLoop 시나리오 2개로 고정했고, 수정 전 실패를 재현했다.
  - 사용자 아닌 turn 관련 시나리오는 `spec037_method_selection/turns.rs`로 모았다.
- 실행 로그·tmux transcript·전체 workspace baseline 비교 및 잔여 제한은 [PRD000 실행 보고서](../../../../.omo/evidence/spec037/prd000/REPORT.md)에 모은다. 로컬 `.omo` evidence가 없는 checkout에서는 이 링크를 실행 증명으로 간주하지 않는다.
- 첫 구현 후 전체 workspace를 `--locked --workspace --no-fail-fast`로 실행한 결과는 **3,147 passed / 1 failed / 13 ignored**였다. 위 보정, side-effect 기본값 변경, Oracle 1·2차 판정 후 보정을 모두 반영한 최종 코드로 다시 실행한 결과는 **3,158 passed / 1 failed / 13 ignored**, 1,288초다. Baseline의 **3,119 / 1 / 13** 대비 새 테스트 39개가 통과했다. 내역은 command 3, classifier 4, crate 내부 workflow 2, AgentLoop 시나리오 24, compiled REPL 3, CLI parser 3이다. 유일한 실패는 같은 Spec034 `mismatch_uses_isolated_tree`의 로컬 fixture 누락이며 추가 회귀는 없었다. Workspace 전체 all-green이나 부모 gate 승인으로 표현하지 않는다.

PRD010–040의 계산 독립 확인, 실제 출처 결속, 이미지 요구 검토, 복합 결과 성공 판정은 구현하지 않았다. 복합 전문 의도는 `composed`/`unsupported`로 차단하고 단일 방식 선택을 안내한다.

## 종료 확인 (2026-10-22)

- 범위: 한 요청의 방식 선택·이유·가용 상태 표시, 시작 전 1회 지정과 진행 중 변경, 변경 전후 목표·산출물·남은 질문·외부 효과 보존, 승인·검증 비승계. 계산·보고서·이미지의 전문 확인과 복합 결과는 포함하지 않는다.
- 최종 검증: 전체 workspace `cargo test --manifest-path crates/Cargo.toml --locked --workspace --no-fail-fast` 결과는 **3,158 passed / 1 failed / 13 ignored**다. 유일한 실패는 baseline과 같은 Spec034 fixture 누락 테스트다. `cargo fmt --all -- --check`와 workspace clippy `-D warnings`도 통과했다.
- 실제 표면: 격리된 config·workspace에서 compiled `agent` REPL로 다음을 확인했다.
  - 일반·계산·보고서·이미지·복합 선택.
  - 도구 부족 차단 후 `/method use general`로 이어가기.
  - 질문·승인 대기 중 변경과 `/stop`.
  - 재시작 후 `/method history`.
- Oracle 독립 검증: **PASS** (3차).
  - 1차 BLOCKED: 사용자 아닌 turn의 `/method` 변경, 결속 workflow 종료 시 요청 미종료, 증거 수치 불일치. 보정했다.
  - 2차 BLOCKED: 사용자 channel로 들어오는 자동화 메시지를 사용자 turn으로 오판했다. 보정했다.
  - 3차 PASS. 이 PASS는 PRD 010–040의 작업별 확인이나 복합 결과를 포함하지 않는다.
- 남은 제한: 이 환경의 native host에서는 기존 Spec030 process gate가 승인 뒤에도 `exec` 실행을 막는다. Spec037 이전 baseline에서도 같은 결과를 재현했다. 계산 방식의 선택과 표시는 동작하지만, 실제 명령 실행과 결과 확인은 PRD 010의 범위다.
- 집계 주석(2026-10-22, PRD010 작업 중 확인): 위 테스트 수는 `test result` 줄의 단순 합계다.
  - 이 합계에는 일부 테스트가 띄운 하위 test process의 결과 줄 8개(각 1개, `filtered out`이 있는 줄)가 baseline을 포함한 모든 실행에 똑같이 들어 있다.
  - `filtered out`이 0인 suite만 합산하면 baseline은 3,111, PRD000 종료 시점은 3,150이다. 새 테스트 39개 통과와 추가 실패 없음이라는 비교 결론은 같다.
