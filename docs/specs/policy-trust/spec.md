# SPEC: Policy / Approval / Trust Model

> **Prompt**: workspace-level approval gate를 assistant 제품 수준의 user/channel/action-aware policy 및 trust model로 확장한다.

## PRDs

| PRD | 설명 |
|---|---|
| [`policy-trust.md`](./prds/policy-trust.md) | ApprovalGate 확장 + usage-aware quota + channel/user trust 규칙 |

## TL;DR

> **목적**: 멀티채널 assistant에서 도구 호출, background 작업, 고비용 모델 사용을 사용자/채널/행위 단위로 통제한다.
>
> **Deliverables**:
> - `agent/policy.py` — trust/policy evaluator
> - `agent/approval.py` — manual/auto approval와 policy 통합
> - `agent/usage.py` — quota 판단 입력 제공
> - `config/schema.py` — user/channel/action 정책 설정 추가
>
> **Estimated Effort**: Medium (4-6시간)

## 현재 상태 분석

- `agent/approval.py`는 읽기 도구 즉시 허용, 위험 exec 패턴 즉시 거부, workspace 내 파일 쓰기 허용, 그 외 LLM 분류기/수동 승인 구조를 제공한다.
- 이 모델은 **workspace skill** 중심으로는 유효하지만, assistant 제품 관점에서는 다음이 빠져 있다.

1. 특정 채널에서는 어떤 도구를 허용할지
2. 신규/저신뢰 사용자에게 어떤 제한을 둘지
3. usage/cost가 일정 한도를 넘으면 어떤 degrade를 할지
4. background workflow / planner 실행에도 같은 정책을 적용할지

## 설계

### 정책 계층

```text
Tier 0: hard deny (파괴 명령, 외부 유출 위험)
Tier 1: always allow (읽기/조회)
Tier 2: trust rule (user/channel/action/profile)
Tier 3: quota rule (daily cost / tool budget / model budget)
Tier 4: approval rule (auto/manual)
Tier 5: reasoning-blind LLM classifier
```

### 정책 입력

- `actor`: sender_id, session_key, channel
- `action`: tool_name, planner step type, workflow operation
- `budget`: session/day cost, model tier
- `trust`: allow_from, explicit trust, privileged role

### Config 방향

```python
class PolicyConfig(Base):
    default_mode: Literal["auto", "manual", "off"] = "auto"
    trusted_users: list[str] = Field(default_factory=list)
    trusted_channels: list[str] = Field(default_factory=list)
    daily_cost_limit_usd: float = 0.0
    high_risk_tools: list[str] = Field(default_factory=list)
```

### 동작 예시

- Slack shared channel + 신규 사용자 + exec 요청 → manual approval
- trusted Telegram DM + read/search tool → auto allow
- 일일 quota 초과 + 고비용 모델 → lower-cost model fallback 또는 거부
- background workflow가 외부 전송 단계에 도달 → same policy evaluator 재사용

## 파일 변경 목록

| 파일 | 변경 | 설명 |
|------|:---:|------|
| `shacs_bot/agent/policy.py` | 신규 | actor/channel/action-aware policy evaluator |
| `shacs_bot/agent/approval.py` | 수정 | PolicyEvaluator 연동 |
| `shacs_bot/agent/usage.py` | 수정 | quota 판단용 summary helper 확장 |
| `shacs_bot/config/schema.py` | 수정 | PolicyConfig 추가 |
| `shacs_bot/agent/loop.py` | 수정 | tool/planner/workflow 실행 전 policy 적용 |

## 검증 기준

- [ ] 기존 auto/manual/off skill approval 모드가 유지됨
- [ ] channel/user trust rule이 실제 승인 결정에 반영됨
- [ ] quota 초과 시 degrade 또는 차단이 일관되게 동작함
- [ ] workflow/planner 단계에도 동일 정책이 적용됨

## Must NOT

- 정책 판단을 특정 채널 구현 내부에 흩뿌리지 않는다.
- usage enforcement 때문에 기존 usage 기록 경로가 깨지지 않는다.
