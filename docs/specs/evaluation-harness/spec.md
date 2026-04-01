# SPEC: Meta-Harness Inspired Evaluation Harness

> **Prompt**: meta-harness에서 쓰는 기법 중 지금 프로젝트에 맞는 부분만 가져와 shacs-bot용 평가 하네스를 설계하고, 템플릿을 repo 스타일 spec으로 변환한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`evaluation-harness-mvp.md`](./prds/evaluation-harness-mvp.md) | `process_direct()` 기반 단일 평가 실행, variant 비교, trace artifact 저장을 위한 MVP 설계 |

## TL;DR

> **목적**: meta-harness의 핵심 아이디어 중 shacs-bot에 맞는 부분만 도입하여, 하네스 변경(환경 부트스트랩, 컨텍스트 구성, 완료 판단)을 반복 가능하게 평가하고 비교할 수 있는 최소 평가 하네스를 마련한다.
>
> **Deliverables**:
> - 평가 케이스 정의 포맷
> - `AgentLoop.process_direct()` 기반 단일/배치 평가 실행 경로
> - harness variant 비교 결과와 trace artifact 저장 구조
> - 실패 원인 진단용 실행 요약 포맷
>
> **Estimated Effort**: Medium (4-8시간)

## 현재 상태 분석

### 현재 강점

- `shacs_bot/agent/loop.py`의 `AgentLoop.process_direct()`를 통해 채널을 거치지 않는 직접 실행 경로가 이미 존재한다.
- `shacs_bot/providers/base.py`에는 `chat_with_retry()`와 usage 집계 지점이 있어 평가 메트릭 수집에 재사용 가능하다.
- `shacs_bot/agent/loop.py`에는 tool call 실행 루프와 `ExecutionHealthMonitor`가 있어 trace 수집 포인트가 명확하다.
- `shacs_bot/agent/context.py`는 환경/부트스트랩 파일/메모리/스킬을 시스템 프롬프트에 조합하는 하네스 중심 모듈이다.

### 현재 한계

- 평가 전용 디렉터리, 데이터 포맷, CLI 진입점이 없다.
- 하네스 변경 전후를 같은 입력 세트로 비교하는 표준 절차가 없다.
- 실패 원인 분석에 필요한 trace가 세션 히스토리에 일부 남지만, 평가 artifact로 구조화되어 저장되지는 않는다.
- 프로젝트 전반에 pytest/benchmark 중심의 평가 인프라가 없으므로, meta-harness의 대규모 evolutionary search를 바로 도입하기엔 기반이 부족하다.

## 사용자 시나리오 & 테스트

### User Story 1 - 단일 턴 평가 실행 (P1)

개발자는 현재 shacs-bot 하네스가 특정 입력에 대해 어떻게 동작하는지 반복 가능하게 실행하고 결과를 저장할 수 있어야 한다.

**Why this priority**: 비교 가능한 실행 결과가 없으면 어떤 하네스 개선도 검증할 수 없다.

**Independent Test**: 평가 케이스 1개를 실행하여 최종 응답, tool call, 종료 상태, usage가 artifact로 저장되면 독립적인 가치가 있다.

**Acceptance Scenarios**:

1. **Given** 평가 케이스가 정의되어 있을 때, **When** 개발자가 단일 평가를 실행하면, **Then** 시스템은 결과 요약과 trace artifact를 저장한다.
2. **Given** 동일한 평가 케이스와 동일한 하네스 설정이 있을 때, **When** 평가를 다시 실행하면, **Then** 시스템은 같은 스키마의 결과를 생성한다.

---

### User Story 2 - 하네스 variant 비교 (P2)

개발자는 환경 부트스트랩 on/off, completion-checking 차이 등 두 개 이상의 하네스 variant를 같은 입력 세트로 실행하고 비교할 수 있어야 한다.

**Why this priority**: meta-harness의 실제 가치는 “좋아졌는지 비교 가능한가”에 있다.

**Independent Test**: variant A/B를 같은 케이스 세트에 실행하고 success/failure, 평균 tool call 수, usage를 비교할 수 있으면 독립적인 가치가 있다.

**Acceptance Scenarios**:

1. **Given** 두 개의 하네스 variant가 정의되어 있을 때, **When** 동일한 평가 세트로 실행하면, **Then** 시스템은 variant별 결과를 분리 저장한다.
2. **Given** 한 variant가 더 적은 tool call로 동일한 성공률을 달성했을 때, **When** 요약 결과를 확인하면, **Then** 그 차이가 명시적으로 드러난다.

