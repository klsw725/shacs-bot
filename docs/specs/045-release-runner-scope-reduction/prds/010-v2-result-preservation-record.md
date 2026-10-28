# PRD 010. v2 결과 보존 기록

Status: Complete (Scoped)

Depends on: [PRD 000](000-passed-cargo-test-count-check.md) 완료. 뒤 번호 PRD에 의존하지 않는다.

## Goal

v2 판독 장치를 지우기 전에, canonical v2 결과 10개를 지금 판정 그대로 다시 확인할 수 있는 추적되는 보존 경로를 만든다. runner 코드는 바꾸지 않는다.

## Scope

1. **보존 문서:** `docs/archive/spec031-v2-results/`에 다음을 둔다.
   - 결과마다 파일별 sha256 기록(결과 폴더 기준 상대 경로, `shasum -a 256` 형식). 대상은 [041 판정](../../041-release-evidence-tooling-disposition/prds/000-runner-consumers-evidence-and-disposition.md#판정-2026-10-25-범위-축소)의 canonical 10개이며, 각 결과의 `.omo/worktree-archives/spec035-20260928/` 기준 위치를 적는다. 테스트 임시 출력 49개(`closure/f1-r01-owned-tmp/`)는 035 인계와 같이 대상에서 뺀다.
   - 결과별 당시 schema·행 수·summary 상태와 고정 commit validator의 판정표. 041 조사에서는 G7 두 개(`closure/finalization/g7-integrated/{runner-output,retained-originals}`)가 valid, 나머지 8개가 `UnsupportedSchema`였다.
   - 고정 commit hash, harness 원본(Cargo.toml과 `main.rs`. 경로 의존성은 꺼낸 소스를 가리키는 자리 표시로 적는다)과 실행 방법. 결과를 복사하고 `manifest.json`의 `evidence_root`만 사본 경로로 바꾼 뒤, `git archive <고정 commit>`으로 꺼낸 소스에 연결해 빌드한 harness로 판독한다.
   - 검사 방법. `shasum -a 256 -c`와 실제 파일 목록 비교를 함께 쓴다(`shasum -c`만으로는 추가된 파일을 잡지 못한다, 041 s09).
   - 이 기록이 증명하는 것과 증명하지 않는 것. 기록은 byte 동일성만 증명하고, 결과의 의미는 고정 commit의 validator와 당시 source로만 재현한다. G7의 146행·owner 6개·`pending-final-audit`와 다른 8개의 `BLOCKED`·`PASS` summary는 당시 schema의 결과로만 읽고, 현행 65행·owner 4개로 재분류하거나 현재 PASS로 재결속하지 않는다. 원본 폴더는 추적하지 않는 로컬 자료라서 잃으면 기록만으로는 결과를 읽을 수 없다.
2. **고정 commit:** PRD 000의 수정과 v2 판독 장치를 모두 담고, 보존 문서를 쓸 때 이미 main에 들어가 있는 commit이다. 보존 문서를 더하는 변경은 runner 코드를 바꾸지 않으므로 이 commit의 validator는 PRD 020이 지우기 직전의 validator와 같다. 기록한 뒤에는 이 commit의 hash가 바뀌지 않도록 main 이력을 다시 쓰지(rebase·squash) 않는다.
3. **검사:** 원본 10개와 041 s02–s08 사본에 기록 검사를 돌리고, 고정 commit에서 다시 빌드한 harness로 판정표를 재현한다.

## Non Scope

- v2 판독 장치 삭제와 그에 따른 문서 안내([PRD 020](020-v2-reader-removal-and-docs.md)).
- 원본 폴더와 임시 출력의 이동·삭제·수정, `docs/archive/spec035/`의 변경.

## Parent Requirement Mapping

- [045 SPEC](../SPEC.md)의 소유 범위 2와 수용 조건 2를 소유한다.

## Required Contract

1. 보존 문서만으로 결과 byte의 변경·누락·추가를 거절할 수 있다.
2. 판정표는 고정 commit validator의 실제 실행 결과이며 손으로 적은 기대값이 아니다.
3. 보존 문서는 v2 결과를 현재 release PASS로 다시 해석하지 않는다.

## Acceptance Criteria

1. 기록 검사에서 원본 10개는 통과하고, s02–s08 사본(바뀐 transcript, 지운 binding, 추가된 파일, pass로 바꾼 blocked 행, 실패로 바꾼 명령, binding 없는 v2의 BLOCKED→PASS 위조)은 모두 거절된다.
2. `git archive <고정 commit>`에서 다시 빌드한 harness가 원본 10개의 사본에서 판정표와 같은 결과를 낸다.
3. 고정 commit이 PRD 000의 수정과 v2 판독 장치를 담고 있고, `git merge-base --is-ancestor <고정 commit> main`이 성공한다.

## Closure Evidence

1. 보존 문서.
2. 기록 검사와 고정 commit harness의 실행 기록.

PRD 010은 PRD 000의 닫힌 결과만 소비한다. PRD 020의 삭제를 기다리지 않는다.

## 구현 메모 및 검증 근거 (2026-10-28)

실행 기록은 로컬 `.omo/evidence/spec045/prd010/`(추적하지 않음)에 있다.

- 보존 문서: [`docs/archive/spec031-v2-results/`](../../../archive/spec031-v2-results/README.md)의 README와 결과별 sha256 기록 10개. 기록은 원본 결과에서 새로 만들었고 041 조사의 기록(`.omo/evidence/spec041/prd000/preservation/records/`)과 byte가 같다. 원본 폴더는 041 조사 뒤 바뀌지 않았다.
- 고정 commit: `0f0279973a12af65b9616fc41163748b367fc937`. PRD 000의 수정(`2a0bf59`)과 v2 판독 장치(`release_runner/spec035_*` 44개 파일)를 모두 담고 이미 main에 있으며, `git merge-base --is-ancestor`가 성공했다. 이 PRD의 변경은 문서뿐이라 고정 commit의 validator는 PRD 020이 지우기 직전의 validator와 같다.
- 기록 검사(`record-checks.txt`):
  - 원본 10개는 빠진·추가된·바뀐 파일과 symlink를 보는 검사, README의 `shasum -a 256 -c`와 파일 목록 비교를 모두 통과했다.
  - G7 runner-output의 041 사본은 그대로인 사본만 통과하고, 바뀐 transcript·지운 binding·추가된 파일·pass로 바꾼 blocked 행·실패로 바꾼 명령의 사본은 거절됐다(s02–s07). f1-r01-current-final의 사본은 그대로인 사본만 통과하고 BLOCKED→PASS 위조는 거절됐다(s08).
  - 파일을 추가한 사본은 `shasum -c`만으로는 통과했고 파일 목록 비교가 그 파일을 잡았다(041 s09와 같다).
- 판정표 재현(`verdicts.txt`): README의 절차대로 원본 10개를 복사해 `evidence_root`만 바꾸고, `git archive 0f02799`로 꺼낸 소스에서 빌드한 harness를 그 소스를 repo root로 실행했다. G7 두 개는 valid, 나머지 여덟 개는 `UnsupportedSchema`로 표와 같다. 같은 사본을 PRD 000 이전 소스(`4b449f9`, 기록의 `harness-head`)와 `0f02799`와 같은 작업 트리 코드(`harness-fix`)에서 빌드한 harness로 판독해도 같은 결과였다.

## 종료 확인 (2026-10-28)

- 범위: 보존 문서, 고정 commit, 기록 검사와 판정표 재현. v2 판독 장치 삭제와 그 문서 안내(PRD 020)는 포함하지 않는다.
- 코드를 바꾸지 않아 Cargo 검증은 하지 않았다(AGENTS.md의 문서 전용 변경).
- Oracle 독립 검증: **PASS** (2차).
  - 1차는 BLOCKED였다. README가 아직 하지 않은 v2 판독 삭제 뒤의 판정을 과거형으로 적었다는 지적이었다. 지금의 판정과 PRD 020 뒤에도 지켜야 할 판정으로 나눠 고치고, 재현 절차에 `<work>/source` 생성과 `cargo run` 명령을, 판정표 기록에 각 harness의 commit을 더했다.
- 남은 제한:
  - 기록은 byte가 같다는 것만 증명한다. 원본 폴더는 추적하지 않는 로컬 자료라서 잃으면 기록만으로는 결과를 읽을 수 없다.
  - 고정 commit은 이 기록을 쓸 때 로컬 main에만 있었다. main을 push하거나 병합할 때 이력을 다시 쓰면 고정 commit을 잃는다.
