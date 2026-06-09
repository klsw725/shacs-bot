# zero-setup sandbox execution 아키텍처 명세

Status: Active. 공식 Docker/Compose containment evidence는 opt-in smoke gate `./docs/scripts/spec023-compose-smoke.sh`로 연결되어 있다. 이 문서는 Docker/Compose runtime containment, native unknown fallback, exec fail-closed, MCP/subagent containment inheritance를 현재 구현 수준에 맞춰 고정하며, kernel-level isolation이나 provider credential이 필요한 full MCP child execution smoke를 주장하지 않는다.

## 문서 목적

이 문서는 사용자가 `shacs-bot`을 self-hosted / personal-use 런타임으로 설치했을 때, sandbox 실행을 위해 별도의 host runtime component를 직접 설치하거나 조합하지 않아도 되는 최종 계약을 정의한다.

목표는 다음과 같다.

1. 기본 실행 경로가 사용자에게 gVisor, Firecracker, Kata, bubblewrap, 그 밖의 host runtime component 수동 설치를 요구하지 않도록 한다.
2. Docker/Compose 기반 공식 패키징과 native host 실행의 containment 의미를 분리한다.
3. `exec`, MCP stdio child process, subagent, deferred MCP tool call이 같은 runtime containment boundary를 소비하도록 한다.
4. permission과 auto approval이 containment 존재를 근거로 권한 판단을 생략하지 않도록 한다.
5. future Rust 구현에서 containment detection, fail-closed native exec, MCP stdio inheritance, packaging smoke test를 직접 도출할 수 있게 한다.

핵심 문장:

```text
Sandbox 실행의 기본 경로는 사용자가 별도의 보안 런타임을 설치하지 않아도 동작해야 한다.
```

이 문서는 보안 기술 목록을 늘리는 메모가 아니다. 구현이 이 문서와 충돌하면 "고급 사용자가 host에 뭔가 설치하면 된다"는 식으로 기본 제품 경험을 미루지 말고, 공식 패키징과 runtime containment 계약부터 다시 점검해야 한다.

---

## 상위 기준과의 관계

이 문서는 다음 spec을 소비한다.

| spec | 이 문서가 소비하는 것 | 이 문서가 소유하는 것 |
|---|---|---|
| 004 tool runtime | `RuntimeToolExecutor`, `ExecTool`, tool 결과와 interrupt 경계 | `exec` 실행이 알려진 containment 밖에서 sandbox를 주장하지 않는 계약 |
| 010 host safety, permissions, and secrets | workspace guard, process guard, MCP default-deny, future permission primitive | containment state를 host safety 판단의 입력으로 제공하는 계약 |
| 011 subagent runtime | child tool registry restriction, execution config inheritance | child execution이 parent보다 넓은 containment를 얻지 못한다는 계약 |
| 015 packaging, process lifecycle, and upgrades | Dockerfile, Docker Compose, install/start lifecycle | 공식 image/package가 zero-setup sandbox path를 제공해야 한다는 packaging 계약 |
| 016 verification matrix and release gates | release gate와 검증 family | sandbox packaging smoke와 fail-closed 안전성 테스트 요구 |
| 017 app operating environment | app/device/process/permission ledger 개념 | app이 띄우는 MCP stdio child가 같은 runtime containment를 상속해야 한다는 계약 |
| 020 tool search and provider tool surface | deferred MCP catalog와 bridge execution scope | deferred MCP 호출도 underlying child/process containment를 우회하지 못한다는 계약 |
| 022 auto approval permissions | permission mode, Docker primary containment, auto approval gate | zero-setup containment가 있어도 permission decision을 대체하지 않는 packaging/runtime 경계 |

이 문서의 owner boundary는 **zero-setup sandbox execution을 위한 packaging/runtime containment contract**다. 이 문서는 004의 tool 실행 알고리즘, 010의 전체 permission engine, 015의 모든 upgrade lifecycle, 022의 auto approval decision table을 대체하지 않는다.

---

## 범위

이 문서는 다음을 정의한다.

