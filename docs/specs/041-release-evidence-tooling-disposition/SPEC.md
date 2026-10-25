# 041. 로컬 release evidence 도구의 유지·축소·제거 판정 명세

Status: Complete (Decision)

Consumes: 016 verification gates, 031 release artifact contract, 033 and 034 independent release evidence

## 목적과 종료 조건

이 스펙은 개인 어시스턴트의 직접 실행 기능이 아닌 **`spec031-release-runner` 바이너리, 공개 Rust 실행·검증 API, 보존된 evidence schema**의 필요 범위를 판정한다. 런타임 session·config·snapshot/replay 의미나 033/034의 별도 release runner를 재소유하지 않는다. 제품의 첫 경험을 우선한다는 이유만으로 검증 도구를 dead code로 판정하거나 기존 실패 테스트를 지우지 않는다.

완료 산출물은 runner의 실제 소비·실행 비용·대체 검증 능력·artifact 호환성을 조사해 **유지 / 범위 축소 / 제거** 중 근거 있는 결론 하나와 후속 구현 계약을 기록한 결정문이다. repo 밖 사용자 스크립트 또는 과거 증거 소비가 확인되지 않았다는 사실은 `없음`의 증거가 아니다. 판정 증거가 불충분하면 `미결정`으로 남기고 완료하지 않는다. 036–040과 무관하게 이 도구만으로 독립 판정한다.

## 조사할 현재 계약

1. **표면과 호출자:** `crates/shacs-projection/src/bin/spec031-release-runner.rs`의 `--run-id`, `--repo-root`, `--evidence-root`, `--mode`, stdout/stderr/exit code와 `shacs-projection`의 공개 `run`·artifact validator API 소비자를 조사한다. `README.md`, `docs/USAGE.md`, 테스트·CI·사용자 소유 로컬 자동화에서 실제 호출을 구분한다. 제품 런타임에서 직접 호출되지 않는다는 사실만으로 삭제를 결정하지 않는다.
2. **검증 내용과 비용:** 현재 worktree의 Cargo 명령, 정확한 외부 owner fact, requirement/command/출력 transcript 결속, dirty source 관측, cleanup·triage·실패 차단을 항목별로 나누고 실행 시간·중복 검사의 비용을 측정한다. `success-fixture`의 PASS는 실제 source-bound release 승인과 분리한다.
3. **보존 자료와 호환성:** 현행 `spec031.release_runner.v5`(이 스펙을 쓸 때는 v3였고 [044](../044-video-analyzer-retirement/SPEC.md)·[043](../043-app-capability-retirement/SPEC.md)에서 v4·v5가 됐다)와 보존된 v2–v4 artifact, coverage/index/manifest 및 [`035 비활성 자료`](../../archive/spec035/README.md)의 source hash 검증을 서로 다른 계약으로 구분한다. artifact를 읽는 도구, 저장된 결과의 locator와 validator, 오류/차단 의미를 파악한다. 이전 실행을 새 소스의 PASS로 재결속하지 않는다.
4. **대체 가능성:** 단순히 `cargo test`가 성공하는 것 외에 source identity, 각 requirement의 실행·실패·누락 근거, artifact 변조 거부와 로컬 사용자 진단을 누가 제공할 수 있는지 비교한다. 033·034 runner의 별도 소유권과 재사용 가능성은 조사하되 그것들이 031 계약을 자동 대체한다고 추론하지 않는다.

## 판정 규칙

| 결론 | 필요한 증거와 후속 계약 |
|---|---|
| 유지 | 실제 로컬/자동화 소비나 독립 검증 가치가 확인되고, 현재 명령·공개 API·artifact/validator와 유지할 필수 검사·비용 한도를 명시한다. |
| 범위 축소 | 제거할 명령/산출물/API의 소비자를 확인하고, 남길 검증이 같은 누락·실패·변조 입력을 차단함을 실증한다. v2–v4 보존 자료의 조회·해석, CLI 변경 안내와 출처 연결이 깨지지 않을 이행 경로를 제시한다. |
| 제거 | 현행 바이너리·API·artifact 소비와 외부 사용자 가능성을 조사하고, 기존 증거를 읽고 검증할 별도 보존 경로와 대체 release gate를 실제 실행으로 증명한다. 새 증거 생성 중단과 과거 증거 판독 중단은 각각 별도 결정이다. |

## 변경 전 불변식

- missing/unknown owner fact, 실패한 Cargo 명령, 불완전한 cleanup, stale source 또는 tampered artifact를 PASS로 바꾸지 않는다. fixture 실행을 semantic closure라고 쓰지 않는다.
- 테스트를 지우거나 assertion을 약하게 바꾸어 검증 비용을 낮추지 않는다. 대체 gate가 있으면 같은 실패 입력으로 실제 동등성을 보여야 한다.
- 031 runtime snapshot/replay와 config migration, 033·034 별도 release runner를 이 도구의 retire 결정만으로 삭제하지 않는다.
- 유효한 과거 v2–v4 artifact는 당시 schema와 source 기준으로 해석한다. 보관된 035 owner의 status나 당시 146행 결과를 현행 65행 계약(이 스펙을 쓸 때는 66행)으로 조용히 재분류하지 않는다.
- 이 평가 중 개인 사용자의 실제 data/config/auth root에 쓰거나 기존 release evidence를 덮어쓰지 않는다.

