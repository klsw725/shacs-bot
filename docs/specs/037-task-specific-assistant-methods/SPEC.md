# 037. 작업별 수행 방식의 선택과 결과 확인 아키텍처 명세

Status: Complete (Scoped)

Consumes: 004 tool runtime, 005 skills, 024 workflow/harness execution, 030 trusted execution, 034 generated media

## 목적과 완료 경계

이 스펙은 같은 개인 어시스턴트가 **일의 성격에 맞는 수행 방식과 확인 기준을 고르고, 사용자에게 보여주고, 사용자가 변경할 수 있게 하는 경험**을 소유한다. 기존 [024](../024-dynamic-workflows-and-harness-orchestration/SPEC.md)는 workflow plan, pattern, child/verifier, budget, resume, recipe 공유를 이미 소유한다. 037은 새로운 workflow engine이나 하네스 DSL을 만들지 않고, 일반 실행과 기존 skill/recipe/workflow를 사용자 목표·결과 확인에 맞게 연결한다.

현재 workflow admission은 복잡도와 keyword를, ready recipe 정렬은 입력에 포함된 recipe/skill 이름을 주로 사용한다. Shell로 Python을 호출하거나 `image_generate`, 일반 파일 작성 도구로 결과를 만들 수는 있지만 이것이 계산 검증, 보고서 출처 확인, 이미지 검토를 갖춘 전문 작업 경험의 완료는 아니다. 037은 지속 관찰이나 방법 학습 없이도 **사용자가 지금 요청한 한 작업**으로 단독 완료돼야 한다.

## 소유 범위

1. **선택과 변경:** 일반 수행으로 충분하면 전문 방식을 강제하지 않는다. 지원되는 계산·보고서·이미지 요청에서는 사용할 방식, 선택 이유, 필요한 도구/가용 상태와 결과 확인 항목을 사용자가 알 수 있게 한다. 사용자는 시작 전에 방식을 지정하거나 진행 중 다른 지원 방식으로 변경할 수 있다. 변경 후에도 사용자 목표·이미 내린 결정·확인된 산출물을 잃거나 과거 검증 상태를 새 방식에 무조건 승계하지 않는다.
2. **한 요청의 여러 방식:** 보고서에 계산 결과나 생성 이미지를 넣는 등 한 요청에서 둘 이상을 사용할 수 있다. 각 방식은 자신의 중간 산출물과 검증 결과를 기록하되 최종 답과 남은 질문은 같은 사용자 요청에 귀속한다. 다른 방식의 성공을 근거로 실패한 필수 검증을 통과 처리하지 않는다.
3. **작업에 맞는 확인:** 계산은 사용한 입력·단위·가정·실행 결과와 가능한 독립적 확인을 남긴다. Python 명령의 종료 코드 0만으로 수치의 정답을 주장하지 않는다. 보고서는 사용한 자료와 확인 가능한 출처, 결론의 근거와 확인되지 않은 부분을 구별한다. 이미지는 실제 생성 artifact와 사용자의 요구 조건에 대한 확인·수정 결과를 분리하고 주관적인 품질을 자동 검증 성공으로 단정하지 않는다.
4. **결과 우선 표면:** 사용자에게는 답/산출물, 핵심 근거, 아직 확인이 필요한 부분을 먼저 보여준다. 코드·도구 실행 기록·검증 상세는 필요할 때 확인할 수 있어야 한다. provider나 도구가 없으면 `unsupported`/`unavailable`을 명확히 표시하고 일반 답변으로 조용히 성공을 합성하지 않는다.

## 기존 owner와의 경계

| 기존 owner | 037이 소비하는 것 | 037이 소유하지 않는 것 |
|---|---|---|
| [004](../004-tool-runtime/SPEC.md), [005](../005-skill-system/SPEC.md) | 도구 호출 결과, skill discovery·injection | tool 권한/registry와 skill 설치·신뢰 출처의 재정의 |
| [024](../024-dynamic-workflows-and-harness-orchestration/SPEC.md) | plan·recipe·verifier·resume·취소 경계 | 새 workflow 패턴, child orchestration, worktree·merge·budget 정책 |
| [030](../030-trusted-agent-runtime-and-operational-controls/SPEC.md) | 현재 hook/confirmation, process, sandbox, credential, disclosure 경계 | universal Python sandbox, 외부 행동 자동 승인, raw secret 보장 |
| [034](../034-generated-media-and-rich-file-context-expansion/SPEC.md) | provider별 이미지 생성·artifact와 미지원 상태 | 이미지 provider parity, editor UI, 영상 분석의 재구현 |