1. 사용자 별도 세팅 없는 sandbox 실행의 제품 요구.
2. 공식 Docker/Compose 실행 표면에서 기대하는 containment baseline.
3. `tools.exec.sandbox`와 `ExecTool`의 sandbox 계약.
4. MCP stdio child process가 runtime containment boundary를 상속하는 계약.
5. native host 실행에서 알려진 containment가 없을 때의 fail-closed 또는 scope narrowing 규칙.
6. permission, side-effect gate, auto approval과 containment state의 관계.
7. 금지 패턴과 TDD 검증 매트릭스.

이 문서는 다음을 정의하지 않는다.

1. Rust 구현 PRD.
2. Dockerfile 또는 `docker-compose.yml` 변경 내용.
3. gVisor, Firecracker, Kata 같은 별도 sandbox runtime 도입 계획.
4. 조직 단위 policy 배포, 중앙 관리자 승인, fleet 운영.
5. 완전한 kernel-level isolation 보증 문구.

---

## 현재 구현 상태

현재 저장소에서 확인되는 사실은 다음과 같다.

1. Dockerfile이 존재하며, runtime stage는 `useradd --create-home --uid 1000 --shell /bin/bash shacs`를 만들고 `USER shacs`로 non-root runtime user를 사용한다.
2. `docker-compose.yml`이 존재하며, `shacs-gateway`, `shacs-api`, `shacs-cli` 서비스를 제공하고 host의 `~/.shacs-bot`을 `/home/shacs/.shacs-bot`에 mount한다.
3. Compose의 `shacs-gateway` command는 `--allow-side-effects`를 명시한다.
4. `ExecTool`은 `ExecConfig.sandbox: Option<String>`을 받으며, config shape에는 `tools.exec.sandbox` 문자열이 있다.
5. 현재 sandbox backend는 `bwrap`만 알려져 있다. 알 수 없는 backend는 error가 되며, 빈 sandbox 설정은 wrapper 없이 shell command를 실행한다.
6. MCP stdio connector는 `McpServerSpec`의 command와 args를 정규화한 뒤 `Command::new(&command).args(&args)`로 child process를 직접 시작한다.
7. MCP tools, resources, prompts는 `enabledTools` 기본값이 빈 배열인 default-deny opt-in이다.
8. write/edit/exec 같은 side-effect tool surface는 CLI의 `--allow-side-effects` 또는 동등한 설정으로 등록 범위가 조절된다.
9. `runtime inspect`와 `runtime diagnostics`는 runtime containment summary와 digest를 남긴다. Native host에서 인식 가능한 containment evidence가 없으면 unknown state를 보존한다.
10. Unsafe privileged containment evidence는 `unsafe-privileged`로 분류되며, `bypass_permissions`는 native unknown과 unsafe privileged evidence 모두에서 default fallback으로 내려간다.
11. `bwrap`는 packaged availability가 확인되지 않은 경우 optional hardening으로 분류된다. Setup failure와 unknown backend는 원래 command를 silent unsandboxed fallback으로 실행하지 않는다.
12. MCP stdio와 subagent 경로에는 parent containment snapshot과 MCP default-deny를 보존하는 regression evidence가 있다.
13. App install, enable, disable 경로는 현재 app process를 시작하지 않는다. 따라서 app process inheritance는 process supervisor가 boundary를 물려주는 방식이 아니라, app process를 시작하거나 host access를 넓히지 않는 현재 동작으로만 만족된다.

이 사실들은 현 상태의 근거일 뿐이다. 특히 Dockerfile과 Compose가 존재하고 runtime user가 non-root라는 사실만으로 zero-setup sandbox execution이 완료됐다고 주장하면 안 된다. 공식 Compose runtime evidence는 `./docs/scripts/spec023-compose-smoke.sh`로 검증한다. MCP stdio containment inheritance는 provider credential 없이 실행 가능한 core regression tests가 parent containment snapshot과 default-deny 계약을 다루며, Compose smoke는 공식 container runtime evidence와 기본 Compose 안전 속성을 담당한다. App install, enable, disable 경로는 현재 app process를 시작하지 않으므로 app process가 containment를 넓히는 경로도 현재는 없다.

