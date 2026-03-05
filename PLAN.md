# PLAN

> **목적:** 현재 프로젝트의 백로그 및 개발 목표 리스트. 에이전트는 세션 시작 시 이 파일을 스캔하여 태스크를 할당받음.

## 🎯 장기 목표 (Phase X)
- Gandiva-QUIC Architecture 기반의 0-RTT Sovereign Gateway 구축.
- Kaleidoscope (Zero-Panic) 완결성 확보.

## 📝 백로그 (우선순위 순)
- [ ] (작업 전) `transport.rs:114` 테스트 코드 `Result` 처리 수정 - 예상 소요시간: 5분
- [ ] (작업 전) `build_secure_transport` 상위 호출부 (`main.rs` 등) `Result` 처리 - 예상 소요시간: 15분
- [ ] (작업 전) Gandiva-QUIC 0-RTT (Sliver-Shot) 로직 `gateway` 이식 - 예상 소요시간: 1시간
- [ ] (작업 전) `kaleidoscope.rs` 정책 기반 동적 우선순위 라우팅 구현 - 예상 소요시간: 2시간
- [ ] (작업 전) 런칭 스트레스 테스트 및 모니터링 훅업 - 예상 소요시간: 1시간

## 🔄 병렬 에이전트 할당 계획
- Agent-PHz: [ ] 0-RTT 및 Zero-Panic 강화 (Current)
- Agent-Hz: [ ] Moltbook 및 Telegram 봇 활성화 대기