037은 계산을 위해 반드시 전용 Python kernel을 도입하지 않는다. 기존 shell 등 실제 실행 경로를 사용할 수 있지만, 사용한 실행 방식과 검증의 한계는 기록한다. 024의 복잡한 workflow가 모든 단순 요청에 필요하다고 가정하지 않는다.

## 반드시 지킬 것

- 선택된 방식은 사용자 지시로 바꿀 수 있어야 하지만, 이미 실행한 외부 효과를 조용히 되돌렸다고 주장하지 않는다. 취소·재실행은 기존 owner 경계를 통과한다.
- 도구가 성공했다는 사실, 사용자가 결과를 승인했다는 사실, 계산·출처·이미지가 실제 요구를 충족한다는 사실을 별개의 증거로 남긴다.
- 특정 모델 entitlement나 외부 이미지 provider, 네트워크, 선택적 sandbox가 없는 환경에서 성공을 합성하지 않는다.
- 지속 위임·선제 알림, 방법 후보 승격·철회, 새로운 앱 marketplace, 이미지 편집 UI는 037의 완료 조건이 아니다.

## 수용 조건

1. 같은 사용자 session에서 일반 요청은 일반 방식으로 처리하고, 계산·보고서·이미지 요청은 지원 가능한 수행 방식과 이유를 표시한다. 사용자가 방식을 명시하거나 도중에 바꾸면 변경 사실과 현재 목표·이미 확인된 근거가 결과에 반영된다.
2. 계산에서는 재현 가능한 입력·가정·실행 결과와 적어도 하나의 독립적인 확인/반례 검토를 남긴다. 의도적으로 단위를 바꾸거나 반례 입력을 넣으면 검증 실패 또는 불확실성으로 나타나며 성공을 합성하지 않는다.
3. 보고서는 실제 읽은 출처를 연결하고 없는 출처를 만들어내지 않는다. 이미지 작업은 구성된 provider의 결과 artifact를 보여주고 요구 조건 확인과 사용자 수정 요청을 이어가며, provider 미설정/미지원 시 실패 이유를 보여준다. 미관측 주관적 품질을 PASS로 표시하지 않는다.
4. 보고서에 계산 또는 이미지를 포함하는 복합 요청에서도 동일 사용자 목표를 유지하고, 각 산출물의 검증 상태를 독립적으로 확인할 수 있다. 답과 핵심 근거가 먼저 나오고 코드·상세 실행 기록은 별도로 확인된다.
5. 실제 CLI 또는 대화 표면에서 일반·전문·방식 변경·미지원 입력을 실행해 관측한다. 해당 결과는 mock 화면이나 024의 workflow unit test만으로 대신하지 않는다.

위 조건은 037 자체의 완료 기준이다. 감시 작업이나 학습된 방법 없이도 현재 도구·기존 skill/recipe를 사용해 검증할 수 있다.

## 실행 PRD와 독립 종료

각 PRD는 자신의 사용자 결과와 검증 증거로 닫힌다. 앞 번호 결과만 소비하며 뒤 PRD의 구현을 종료 조건으로 삼지 않는다. 구현 전 상태는 모두 `Planned`다.

| PRD | 유일하게 소유하는 결과 | 앞선 PRD 소비 | 자체 완료 표면 |
|---|---|---|---|
| [000 방식 선택·사용자 변경](prds/000-method-selection-and-user-override.md) | 일반/전문 선택, 선택 이유, 사용자 override와 목표 보존 | 없음 | 한 요청의 방식 표시·변경 |
| [010 계산 검증](prds/010-reproducible-calculation-evidence.md) | 입력·단위·가정·실행·독립 확인 | 000 | 계산 답과 검증 상태, 코드 상세 조회 |
| [020 보고서 출처](prds/020-source-grounded-report-output.md) | 실제 읽은 출처와 결론이 결속된 보고서 | 000 | 보고서·출처·수정 결과 |
| [030 이미지 검토](prds/030-image-result-review-and-revision.md) | 생성 artifact·요구 확인·사용자 수정과 미지원 상태 | 000 | 이미지 결과·수정 또는 unavailable |
| [040 복합 결과 인계](prds/040-composed-method-results-and-handoff.md) | 한 목표의 다중 산출물·독립 검증·답 우선 표면 | 000, 010, 020, 030 | 보고서+계산 및 지원 시 보고서+이미지 실제 표면 |