---

## 핵심 요구사항

1. 기본 경로는 사용자가 gVisor, Firecracker, Kata, bubblewrap, 별도 seccomp loader, 별도 VM runtime 같은 host component를 직접 설치하지 않아도 동작해야 한다.
2. 공식 image 또는 package가 sandbox 실행을 지원한다고 말하려면 필요한 runtime component와 config가 함께 제공되어야 한다.
3. `bwrap`는 선택적 hardening으로 남을 수 있다. 단, 공식 image/package가 자동으로 포함하고 설정하지 않는 한 기본 요구사항으로 삼으면 안 된다.
4. native host 실행에서 알려진 containment가 없으면 sandboxed라고 표시하면 안 된다.
5. native host 실행에서 containment를 확인할 수 없고 action이 side effect를 갖는다면 fail closed 하거나 permission/side-effect scope를 좁혀야 한다.
6. MCP stdio child process는 기본적으로 parent runtime과 같은 containment boundary를 상속해야 한다.
7. Docker socket mount와 privileged Docker-in-Docker은 기본 zero-setup path가 될 수 없다.
8. Containment state는 permission decision의 입력일 뿐이며, permission decision 자체를 대체하지 않는다.

---

## 대상 아키텍처

대상 구조는 세 층으로 나눈다.

1. Packaging containment layer.
2. Runtime containment detector.
3. Tool and child process execution boundary.

### Packaging containment layer

공식 Docker/Compose path는 사용자가 별도 sandbox backend를 설치하지 않아도 실행 가능한 기본 path다. 이 path는 최소한 다음을 충족해야 한다.

1. Runtime process가 non-root user로 실행된다.
2. Host data mount는 필요한 user data root로 좁힌다.
3. Docker socket을 mount하지 않는다.
4. `privileged: true`를 기본으로 두지 않는다.
5. Nested Docker daemon을 기본으로 띄우지 않는다.
6. Containment evidence를 runtime이 읽을 수 있는 형태로 제공한다.

### Runtime containment detector

Runtime은 현재 실행 환경을 아래처럼 분류할 수 있어야 한다.

1. `recognized_container`: 공식 image 또는 인식 가능한 container/devcontainer 안에서 실행 중이다.
2. `packaged_sandbox`: 공식 package가 필요한 sandbox component를 함께 제공하고 자동 설정했다.
3. `native_unknown`: native host 실행이며 알려진 containment가 없다.
4. `unsafe_privileged`: root, privileged container, Docker socket mount, host mount root 과다 노출 같은 위험 신호가 있다.

이 분류는 사용자에게 보여 줄 수 있는 diagnostics evidence를 가져야 한다. 다만 diagnostics는 raw secret, token, full environment dump를 남기면 안 된다.

### Tool and child process execution boundary

`exec`, MCP stdio child, subagent child, app device process는 같은 containment snapshot을 소비해야 한다. Child process를 만드는 코드가 parent보다 넓은 host access를 얻거나, sandbox 상태를 잃어버린 채 성공처럼 보이면 안 된다.

---

## exec sandbox 계약

`exec` sandbox 계약은 다음을 따른다.

1. `tools.exec.sandbox`는 선택된 exec sandbox backend의 이름이다.
2. 현재 알려진 backend가 `bwrap`뿐이라는 사실은 현재 구현 상태로 기록한다.
3. `bwrap`가 host에 수동 설치되어야만 동작하는 형태라면 기본 zero-setup path의 완료 조건이 아니다.
4. `bwrap`가 공식 image/package에 포함되고 자동 설정된다면 optional hardening이 아니라 packaged hardening으로 분류할 수 있다.
5. Unknown sandbox backend는 실행 실패로 처리해야 하며, 조용히 unsandboxed 실행으로 내려가면 안 된다.
6. `tools.exec.sandbox`가 비어 있고 runtime containment가 `native_unknown`이면 side-effect 있는 `exec`를 sandboxed라고 주장하면 안 된다.
7. Native host에서 알려진 containment가 없으면 `exec`는 fail closed 하거나 read-only/deny-pattern/workspace-bound scope처럼 더 좁은 policy로 내려가야 한다.
8. Docker container 안이라는 사실만으로 모든 `exec`가 자동 승인되면 안 된다.

