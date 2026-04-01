# PRD: Evaluation Harness v1

> **배경**: meta-harness의 핵심 가치는 “하네스 변경이 실제로 개선인지 raw execution trace 기반으로 비교 가능하게 만드는 것”이다. shacs-bot은 `AgentLoop.process_direct()`라는 좋은 실행 경계를 이미 갖고 있으므로, 연구용 optimizer 전체 대신 **반복 가능 실행 + variant 비교 + trace artifact 저장**을 먼저 구현한다.
>
> **v0 대비 변경**: 기존 MVP 초안 수준을 넘어, 실제 구현 가능한 파일 구조 / 데이터 모델 / 메서드 시그니처 / 상태 분류 / 저장 포맷 / CLI 흐름까지 구체화한다.
>
> **참고**: `docs/specs/model-failover/spec.md`, `docs/specs/skill-isolation/prds/skill-isolation.md`, meta-harness 분석 결과.

---

## 문제

현재 shacs-bot은 하네스 변경 효과를 반복 가능하게 측정하는 공식 경로가 없다.

1. **단일 실행만 존재**: `AgentLoop.process_direct()`는 있지만, 평가 데이터 모델/저장 규약/비교 요약이 없다.
2. **trace가 평가 artifact가 아님**: 세션 히스토리와 로그는 존재하지만, 케이스 단위 결과/실패 원인 분석을 위한 구조화된 산출물이 없다.
3. **하네스 실험 표준 부재**: 환경 부트스트랩, context profile, completion policy 변경을 같은 입력 세트에 반복 실행하는 공식 절차가 없다.
4. **제품 코드와 실험 코드 경계 부재**: 평가를 위해 임시 스크립트를 만들면 재현성과 누적 관리가 깨진다.

결과적으로 다음 질문에 답하기 어렵다.

- 환경 부트스트랩을 넣으면 실제로 tool 탐색 낭비가 줄어드는가?
- context profile을 줄이면 성공률이 떨어지는가, 아니면 token만 절감되는가?
- 실패한 실행은 task failure인가, provider failure인가?
- variant A/B 중 무엇이 더 안정적인가?

---

## 해결책

### 최소 평가 하네스 도입

`AgentLoop.process_direct()`를 공통 실행 경계로 사용해, 다음을 제공하는 평가 모듈을 추가한다.

1. **평가 케이스 로딩** — JSON 파일에서 입력 케이스를 읽는다.
2. **variant 적용** — 환경 부트스트랩, context profile, completion policy를 variant로 주입한다.
3. **실행 + trace 수집** — 기존 agent loop를 사용하되 평가 mode에서 observer를 통해 trace를 모은다.
4. **결과 저장** — 케이스별 result/trace와 run summary를 저장한다.
5. **variant 비교** — 동일한 케이스 세트를 여러 variant에 돌리고 요약 비교를 만든다.

### 핵심 원칙

1. **제품 경로 재사용**: agent를 새로 구현하지 않고 기존 `AgentLoop`를 사용한다.
2. **비침범성**: evaluation mode가 아닐 때 제품 동작은 바뀌지 않는다.
3. **trace 우선**: 점수보다 먼저 실패 원인을 볼 수 있어야 한다.
4. **단일 턴 우선**: v1은 multi-turn benchmark가 아니라 single-turn harness evaluation에 집중한다.
5. **하네스 축만 변경**: provider routing/failover/MCP topology 자체는 variant 축에서 제외한다.

---

## 실행 흐름

```text
eval run cases.json --variant default --variant bootstrap-off
  → CLI가 EvaluationRunConfig 생성
  → EvaluationRunner가 케이스 로드
  → variant별로 AgentLoop를 평가 모드로 실행
      → ContextBuilder가 variant에 따라 system/user context 구성
      → AgentLoop._run_agent_loop()가 observer에 tool/response 이벤트 전달
      → EvaluationRunner가 Result/Trace를 조립
  → EvaluationStorage가 result.json / trace.json / summary.json 저장
  → CLI가 run 경로와 비교 요약 출력
```

### 상태 분류 흐름

```text
LLM 실행 완료
  → finish_reason == error ?
      → infra_error
  → timeout / provider exception / config error ?
      → infra_error
  → 응답은 왔지만 기대 조건 미충족 ?
      → task_failure
  → 그 외
      → success
```

---

## 사용자 영향

