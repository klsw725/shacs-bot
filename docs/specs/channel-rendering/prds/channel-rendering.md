# PRD: Channel-aware Rendering Layer

## 목표

assistant의 응답 의미 구조를 canonical form으로 유지하면서, 최종 표현만 채널별 renderer가 담당하게 만든다.

## Deliverables

1. Outbound render metadata 확장
2. 공통 renderer 인터페이스
3. Slack/Discord/Telegram/Email 우선 적용
4. plain-text fallback 보장

## 비목표

- 블록 UI 시스템 전면 도입
- 채널별 독립 프롬프트 시스템

## Acceptance Criteria

- 같은 응답 의도가 여러 채널에서 각각 자연스럽게 표현됨
- 기존 문자열 전송 경로는 유지됨
- thread/reply/hint/footer 규칙이 renderer로 이동함