---

## MCP stdio child process 계약

MCP stdio는 local child process를 시작하는 transport다. 따라서 MCP stdio child는 tool schema 노출 문제가 아니라 process containment 문제이기도 하다.

계약은 다음과 같다.

1. MCP stdio child process는 기본적으로 parent runtime과 같은 containment boundary를 상속해야 한다.
2. Parent가 공식 container path 안에서 실행 중이면 stdio child도 그 container 안에서 실행되어야 한다.
3. Parent가 packaged sandbox component를 통해 실행 중이면 stdio child도 같은 component 또는 동등한 자동 설정 경계를 통해 시작되어야 한다.
4. Parent가 `native_unknown`이면 stdio child를 조용히 sandboxed로 표시하면 안 된다.
5. Stdio child가 host runtime component, Docker socket, privileged nested runtime을 요구하면 기본 zero-setup path에서 제외해야 한다.
6. MCP default-deny는 그대로 유지되어야 한다. Containment가 있다고 해서 disabled MCP capability가 catalog나 tool registry에 나타나면 안 된다.
7. Deferred MCP tool call도 underlying stdio child boundary를 우회하면 안 된다.

App device로 등록된 MCP server에도 같은 규칙을 적용한다. App manifest가 child command를 선언했다는 이유만으로 permission이나 containment가 자동 확장되지 않는다.

---

## Docker/Compose packaging 계약

공식 Docker/Compose packaging은 zero-setup sandbox execution의 primary path다.

계약은 다음과 같다.

1. Compose path는 사용자가 host에 별도 sandbox backend를 설치하지 않아도 시작 가능해야 한다.
2. Runtime image는 non-root user 실행을 유지해야 한다.
3. 필요한 child process runtime, 예: Node 기반 MCP server 실행에 필요한 기본 runtime은 공식 image 안에 포함하거나 명확한 optional app requirement로 분리해야 한다.
4. Host mount는 user data와 workspace에 필요한 범위로 설명 가능해야 한다.
5. Docker socket mount는 기본값으로 금지한다.
6. `privileged: true`와 privileged Docker-in-Docker은 기본값으로 금지한다.
7. Host network mode는 기본 zero-setup sandbox path로 삼지 않는다.
8. Compose smoke test는 공식 container runtime evidence와 기본 Compose 안전 속성을 확인해야 한다. Provider credential 없이 full MCP child execution smoke를 반복 가능하게 만들 수 없는 동안, MCP stdio containment inheritance는 core regression tests의 parent containment snapshot/default-deny evidence로 연결한다.

Docker는 primary containment지만 완전한 permission waiver가 아니다. Permission gate, side-effect gate, protected target rule, diagnostics redaction은 Docker 안에서도 유지되어야 한다.

---

## permission/auto approval 관계

Containment와 permission은 역할이 다르다.

1. Containment는 실행 피해 범위를 줄이는 runtime boundary다.
2. Permission은 사용자의 의도와 scope 안에서 action을 실행해도 되는지 결정하는 policy boundary다.
3. Auto approval은 permission evaluator가 action을 실행 직전에 평가하는 gate다.

따라서 다음 원칙을 따른다.

1. `--allow-side-effects` 또는 config로 side-effect tool surface를 켜도 sandbox가 자동으로 완성되는 것은 아니다.
2. Sandbox가 확인되어도 `proc_exec`, `fs_write`, `external_delivery`, `secret_read` 판단은 남아야 한다.
3. `auto` 또는 `bypass_permissions` 같은 mode는 containment snapshot을 입력으로 받을 수 있지만, containment snapshot만으로 mode를 승격하면 안 된다.
4. Native host에서 containment가 없는데 `bypass_permissions`를 허용하면 안 된다.
5. Permission denial은 sandbox 부재를 이유로 더 강해질 수 있지만, sandbox 존재를 이유로 protected target denial을 약하게 만들면 안 된다.