| Before | After |
|---|---|
| 하네스 실험은 수동 메모/감으로 비교 | 케이스/variant 단위로 반복 실행 후 결과 저장 |
| 실패 원인 분석에 세션 로그와 콘솔 로그를 뒤져야 함 | result/trace artifact에서 바로 확인 가능 |
| 동일 입력 재실행 규약 없음 | case_id 기준 단일 rerun 가능 |
| 하네스 변경의 비용/효과 비교 어려움 | success/failure/tool usage/token usage 비교 가능 |

---

## 기술적 범위

### 신규 파일

| 파일 | 역할 | 규모 |
|---|---|---|
| `shacs_bot/evals/models.py` | 평가 데이터 모델 정의 | ~150줄 |
| `shacs_bot/evals/runner.py` | 케이스/variant 실행 orchestration | ~220줄 |
| `shacs_bot/evals/storage.py` | 결과/trace/summary 저장 | ~120줄 |
| `shacs_bot/evals/__init__.py` | package export | ~5줄 |

### 수정 파일

| 파일 | 변경 내용 | 규모 |
|---|---|---|
| `shacs_bot/cli/commands.py` | `eval run` 명령 추가 | ~60줄 |
| `shacs_bot/agent/loop.py` | evaluation observer/collector 훅 추가 | ~90줄 |
| `shacs_bot/agent/context.py` | variant 기반 bootstrap/context 분기 | ~50줄 |
| `shacs_bot/config/schema.py` | 선택적 eval config 추가 여부 검토 (v1은 생략 가능) | ~0~20줄 |

### 총 변경량

약 **500~700줄** 수준 예상. 문서/예제 케이스 제외.

---

## 설계

### 1. 평가 데이터 모델 (`shacs_bot/evals/models.py`)

Pydantic 모델로 정의한다. JSON 입출력과 스키마 안정성을 위해 dataclass 대신 BaseModel을 사용한다.

```python
from pydantic import BaseModel, Field
from typing import Any, Literal


EvalStatus = Literal["success", "task_failure", "infra_error"]
ExpectedMode = Literal["response", "tool_use", "failure_expected"]


class EvaluationCase(BaseModel):
    case_id: str
    input: str
    expected_mode: ExpectedMode = "response"
    tags: list[str] = Field(default_factory=list)
    timeout_seconds: int = 120
    notes: str = ""


class HarnessVariant(BaseModel):
    name: str
    environment_bootstrap: bool = True
    context_profile: str = "default"      # default | minimal
    completion_policy: str = "default"    # default | strict


class ToolEvent(BaseModel):
    name: str
    arguments: dict[str, Any] = Field(default_factory=dict)
    result_preview: str = ""
    is_error: bool = False


class TraceArtifact(BaseModel):
    model: str = ""
    provider: str = ""
    finish_reason: str = ""
    assistant_response: str = ""
    tool_events: list[ToolEvent] = Field(default_factory=list)
    usage: dict[str, int] = Field(default_factory=dict)
    started_at: str = ""
    completed_at: str = ""


class EvaluationResult(BaseModel):
    case_id: str
    variant: str
    status: EvalStatus
    final_response: str = ""
    finish_reason: str = ""
    tool_call_count: int = 0
    usage: dict[str, int] = Field(default_factory=dict)
    trace_path: str = ""
    error_message: str = ""


class VariantSummary(BaseModel):
    variant: str
    total: int = 0
    success: int = 0
    task_failure: int = 0
    infra_error: int = 0
    avg_tool_calls: float = 0.0
    prompt_tokens: int = 0
    completion_tokens: int = 0


class RunSummary(BaseModel):
    run_id: str
    variants: list[VariantSummary] = Field(default_factory=list)
```

#### 모델 원칙

- `EvaluationCase`는 단일 입력만 가진다. multi-turn transcript는 v2로 미룬다.
- `expected_mode`는 semantic grading을 대체하는 최소 기대 유형이다.
- `TraceArtifact`는 raw 전체 payload 대신 진단에 필요한 핵심 필드만 담는다.
- `EvaluationResult`는 CLI 출력과 summary 집계를 위해 compact하게 유지한다.

### 2. 입력 파일 포맷

v1에서는 JSON만 지원한다. YAML까지 열지 않는다.

```json
{
  "cases": [
    {
      "caseId": "bootstrap-basic-001",
      "input": "현재 워크스페이스의 핵심 파일을 파악해 요약해줘",
      "expectedMode": "response",
      "tags": ["bootstrap", "context"]
    },
    {
      "caseId": "tool-use-001",
      "input": "README를 읽고 설치 방법을 요약해줘",
      "expectedMode": "tool_use",
      "tags": ["readme", "tool"]
    }
  ]
}
```

