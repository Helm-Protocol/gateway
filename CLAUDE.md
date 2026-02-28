# Helm Protocol — Claude Code 작업 규칙

## 자동 트리거: 워크플로우 다이어그램

**사용자가 "워크플로우" 키워드를 언급하면 다음을 자동 실행:**

1. `/home/user/Helm/워크플로우 다이어그램.md` 파일을 열어 최신 취약점 현황 확인
2. 변경된 소스 파일 스캔 (`crates/helm-node/src/gateway/**`)
3. 취약점 패치 현황 업데이트 (완료된 항목 체크)
4. 새 취약점 발견 시 목록에 추가
5. 업데이트된 파일을 사용자에게 렌더링해서 보여주기

## 보안 패치 원칙

- **커밋 전 반드시 `cargo test --workspace`로 시뮬레이션 테스트**
- CRITICAL 취약점은 즉시 패치, HIGH는 다음 커밋, MEDIUM/LOW는 배치 처리
- 패치 시 반드시 `워크플로우 다이어그램.md`의 해당 항목을 ✅로 업데이트

## 개발 브랜치

- 항상 `claude/update-github-oauth-strategy-wT1JP` 브랜치에서 작업
- 커밋 메시지: `security: <패치 내용 요약>`

## 배포 필수 환경변수

```
HELM_ADMIN_SECRET=<64바이트 랜덤 hex>
HELM_CORS_ORIGINS=https://your-frontend.com
HELM_PORT=8080
```