---

## 금지 패턴

1. 기본 안내에서 gVisor, Firecracker, Kata, bubblewrap 수동 설치를 요구한다.
2. `bwrap`가 host에 없으면 사용자가 직접 설치하라고 한 뒤 이를 zero-setup 완료로 본다.
3. Docker socket을 mount해 host Docker daemon으로 sibling container를 띄우는 구조를 기본 sandbox path로 둔다.
4. `privileged: true` 또는 privileged Docker-in-Docker을 기본값으로 둔다.
5. Native host execution을 containment 확인 없이 sandboxed로 표시한다.
6. MCP stdio child process를 parent runtime containment 밖에서 직접 시작하면서 같은 sandbox에 있다고 기록한다.
7. Docker 안이라는 이유만으로 `--allow-side-effects`, permission mode, auto approval을 우회한다.
8. Deferred MCP bridge가 stdio child containment나 MCP default-deny를 우회한다.
9. 실패한 sandbox setup을 warning만 남기고 unsandboxed 실행으로 계속한다.
10. 조직 관리자, fleet policy, 중앙 approval service를 기본 사용자 흐름으로 전제한다.

---

## TDD 검증 매트릭스

### Unit tests

1. `ContainmentSnapshot` 분류가 official container, packaged sandbox, native unknown, unsafe privileged를 구분한다.
2. Unknown `tools.exec.sandbox` backend는 fail closed 한다.
3. Empty `tools.exec.sandbox`와 `native_unknown` 조합은 sandboxed evidence를 만들지 않는다.
4. `bwrap` backend는 packaged availability가 없으면 optional hardening으로만 분류된다.
5. Permission decision input에 containment snapshot이 들어가지만 mode source를 바꾸지 않는다.

### Integration tests

1. `ExecTool`은 sandbox wrapper 실패 시 unsandboxed shell로 fallback하지 않는다.
2. Native unknown path에서 side-effect exec는 fail closed 하거나 좁아진 scope로만 실행된다.
3. MCP stdio connector는 child process start에 parent containment snapshot을 전달하거나 같은 boundary 안에서 시작됐다는 evidence를 남긴다.
4. Subagent execution config는 parent containment ceiling을 넓히지 않는다.
5. Deferred MCP `tool_call`은 underlying MCP capability와 child process boundary를 그대로 소비한다.

### Packaging smoke tests

1. 공식 Docker image는 non-root user로 실행된다.
2. Compose path는 Docker socket 없이 시작된다.
3. Compose path는 privileged mode 없이 시작된다.
4. Compose path에서 `runtime inspect`가 `Runtime containment: contained=true`와 official-container backend를 보고한다.
5. Compose path에서 official package marker, container runtime evidence, no Docker socket inside the service가 관측된다.
6. MCP stdio child inheritance는 provider credential 없는 core regression tests로 parent containment snapshot/default-deny 계약을 검증한다.

### Safety tests

1. Sandbox setup 실패가 warning-only unsandboxed execution으로 이어지지 않는다.
2. Docker socket mount 또는 privileged mode가 감지되면 unsafe containment evidence가 남고 permissive mode가 차단된다.
3. Auto approval evaluator failure는 containment가 있어도 allow로 접히지 않는다.
4. Protected target denial은 Docker containment 안에서도 유지된다.
5. Diagnostics는 containment evidence를 남기되 secret과 full env를 노출하지 않는다.

---

## future PRD 후보

아직 PRD를 만들지 않는다. 필요할 때 아래 순서로 나눌 수 있다.

1. `000-containment-snapshot-and-diagnostics.md`: runtime containment detection, evidence redaction, unsafe privileged classification.
2. `001-exec-fail-closed-and-packaged-hardening.md`: exec sandbox backend resolution, no silent fallback, native unknown scope narrowing.
3. `002-mcp-stdio-containment-inheritance.md`: stdio child containment inheritance, default-deny preservation, deferred MCP bridge regression.
4. `003-compose-zero-setup-packaging-smoke.md`: official image/Compose non-root, no Docker socket, no privileged default, child process smoke.
5. `004-permission-auto-approval-containment-input.md`: containment snapshot integration with permission action and auto approval without mode bypass.