#### 왜 JSON만?

- 현재 프로젝트 설정과 잘 맞는다.
- camelCase ↔ snake_case 규칙을 기존 config 관례와 맞출 수 있다.
- 파서 추가 복잡도를 줄인다.

### 3. CLI (`shacs_bot/cli/commands.py`)

Typer 하위 그룹으로 추가한다.

```python
eval_app = typer.Typer(help="Evaluation harness commands")
app.add_typer(eval_app, name="eval")


@eval_app.command("run")
def eval_run(
    cases: Path,
    variant: list[str] = typer.Option(None, "--variant"),
    case_id: str | None = typer.Option(None, "--case"),
    output: Path | None = typer.Option(None, "--output"),
):
    ...
```

#### CLI 동작

1. config/provider/workspace를 기존 `agent` command와 동일한 방식으로 초기화
2. `cases.json` 로드
3. `--variant`가 없으면 `default` 1개 자동 생성
4. `--case`가 있으면 해당 case만 필터
5. runner 실행
6. summary path와 variant 요약을 콘솔 출력

#### variant 입력 방식

v1에서는 `--variant`를 **이름 기반 preset**으로 제한한다.

지원 preset:

- `default`
- `bootstrap-off`
- `minimal-context`
- `strict-completion`

이 preset은 `runner.py` 내부의 간단한 factory로 해석한다.

```python
def resolve_variant(name: str) -> HarnessVariant:
    presets = {
        "default": HarnessVariant(name="default"),
        "bootstrap-off": HarnessVariant(name="bootstrap-off", environment_bootstrap=False),
        "minimal-context": HarnessVariant(name="minimal-context", context_profile="minimal"),
        "strict-completion": HarnessVariant(name="strict-completion", completion_policy="strict"),
    }
    return presets[name]
```

### 4. 실행 orchestration (`shacs_bot/evals/runner.py`)

핵심 클래스:

```python
class EvaluationRunner:
    def __init__(self, agent_loop: AgentLoop, storage: EvaluationStorage):
        self._agent_loop = agent_loop
        self._storage = storage

    async def run_cases(
        self,
        cases: list[EvaluationCase],
        variants: list[HarnessVariant],
        output_dir: Path,
    ) -> RunSummary:
        ...

    async def _run_case(
        self,
        case: EvaluationCase,
        variant: HarnessVariant,
        run_dir: Path,
    ) -> EvaluationResult:
        ...
```

#### `_run_case()` 세부 흐름

```text
case + variant 시작
  → trace collector 생성
  → agent_loop에 evaluation context 주입
  → asyncio.wait_for(process_direct(...), timeout=case.timeout_seconds)
  → trace collector에서 finish_reason/tool events/usage 획득
  → status 분류
  → storage에 trace/result 저장
  → EvaluationResult 반환
```

#### timeout 처리

```python
try:
    response = await asyncio.wait_for(
        self._agent_loop.process_direct(...),
        timeout=case.timeout_seconds,
    )
except asyncio.TimeoutError:
    status = "infra_error"
    error_message = f"timeout after {case.timeout_seconds}s"
```

v1에서는 timeout을 `infra_error`로 분류한다. 과업 실패와 분리해야 비교가 된다.

### 5. AgentLoop observer 훅 (`shacs_bot/agent/loop.py`)

기존 `_run_agent_loop()`에 선택적 observer를 추가한다.

```python
class AgentLoopObserver(Protocol):
    def on_llm_response(self, response: LLMResponse) -> None: ...
    def on_tool_result(self, tool_name: str, arguments: dict[str, Any], result: str) -> None: ...
    def on_final(self, final_content: str | None, finish_reason: str) -> None: ...
```

`_run_agent_loop()` 시그니처를 아래처럼 확장한다.

```python
async def _run_agent_loop(
    self,
    init_messages: list[dict[str, Any]],
    on_progress: Callable[..., Awaitable[None]] | None = None,
    observer: AgentLoopObserver | None = None,
) -> tuple[str | None, list[str], list[dict[str, Any]], TurnUsage]:
```

#### observer 호출 지점

1. `chat_with_retry()` 직후 `observer.on_llm_response(response)`
2. tool 실행 직후 `observer.on_tool_result(tool_name, args, result)`
3. 종료 직전 `observer.on_final(final_content, response.finish_reason)`

#### 주의사항

- observer는 evaluation mode 전용이며, `None`일 때 기존 경로와 완전히 동일해야 한다.
- observer에서 예외가 나더라도 agent 실행을 깨뜨리면 안 된다. 내부적으로 swallow + logger.warning 처리한다.