## 판정 수용 조건

1. CLI 및 공개 API 호출처, 테스트·문서·CI, 사용자 로컬 스크립트/기존 artifact 소비의 조사 범위와 미확인 사항을 목록화한다. `--help`, 정상 fixture, 실패 입력 및 실제 current-worktree의 서로 다른 판정을 격리된 evidence root에서 관측한다.
2. 현행 요구사항별 명령·source·artifact 대응, 실행 비용과 차단 이유를 기록한다. 축소/제거 후보에는 동일한 누락·명령 실패·artifact 변조를 주어 대체 경로의 fail-closed 동등성을 검증한다. 대체가 없다면 그 사실을 결론에 반영한다.
3. 현재와 과거 artifact의 읽기·검증·문서 링크, CLI/API 사용 방식과 033/034별 독립 runner에 대한 영향표를 남긴다. 결론 하나의 근거·보존 계약·구현 범위·회귀 기준을 담은 결정문이 있어야 완료된다. 증거가 없으면 미결정 사유를 기록하고 완료하지 않는다.

이 스펙은 **평가와 선택만** 완료한다. 선택한 구현·제거, 출시 승인이나 과거 근거의 소급 봉인은 후속 작업이며 다른 판정 스펙의 완료를 기다리지 않는다.

## 실행 PRD

| PRD | 유일하게 소유하는 결과 | 선행 PRD |
|---|---|---|
| [000 runner 소비·대체 경로 판정](prds/000-runner-consumers-evidence-and-disposition.md) | 조사 계약 1–4, 판정 수용 조건 1–3의 호출·증거/호환·대체 검증과 유지/축소/제거 결정문 | 없음 |

PRD 000은 실행 비용만 측정하거나 fixture PASS만 얻은 상태로 닫지 않는다. 근거를 갖춘 결정 후의 `Complete (Decision)`은 release runner 코드 제거·축소 또는 현재 소스의 출시 승인이 아니다.

## 판정 결과 (2026-10-25)

결론은 **범위 축소**다. CLI, v5 생성, v5·v4·v3 판독과 033·034·030 runner가 쓰는 공용 명령 실행 API는 남기고, 퇴역한 035 시기의 v2 판독 장치와 production 소비자가 없는 공개 API를 지운다. 근거·보존 계약·구현 순서·회귀 기준은 [PRD 000의 판정](prds/000-runner-consumers-evidence-and-disposition.md#판정-2026-10-25-범위-축소)에 있다.

- 현재 경로는 대체가 없다. 031 요구사항 65행을 명령·transcript·owner fact·035 보관 자료 hash에 묶어 실패 명령·누락·transcript 변조·보관 자료 변조를 거절했다. `cargo test`와 033·034 runner는 이 결속을 대신하지 않는다.
- v2 판독 장치는 runner 코드의 60%(`spec035_*` 6,865줄)와 단위 테스트 113개 중 100개(약 36초)다. 새 v2 결과는 만들어지지 않는다. 035 보관 폴더의 v2 결과 59개 중 canonical 10개에서 8개, 테스트 임시 출력 49개 전부가 지금도 `UnsupportedSchema`이며 읽히는 것은 G7 두 개뿐이다. 043·044는 이 장치를 고쳐야 했다.
- canonical v2 결과 10개는 파일별 sha256 기록과 v2 판독을 지우기 직전 commit의 validator로 읽는다. `git archive`로 꺼낸 commit에서 다시 빌드한 validator는 이 조사의 98쌍 입력 모두에서 지금과 같은 판정을 냈고, sha256 기록은 원본 10개를 통과시키고 변조·누락·추가·위조 사본을 모두 거절했다. 임시 출력 49개는 지우지 않지만 035 인계와 같이 보존 대상에서 뺀다.
- 조사 중 현재 validator가 실패한 `cargo test --workspace` 기록을 통과로 고치고 test 개수를 비우면 PASS로 받아들이는 결함을 찾았다. 축소와 별개로 후속 구현의 첫 단계에서 고친다. 이 수정은 v3–v5 판독도 강화해 "통과 + 개수 없음" 기록을 새로 거절한다. 그때까지 PASS는 runner가 직접 만든 evidence root에서만 믿는다.
- 이 checkout의 current-worktree 실행은 추적하지 않는 파일을 읽는 034 테스트 하나 때문에 `BLOCKED`다. runner 결함이 아니며 034 쪽 일로 남긴다.

구현은 별도 작업이다. 그때까지 runner 동작은 그대로다. 이 판정은 033·034 runner와 031 snapshot·config 계약을 바꾸지 않고 다른 판정 스펙을 기다리지 않는다.