소유 범위 1과 수용 조건 1의 선택·변경은 000, 범위 3의 계산은 010·보고서는 020·이미지는 030이 각각 단독 소유한다. 범위 2·4의 복합 결과와 수용 조건 4–5의 통합 표면은 040이 소유한다. 조건 2는 010, 조건 3은 020/030이 각 분야별로 닫는다. 037 최종 판정에는 다섯 PRD의 종료 증거와 040의 실제 사용자 결과를 함께 확인하되 별도 뒤 번호 PRD는 요구하지 않는다.

## 종료 증거 (2026-10-23)

PRD [000](prds/000-method-selection-and-user-override.md), [010](prds/010-reproducible-calculation-evidence.md), [020](prds/020-source-grounded-report-output.md), [030](prds/030-image-result-review-and-revision.md), [040](prds/040-composed-method-results-and-handoff.md)이 각각 독립 Oracle **PASS**를 받았다(000은 3차, 010은 7차, 020은 7차, 030은 9차, 040은 16차). 마지막 Oracle 검토는 이 스펙의 수용 조건 1–5와 반드시 지킬 것 전체도 **PASS**로 판정했다. 이는 현재 self-hosted 지원 범위의 scoped 완료이며 전체 release seal이 아니다.

- 조건 1: 같은 session에서 일반 요청은 일반 방식으로 처리한다. 계산·보고서·이미지 요청은 방식, 선택 이유, 가용 상태, 확인 항목을 보인다. 시작 전 지정과 진행 중 변경 뒤에도 목표·사용자 결정·확인된 근거를 잇고, 이전 판의 검증을 승계하지 않는다(000, 040).
- 조건 2: 계산은 실행한 코드의 입력·단위·가정·결과를 기록하고, 다른 코드로 한 독립 확인으로 판정한다. 단위를 바꾸거나 반례를 넣으면 `conflict`나 미확인으로 남는다(010).
- 조건 3: 보고서는 이 요청에서 실제로 읽은 자료에 결론을 연결하고 없는 출처를 만들지 않는다(020). 이미지는 저장이 확인된 artifact만 결과로 보이고 형식·크기·장수 요구를 확인한다. 미설정·미지원·실패는 그 이유를 보이고, 내용과 모양은 미확인으로 남긴다(030).
- 조건 4: 보고서에 계산이나 이미지를 넣는 복합 요청은 같은 목표 아래 부분마다 따로 판정한다. 한 부분의 성공이 다른 부분을 통과시키지 않는다. 답과 핵심 근거가 먼저 나오고, 코드·실행 기록·출처 상세는 `/method status`에서 본다(040).
- 조건 5: macOS native와 공식 container 조건의 compiled REPL에서 일반·전문·방식 변경·미지원 입력을 실행해 관측했다(각 PRD의 `qa-*.txt`).

fmt와 workspace clippy `-D warnings`가 통과했다. 마지막으로 바꾼 package(`shacs-core`)의 테스트는 1,583 passed / 1 failed / 3 ignored이며, 유일한 실패는 baseline부터 있던 Spec034 fixture 테스트다. 이 package 실행은 PRD040 14차 반영 코드에서 했다. 15차 반영(두 판정 함수와 단위 테스트) 뒤에는 대상 테스트(AgentLoop 124/124, 단위 59/59, CLI 2/2·2/2·4/4, command 3/3)를 다시 실행했다. 전체 workspace 테스트는 PRD040 8차 반영 코드의 3,304 passed / 1 failed / 13 ignored가 마지막이다. 그 뒤로는 저장소 작업 지침에 따라 사용자 승인을 기다린다.

이미지 내용·모양 확인, 이미지 편집·마스크·변형, 전용 Python 서비스, 새 workflow engine, 방법 학습은 제공하지 않는다. 이미지·복합 요청에서는 확장 추론(thinking)을 요청하지 않는다. 실제 명령과 제한은 [사용 가이드](../../USAGE.md#복합-요청의-결과-확인)를 따른다.
