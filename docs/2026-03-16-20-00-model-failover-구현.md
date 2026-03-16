# Model Failover 구현 — 작업 기록

> **프롬프트**: `D Next` — PRD 분석 후 다음 우선순위 작업 진행. Model Failover PRD 선택 → M1~M3 구현.

## 변경 파일

- `shacs_bot/config/schema.py` — `FailoverRule`, `FailoverConfig` 스키마 추가, `Config`에 `failover` 필드
- `shacs_bot/providers/failover.py` — 신규 모듈. `FailoverManager` 클래스 (circuit breaker, chain 순회, try_failover)
- `shacs_bot/providers/base.py` — `chat_with_retry`에 `failover_manager`, `provider_name` 파라미터 추가. 3회 재시도 실패 후 failover 시도 삽입
- `shacs_bot/agent/loop.py` — `failover_manager`, `provider_name` 파라미터 추가. `_run_agent_loop`에서 `chat_with_retry`에 전달
- `shacs_bot/cli/commands.py` — gateway/chat 두 진입점에서 `FailoverManager` 생성 및 `AgentLoop`에 전달

## 설계 결정

- `failover.enabled=false`(기본값)이면 기존 동작과 100% 동일 — FailoverManager가 None으로 전달되어 failover 코드 경로 진입 안 함
- circuit breaker 패턴: `time.monotonic()` 기반 쿨다운. 장애 프로바이더를 `cooldown_seconds` 동안 비활성화 후 자동 복구
- failover chain: `from_provider` → `to_provider` 규칙을 순차적으로 따라감. 순환 방지 (visited set)
- `model_map`으로 프로바이더 간 모델명 매핑 지원 (없으면 원래 모델명 그대로 사용)

## 미완료

- M4: 런타임 동작 검증 (잘못된 API 키로 강제 장애 → failover 전환 확인)