---

### User Story 3 - 실패 원인 진단 (P3)

개발자는 실패한 평가에서 어떤 tool chain, 종료 이유, provider 오류, 또는 컨텍스트 차이 때문에 실패했는지 재현 없이 확인할 수 있어야 한다.

**Why this priority**: 점수만으로는 하네스 개선 방향을 잡기 어렵고, raw trace 기반 진단이 meta-harness에서 가져올 핵심 가치다.

**Independent Test**: 실패 케이스 하나에서 입력, 응답, tool call 순서, finish reason, 오류 유형을 열람할 수 있으면 독립적인 가치가 있다.

**Acceptance Scenarios**:

1. **Given** 평가 실행이 실패했을 때, **When** 저장된 결과를 열람하면, **Then** 개발자는 입력, 응답, tool call 순서, 종료 이유를 확인할 수 있다.
2. **Given** 두 variant 중 하나만 실패했을 때, **When** 결과를 비교하면, **Then** 실패를 유발한 차이를 추적할 수 있다.

## Edge Cases

- tool을 전혀 호출하지 않고 바로 종료한 케이스를 어떻게 성공/실패로 분류할 것인가?
- provider 장애, timeout, 인증 오류를 작업 실패와 별도 분류할 것인가?
- MCP 도구 구성이나 외부 웹 상태처럼 비결정적 의존성이 있을 때 비교 결과를 어떻게 표시할 것인가?
- OAuth provider처럼 비대화형 평가에 부적합한 provider는 어떻게 제외할 것인가?
- 동일한 입력이라도 시간/네트워크 의존성으로 결과가 달라질 때 rerun 가능성을 어떻게 보장할 것인가?

## Requirements

### Functional Requirements

- **FR-001**: System MUST allow developers to run a single evaluation case against the current shacs-bot harness without going through a chat channel.
- **FR-002**: System MUST record evaluation artifacts including input, final response, tool call sequence, finish reason, and usage metadata.
- **FR-003**: System MUST support running the same evaluation set against multiple named harness variants.
- **FR-004**: System MUST store evaluation results in a deterministic directory structure so past runs can be inspected and compared.
- **FR-005**: System MUST distinguish task failure from infrastructure failure such as provider error, timeout, or configuration error.
- **FR-006**: System MUST support at least one configurable harness dimension related to environment bootstrap, context shaping, or completion-checking.
- **FR-007**: System MUST produce a machine-readable summary per run, including success count, failure count, error count, tool usage count, and token/cost usage where available.
- **FR-008**: System MUST preserve enough trace detail to diagnose why a case failed without requiring manual reproduction.
- **FR-009**: System MUST allow non-deterministic or unsupported providers/tools to be explicitly excluded from evaluation runs.
- **FR-010**: System MUST not change normal production agent behavior unless evaluation mode is explicitly invoked.
- **FR-011**: System MUST allow one failing case to be rerun independently.
- **FR-012**: System MUST support adding new evaluation cases without editing the core agent loop.

### Key Entities

- **EvaluationCase**: 하나의 평가 입력 단위. 입력 프롬프트, 기대 결과 유형, tags, optional constraints를 가진다.
- **HarnessVariant**: 비교 대상 하네스 설정 묶음. 예: bootstrap on/off, completion strategy A/B.
- **EvaluationRun**: 특정 시점에 특정 variant로 실행된 전체 평가 세트.
- **EvaluationResult**: 케이스별 실행 결과. 응답, 성공 여부, 오류 유형, tool call 수, usage, trace 참조를 포함한다.
- **TraceArtifact**: 실행 중 수집된 상세 로그. 메시지 흐름, tool call/result, finish reason, provider metadata를 담는다.
- **RunSummary**: 실행 단위 요약. success/failure/error count, 평균 tool call 수, 토큰/비용 통계를 담는다.

## 설계

### 도입 원칙

1. **부분 도입**: meta-harness의 연구용 대규모 optimizer 전체가 아니라, 평가 실행과 trace 저장만 먼저 도입한다.
2. **하네스 중심**: 개선 대상은 agent capability 전체가 아니라 `context`, `tool definitions`, `completion-checking`, `environment bootstrap`에 한정한다.
3. **재현 가능성 우선**: 첫 단계에서는 단일 케이스 재실행과 variant 비교가 가능해야 한다.
4. **제품 동작 비침범**: 일반 `agent`, `gateway`, `subagent` 경로는 evaluation mode가 아닐 때 기존과 동일해야 한다.

