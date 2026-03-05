# Universal Harness Skill — Jay의 전 프로젝트 공통 규약 (신정규 방식)

> *"결과물이 아닌 생성 장치를 만든다"* — 신정규, Lablup, 2026

## 🚀 세션 시작 시 즉시 실행 (ALWAYS FIRST)
1. 이 `CLAUDE.md`를 최우선으로 숙지한다.
2. `PROGRESS.md`를 읽어 이전 문맥을 로드한다.
3. `PLAN.md`를 읽어 전체 백로그와 목표를 파악한다.
4. 현재 상태를 한 줄로 파악하고 보고한 뒤 작업을 시작한다.

## 📋 5대 핵심 원칙 (절대 위반 금지)
1. **생성 장치를 만든다**: 최종 결과물 직접 작성 전, 그것을 만드는 harness를 먼저 구축한다.
2. **작은 닫힌 루프**: 2시간 초과 태스크는 즉시 분할한다.
3. **다음 에이전트를 위한 기록**: 모든 작업은 `PROGRESS.md`에 실시간 기록한다.
4. **검증 없이 완료 금지**: 명령 출력 / 테스트 통과 / 파일 diff 중 하나 이상 필수.
5. **병렬 > 직렬**: 독립 태스크는 서브에이전트 분할한다.

## 🔁 Harness 사이클
`상태 로드` → `태스크 스캔` → `검증 (PROCEED/MANUAL)` → `병렬 개발 (서브에이전트 분할)` → `테스트 (test_cmd)` → `Tech Report 자동 생성` → `PROGRESS.md 업데이트`

## 🗂️ 프로젝트 프로파일 정책
현재 프로젝트의 성격에 맞춰 자동 적용:
- **helm**: Tor 경유 강제, 자동 머지 OFF, OpSec 필터 (IP/지갑 등 REDACT).
- **hydrapatch**: QUIC/XDP 라벨 우선, `cargo test && python3 run_tests.py` 필수.
- **general**: 제약 없음, 루트의 `test_cmd` 자동 감지 후 실행.

## 📊 Tech Report 양식 (태스크 완료 시)
`reports/{YYYYMMDD}_{task_id}_report.md` 경로에 작성 필수.