### 6. 평가용 trace collector (`runner.py` 내부)

간단한 collector 클래스를 둔다.

```python
class TraceCollector:
    def __init__(self, model: str, provider: str):
        self._finish_reason = ""
        self._response = ""
        self._tool_events: list[ToolEvent] = []
        self._usage: dict[str, int] = {}

    def on_llm_response(self, response: LLMResponse) -> None:
        self._finish_reason = response.finish_reason
        self._response = response.content or ""
        self._usage = dict(response.usage or {})

    def on_tool_result(self, tool_name: str, arguments: dict[str, Any], result: str) -> None:
        self._tool_events.append(...)
```

`result_preview`는 최대 200자만 저장한다. raw 전체 result는 v1에서 저장하지 않는다.

### 7. Context variant 적용 (`shacs_bot/agent/context.py`)

`ContextBuilder` 전체를 복제하지 않는다. 대신 variant 전용 옵션 객체를 추가한다.

```python
@dataclass(frozen=True)
class ContextVariant:
    environment_bootstrap: bool = True
    context_profile: str = "default"
    completion_policy: str = "default"
```

`build_system_prompt()`와 `build_runtime_context()`에서 최소 분기만 둔다.

#### `environment_bootstrap=False`

- `_get_identity()`의 워크스페이스 상세 안내 일부를 줄인다.
- bootstrap files (`AGENTS.md`, `SOUL.md`, ...) 로드는 유지하되, 런타임 안내 텍스트를 최소화할지 여부를 옵션화한다.

#### `context_profile="minimal"`

- 메모리/skills summary 중 필수 아닌 섹션을 생략 가능하게 한다.
- 단, 시스템 prompt의 안전/정체성 핵심은 제거하지 않는다.

#### `completion_policy="strict"`

- v1에서는 실제 completion-checking engine을 새로 만들지 않는다.
- 대신 strict mode일 때 system prompt에 “완료 전 검증/불확실성 명시” 안내를 추가하는 수준으로 제한한다.

이유: v1 범위를 넘어서는 completion-checking engine 추가는 설계/검증 비용이 크다.

### 8. 결과 저장 (`shacs_bot/evals/storage.py`)

```python
class EvaluationStorage:
    def create_run_dir(self, base_dir: Path | None = None) -> Path: ...
    def write_result(self, run_dir: Path, variant: str, result: EvaluationResult) -> Path: ...
    def write_trace(self, run_dir: Path, variant: str, case_id: str, trace: TraceArtifact) -> Path: ...
    def write_summary(self, run_dir: Path, summary: RunSummary) -> Path: ...
```

#### 디렉터리 구조

```text
<workspace>/evals/
  runs/
    2026-04-01-15-10-30/
      manifest.json
      summary.json
      default/
        bootstrap-basic-001.result.json
        bootstrap-basic-001.trace.json
      bootstrap-off/
        bootstrap-basic-001.result.json
        bootstrap-basic-001.trace.json
```

#### `manifest.json`

run 메타데이터 저장:

```json
{
  "runId": "2026-04-01-15-10-30",
  "casesFile": "/abs/path/to/cases.json",
  "variants": ["default", "bootstrap-off"],
  "createdAt": "2026-04-01T15:10:30+09:00"
}
```

### 9. 결과 상태 분류 규칙

#### `success`

- provider/timeout/config 오류가 없음
- `expected_mode == response` 이고 final response가 비어 있지 않음
- 또는 `expected_mode == tool_use` 이고 tool event가 1개 이상 존재함

#### `task_failure`

- 실행은 끝났지만 기대 조건 미충족
- 예: response expected인데 빈 응답
- 예: tool_use expected인데 tool 호출 없이 종료

#### `infra_error`

- `finish_reason == "error"`
- timeout
- provider exception
- 입력 파일 파싱 오류
- variant preset 해석 실패

#### 왜 semantic grading이 없는가?

v1 목표는 “비교 가능한 실행 trace”를 만드는 것이지, 정답 채점 시스템을 만드는 것이 아니다. semantic grading은 v2에서 LLM judge나 규칙 기반 judge로 추가한다.

### 10. summary 집계 규칙

variant별로 다음을 집계한다.

- total
- success
- task_failure
- infra_error
- avg_tool_calls
- prompt_tokens 합계
- completion_tokens 합계

집계 함수 예시:

```python
def build_variant_summary(variant: str, results: list[EvaluationResult]) -> VariantSummary:
    total = len(results)
    success = sum(1 for r in results if r.status == "success")
    task_failure = sum(1 for r in results if r.status == "task_failure")
    infra_error = sum(1 for r in results if r.status == "infra_error")
    avg_tool_calls = sum(r.tool_call_count for r in results) / total if total else 0.0
    ...
```

---

## 구현 순서

### Step 1 — 데이터 모델/저장소 추가

1. `shacs_bot/evals/models.py`
2. `shacs_bot/evals/storage.py`
3. run directory / manifest / result / trace / summary 쓰기 구현

### Step 2 — AgentLoop observer 훅 추가

1. observer protocol 추가
2. `_run_agent_loop()`에서 observer 호출
3. observer가 `None`일 때 기존 동작 동일 확인

### Step 3 — variant/context 연결

1. `ContextVariant` 추가
2. `ContextBuilder`에 variant 옵션 전달
3. `default`, `bootstrap-off`, `minimal-context`, `strict-completion` preset 구현

### Step 4 — runner 구현

1. cases.json 로드
2. case filter / variant 반복
3. timeout / status classification
4. result/trace 저장
5. summary 생성

### Step 5 — CLI 연결

1. `eval` sub-app 추가
2. `eval run` command 추가
3. 콘솔 결과 출력

### Step 6 — 예제 케이스 수동 검증

1. response expected 케이스
2. tool_use expected 케이스
3. timeout/infra_error 케이스

---

## 파일별 변경 상세

### `shacs_bot/evals/models.py`

- Pydantic 모델만 둔다.
- 비즈니스 로직 넣지 않는다.

### `shacs_bot/evals/storage.py`

- JSON 직렬화 전용
- 파일명 안전화(sanitize) 처리
- 디렉터리 자동 생성

### `shacs_bot/evals/runner.py`

- `EvaluationRunner`
- `TraceCollector`
- `resolve_variant()`
- `load_cases()`
- `classify_result()`

### `shacs_bot/agent/loop.py`

- observer optional arg 추가
- response/tool/final 지점에서 observer 호출
- exception swallowing with warning

### `shacs_bot/agent/context.py`

- `ContextVariant` 또는 동등한 옵션 객체 추가
- 최소 분기 구현
- 시스템 prompt 안전 핵심은 절대 제거하지 않음

### `shacs_bot/cli/commands.py`

- `eval_app = typer.Typer(...)`
- `eval run` 구현
- `uv run shacs-bot eval run cases.json --variant default` 사용 가능하게 함

---

## 검증 기준

- [ ] `uv run shacs-bot eval run cases.json` 으로 default variant 단일 실행이 된다.
- [ ] `--variant default --variant bootstrap-off` 로 두 variant 결과가 분리 저장된다.
- [ ] result 파일에 `status`, `final_response`, `finish_reason`, `tool_call_count`, `usage`, `trace_path`가 존재한다.
- [ ] trace 파일에 `tool_events`, `assistant_response`, `finish_reason`, `provider`, `model`이 존재한다.
- [ ] `expected_mode=tool_use` 케이스에서 tool call이 없으면 `task_failure`가 된다.
- [ ] provider 오류/timeout은 `infra_error`로 기록된다.
- [ ] evaluation mode를 사용하지 않을 때 기존 `agent`/`gateway`/`process_direct()` 동작이 변하지 않는다.

---

## 위험 및 완화

| 위험 | 가능성 | 영향 | 완화 |
|---|---|---|---|
| variant 분기가 context 코드를 복잡하게 만듦 | 중 | 상 | v1은 preset 4개만 허용, 축 추가 금지 |
| trace가 너무 커짐 | 중 | 중 | preview만 저장, raw payload 저장 안 함 |
| semantic grading이 없어 “정답성” 판단이 약함 | 상 | 중 | v1 목표를 trace/비교 인프라로 한정, v2에서 judge 추가 |
| provider/network 비결정성으로 결과가 흔들림 | 상 | 중 | deterministic subset 중심 케이스 구성, unsupported provider 제외 |
| 제품 경로와 평가 경로가 어긋남 | 중 | 상 | `AgentLoop.process_direct()` 재사용, 별도 agent 구현 금지 |

---

## 후속 작업 (v2 이상)

1. semantic grading (`expected_answer`, regex matcher, LLM judge)
2. multi-turn transcript evaluation
3. completion-checking 전용 정책 엔진
4. 자동 variant search 또는 semi-automatic proposer workflow
5. HTML/Markdown report 생성
