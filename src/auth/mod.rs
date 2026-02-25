// src/auth/mod.rs
pub mod did_exchange;
pub mod types;

pub use did_exchange::{build_auth_message, DidExchangeService, JwtClaims};
pub use types::{AgentContext, AuthError, GlobalPassport, LocalVisa, VisaIssuanceResponse};

use actix_web::{HttpRequest, HttpResponse};
use serde_json::json;

/// POST 엔드포인트 JWT 인증 가드
///
/// Authorization: Bearer <token> 헤더를 읽고
/// 토큰의 local_did(sub) 또는 global_did(gdid)가 claimed_did와 일치하는지 검증.
///
/// 사용법:
/// ```rust
/// pub async fn my_handler(
///     state: web::Data<...>,
///     http_req: HttpRequest,
///     req: web::Json<MyReq>,
/// ) -> impl Responder {
///     if let Err(r) = auth::require_auth(&http_req, &req.agent_did, &state.did_service) {
///         return r;
///     }
///     // 검증 통과 — 비즈니스 로직
/// }
/// ```
pub fn require_auth(
    http_req: &HttpRequest,
    claimed_did: &str,
    service: &DidExchangeService,
) -> Result<JwtClaims, HttpResponse> {
    // Authorization 헤더에서 Bearer 토큰 추출
    let token = http_req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| {
            HttpResponse::Unauthorized().json(json!({
                "error": "missing_token",
                "message": "Authorization: Bearer <token> header required",
                "hint": "Authenticate at POST /auth/exchange to receive your token"
            }))
        })?;

    // JWT 검증
    let claims = service.decode_jwt(token).map_err(|_| {
        HttpResponse::Unauthorized().json(json!({
            "error": "invalid_token",
            "message": "Token expired or invalid. Re-authenticate at POST /auth/exchange"
        }))
    })?;

    // DID 일치 확인: sub(local_did) 또는 gdid(global_did) 중 하나와 일치해야 함
    if claims.sub != claimed_did && claims.gdid != claimed_did {
        return Err(HttpResponse::Forbidden().json(json!({
            "error": "did_mismatch",
            "message": "Token DID does not match requested agent_did",
            "token_local_did": claims.sub,
            "requested_did": claimed_did,
        })));
    }

    Ok(claims)
}