---

## 완료 기준

이 spec의 완료 기준은 다음이다.

1. 사용자가 공식 Docker/Compose path를 통해 별도 host sandbox runtime 설치 없이 sandbox 실행 경계를 얻는다.
2. 공식 image/package가 필요 component를 포함하거나, 포함하지 않는 backend를 optional hardening으로 명확히 분류한다.
3. Native host 실행은 알려진 containment 없이 sandboxed라고 주장하지 않는다.
4. Native host에서 side-effect execution은 fail closed 하거나 좁은 permission/side-effect scope로 내려간다.
5. `ExecTool` sandbox setup 실패는 silent unsandboxed fallback이 아니다.
6. MCP stdio child, subagent child, app device process가 parent runtime containment boundary를 기본 상속한다.
7. Docker socket mount와 privileged Docker-in-Docker이 기본값으로 금지된다.
8. Permission/auto approval은 containment snapshot을 입력으로 소비하되, containment를 permission bypass로 쓰지 않는다.
9. 016 release gate에 packaging smoke, integration, safety regression evidence가 연결된다.
10. 문서와 diagnostics가 현재 구현을 과장하지 않고, zero-setup sandbox execution 완료 전에는 완료라고 말하지 않는다.

현재 traceability는 다음처럼 해석한다.

1. Criteria 1과 2는 `README.md`, `docs/USAGE.md`, Dockerfile, `docker-compose.yml`, `./docs/scripts/spec023-compose-smoke.sh`로 Docker/Compose primary path, `bwrap` optional hardening, official-container runtime evidence를 설명하고 검증한다.
2. Criteria 3은 `runtime_containment_classifier_reports_native_unknown`와 `runtime_containment_snapshot_ref_preserves_unknown_state`로 native unknown classification과 digest 보존을 확인한다.
3. Criteria 4와 5는 `exec_tool_native_unknown_without_backend_enforces_workspace_scope`, `exec_tool_unknown_sandbox_backend_does_not_execute_command`, `exec_tool_bwrap_sandbox_setup_failure_does_not_execute_original_command`로 native unknown scope narrowing과 sandbox setup fail-closed를 확인한다.
4. Criteria 6은 `mcp_runtime_connects_registers_and_closes_servers`, `mcp_default_deny_excludes_disabled_capabilities_from_tool_search_bridge`, `subagent_permissioned_action_context_inherits_snapshots_and_origin`으로 MCP stdio, deferred MCP bridge, subagent inheritance를 확인한다. Provider credential 없이 full MCP child execution smoke는 반복 가능하지 않으므로 Compose smoke가 이 항목까지 과장하지 않는다. App process는 현재 inert라서 시작되지 않고 host access를 넓히지 않는 동작만 문서 증거로 남는다.
5. Criteria 7은 `README.md`, `docs/USAGE.md`, `docker-compose.yml`, `./docs/scripts/spec023-compose-smoke.sh`의 default Compose path check가 Docker socket mount, `privileged: true`, host network를 기본값으로 쓰지 않는다는 config evidence로 연결된다.
6. Criteria 8은 `runtime_containment_classifier_reports_unsafe_privileged_evidence`, `bypass_permissions_falls_back_for_native_unknown_containment`, `bypass_permissions_falls_back_for_unsafe_privileged`로 containment snapshot이 permission bypass로 바뀌지 않음을 확인한다.
7. Criteria 9는 `docs/specs/016-verification-matrix-and-release-gates/prds/000-spec-coverage-and-release-readiness.md`의 Spec023 release evidence lane에 연결된다.
8. Criteria 10은 `README.md`, `docs/USAGE.md`, 이 문서의 Active status, `./docs/scripts/spec023-compose-smoke.sh`, 그리고 `runtime inspect`/`runtime diagnostics` containment summary/digest 문구로 연결된다.