### 아키텍처 개요

```text
Evaluation CLI / runner
    ↓
EvaluationCase + HarnessVariant 로드
    ↓
AgentLoop.process_direct() 실행
    ↓
LLM 응답 / tool call / finish_reason / usage 수집
    ↓
TraceArtifact + EvaluationResult 저장
    ↓
RunSummary 생성 및 variant 비교
```

### 범위

#### Phase 1 (MVP)

- 단일 턴 평가 실행
- variant별 배치 실행
- JSON 기반 결과/요약 저장
- 실패 원인 확인용 trace artifact 저장

#### Phase 2 (후속)

- completion-checking 전략 비교 자동화
- context/window fallback 실험
- semi-automatic proposer workflow

#### Non-Goals

- evolutionary search 자동화
- raw repository 전체를 proposer agent에 노출하는 구조
- benchmark leaderboard 수준의 대규모 실험 관리

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/evals/models.py` | 신규 | EvaluationCase, HarnessVariant, EvaluationResult, RunSummary 모델 |
| `shacs_bot/evals/runner.py` | 신규 | 단일/배치 평가 실행 로직 |
| `shacs_bot/evals/storage.py` | 신규 | 평가 artifact 및 요약 저장 |
| `shacs_bot/cli/commands.py` | 수정 | 평가 실행용 CLI 엔트리 추가 |
| `shacs_bot/agent/loop.py` | 수정 | 평가 trace 수집에 필요한 최소 훅 추가 |
| `shacs_bot/agent/context.py` | 수정 | variant 기반 bootstrap/context 옵션 분기 지점 |
| `docs/specs/evaluation-harness/spec.md` | 신규 | 기능 spec |
| `docs/specs/evaluation-harness/prds/evaluation-harness-mvp.md` | 신규 | MVP PRD |

## 검증 기준

- [ ] 단일 평가 케이스 1개를 실행하여 결과 artifact와 run summary가 생성된다.
- [ ] 동일한 평가 세트를 두 개의 variant로 실행하여 variant별 결과가 분리 저장된다.
- [ ] provider 오류와 task failure가 서로 다른 상태값으로 기록된다.
- [ ] 실패한 케이스에서 최종 응답, tool call 순서, finish reason, usage를 확인할 수 있다.
- [ ] evaluation mode를 사용하지 않을 때 기존 `agent`/`gateway` 경로 동작이 바뀌지 않는다.

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| 비결정적 도구/웹 의존성 때문에 비교 결과가 흔들림 | 중 | 중 | deterministic subset만 먼저 평가 대상에 포함 |
| trace 저장 범위가 과도하여 결과 파일이 비대해짐 | 중 | 중 | MVP에서는 핵심 필드만 저장하고 raw payload는 선택적 저장 |
| 평가 코드가 제품 코드 경로를 오염시킴 | 중 | 상 | evaluation mode 전용 엔트리와 분기 최소화 |
| provider/OAuth 환경 차이로 재현성이 낮아짐 | 상 | 중 | 비대화형 provider 우선, unsupported provider 명시 제외 |

## Success Criteria

### Measurable Outcomes

- **SC-001**: 개발자는 10개 이하의 평가 케이스 세트를 단일 명령으로 실행할 수 있어야 한다.
- **SC-002**: 모든 평가 실행은 케이스별 결과와 실행 요약을 파일로 남겨야 하며 결과 누락률은 0%여야 한다.
- **SC-003**: 실패한 평가 케이스의 원인을 재현 없이 파악할 수 있을 정도의 trace 정보가 저장되어야 한다.
- **SC-004**: 두 개의 harness variant를 동일한 평가 세트에 대해 실행했을 때 성공률, 실패 수, 평균 tool call 수를 비교할 수 있어야 한다.
- **SC-005**: evaluation 기능이 비활성 상태일 때 기존 production agent 동작은 기능적으로 동일해야 한다.
- **SC-006**: 최소 1개의 실제 하네스 개선 실험(예: 환경 부트스트랩 on/off)을 이 프레임워크로 비교할 수 있어야 한다.
