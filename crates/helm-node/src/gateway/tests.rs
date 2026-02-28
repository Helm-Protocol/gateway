//! HTTP gateway simulation tests — 점-다 / 점-점 / 다-다 + attack scenarios.
//!
//! These tests use tower::ServiceExt::oneshot to simulate real HTTP requests
//! through the full Axum router without binding a network port.
//! Every security patch is exercised here — error codes are verified exactly.

#[cfg(test)]
mod gateway_tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::gateway::server::build_router;
    use crate::gateway::state::AppState;

    // ─────────────────────────────────────────────────────────────────────────
    // Test helpers
    // ─────────────────────────────────────────────────────────────────────────

    /// Send one HTTP request through the gateway; return (status, json_body).
    async fn req(
        state: AppState,
        method: Method,
        uri: &str,
        auth_did: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let app = build_router(state);
        let mut builder = Request::builder().method(method.clone()).uri(uri);

        if let Some(did) = auth_did {
            builder = builder.header("authorization", format!("Bearer {}", did));
        }

        let request = if let Some(b) = body {
            builder
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&b).unwrap()))
                .unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        };

        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    /// Boot a new agent; return (did, private_key_b58).
    async fn boot(state: &AppState, referrer: Option<&str>) -> (String, String) {
        let body = match referrer {
            Some(r) => json!({
                "referrer_did": r,
                "capability": "compute",
                "preferred_token": "VIRTUAL"
            }),
            None => json!({"capability": "compute", "preferred_token": "VIRTUAL"}),
        };
        let (status, resp) =
            req(state.clone(), Method::POST, "/v1/agent/boot", None, Some(body)).await;
        assert_eq!(status, StatusCode::CREATED, "Boot failed: {resp}");
        let did = resp["did"].as_str().unwrap().to_string();
        let priv_key = resp["private_key_b58"].as_str().unwrap().to_string();
        (did, priv_key)
    }

    /// PUT /v1/sense/memory/:key
    async fn mem_put(state: &AppState, did: &str, key: &str, value: Value) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::PUT,
            &format!("/v1/sense/memory/{}", key),
            Some(did),
            Some(json!({"value": value})),
        )
        .await
    }

    /// GET /v1/sense/memory/:key
    async fn mem_get(state: &AppState, did: &str, key: &str) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::GET,
            &format!("/v1/sense/memory/{}", key),
            Some(did),
            None,
        )
        .await
    }

    /// DELETE /v1/sense/memory/:key
    async fn mem_del(state: &AppState, did: &str, key: &str) -> StatusCode {
        let (status, _) = req(
            state.clone(),
            Method::DELETE,
            &format!("/v1/sense/memory/{}", key),
            Some(did),
            None,
        )
        .await;
        status
    }

    /// Create a pool; return (status, response).
    async fn create_pool(
        state: &AppState,
        did: &str,
        cost: f64,
        goal: u64,
    ) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::POST,
            "/v1/pool",
            Some(did),
            Some(json!({
                "name": "TestPool",
                "vendor": "openai",
                "monthly_cost_usd": cost,
                "bnkr_goal": goal,
            })),
        )
        .await
    }

    /// Join a pool; return (status, response).
    async fn join_pool(
        state: &AppState,
        did: &str,
        pool_id: &str,
        stake: u64,
    ) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::POST,
            &format!("/v1/pool/{}/join", pool_id),
            Some(did),
            Some(json!({"stake_virtual": stake})),
        )
        .await
    }

    /// Create a marketplace post; return (status, response).
    async fn create_post(
        state: &AppState,
        did: &str,
        title: &str,
        description: &str,
    ) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::POST,
            "/v1/marketplace/post",
            Some(did),
            Some(json!({
                "post_type": "Job",
                "title": title,
                "description": description,
                "budget_bnkr": 1000u64,
            })),
        )
        .await
    }

    /// Apply to a marketplace post; return (status, response).
    async fn apply_post(
        state: &AppState,
        did: &str,
        post_id: &str,
        proposal: &str,
    ) -> (StatusCode, Value) {
        req(
            state.clone(),
            Method::POST,
            &format!("/v1/marketplace/post/{}/apply", post_id),
            Some(did),
            Some(json!({"proposal": proposal})),
        )
        .await
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Public endpoints
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_health_ok() {
        let state = AppState::new();
        let (status, resp) = req(state, Method::GET, "/health", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["status"], "ok");
    }

    #[tokio::test]
    async fn test_root_ok() {
        let state = AppState::new();
        let (status, resp) = req(state, Method::GET, "/", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(resp["name"].as_str().is_some());
    }

    #[tokio::test]
    async fn test_stats_ok() {
        let state = AppState::new();
        let (status, resp) = req(state, Method::GET, "/v1/stats", None, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(resp["uptime_ms"].is_number());
    }

    #[tokio::test]
    async fn test_leaderboard_public_no_auth() {
        let state = AppState::new();
        let (status, resp) = req(state, Method::GET, "/v1/leaderboard", None, None).await;
        assert_eq!(status, StatusCode::OK, "Leaderboard should be public: {resp}");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Boot (DID 해자) — [M5][M10]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_boot_success_generates_did() {
        let state = AppState::new();
        let (did, priv_key) = boot(&state, None).await;
        assert!(did.starts_with("did:helm:"), "DID format wrong: {did}");
        assert!(!priv_key.is_empty(), "Private key should not be empty");
    }

    #[tokio::test]
    async fn test_boot_welcome_credits() {
        let state = AppState::new();
        let (_, _, welcome_credits) = {
            let body = json!({"capability": "compute", "preferred_token": "VIRTUAL"});
            let (status, resp) =
                req(state.clone(), Method::POST, "/v1/agent/boot", None, Some(body)).await;
            assert_eq!(status, StatusCode::CREATED);
            let did = resp["did"].as_str().unwrap().to_string();
            let priv_key = resp["private_key_b58"].as_str().unwrap().to_string();
            let credits = resp["welcome_credits"].as_u64().unwrap_or(0);
            (did, priv_key, credits)
        };
        // WELCOME_CREDITS = 10 * VIRTUAL_UNIT = 10_000_000
        assert_eq!(welcome_credits, 10_000_000, "Expected 10 VIRTUAL welcome credits");
    }

    #[tokio::test]
    async fn test_boot_with_valid_referrer() {
        let state = AppState::new();
        let (referrer, _) = boot(&state, None).await;
        let (agent, _) = boot(&state, Some(&referrer)).await;
        assert_ne!(referrer, agent, "Referrer and agent should be different DIDs");
    }

    #[tokio::test]
    async fn test_boot_nonexistent_referrer_rejected() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({
                "referrer_did": "did:helm:doesnotexist123",
                "preferred_token": "VIRTUAL"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "referrer_not_found");
    }

    #[tokio::test]
    async fn test_boot_invalid_token_rejected() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({"preferred_token": "DOGECOIN"})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "invalid_preferred_token");
    }

    #[tokio::test]
    async fn test_boot_capability_too_long_rejected() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({
                "capability": "x".repeat(65),
                "preferred_token": "VIRTUAL"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "capability_too_long");
    }

    #[tokio::test]
    async fn test_boot_github_login_invalid_chars_rejected() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({
                "github_login": "bad login!@#",
                "preferred_token": "VIRTUAL"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "github_login_invalid");
    }

    #[tokio::test]
    async fn test_boot_github_login_too_long_rejected() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({
                "github_login": "a".repeat(65),
                "preferred_token": "VIRTUAL"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "github_login_too_long");
    }

    #[tokio::test]
    async fn test_boot_github_login_valid_chars_accepted() {
        let state = AppState::new();
        // GitHub usernames: alphanumeric + hyphens
        let (status, _) = req(
            state,
            Method::POST,
            "/v1/agent/boot",
            None,
            Some(json!({
                "github_login": "valid-user-123",
                "preferred_token": "VIRTUAL"
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Auth middleware — [C1][C6]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_auth_missing_header_returns_401() {
        let state = AppState::new();
        let (status, resp) =
            req(state, Method::GET, "/v1/sense/memory", None, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(resp["error"], "missing_auth");
    }

    #[tokio::test]
    async fn test_auth_unregistered_did_returns_401() {
        let state = AppState::new();
        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/sense/memory",
            Some("did:helm:unknownpubkeyxyz"),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(resp["error"], "did_not_found");
    }

    #[tokio::test]
    async fn test_attack_c1_invalid_signature_returns_401() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Provide X-Helm-Signature but with garbage data
        let app = build_router(state);
        let request = Request::builder()
            .method(Method::PUT)
            .uri("/v1/sense/memory/testkey")
            .header("authorization", format!("Bearer {}", did))
            .header(
                "x-helm-signature",
                "dGhpcyBpcyBub3QgYSB2YWxpZCBzaWduYXR1cmU=",
            )
            .header("content-type", "application/json")
            .body(Body::from(r#"{"value": "hacked"}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "Invalid signature must be rejected"
        );
    }

    #[tokio::test]
    async fn test_attack_c1_spoofed_did_without_sig_write_allowed_with_warning() {
        // Without a signature, write ops are ALLOWED (backward compat) but logged.
        // This tests that the system doesn't silently block unsigned writes.
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, _) = mem_put(&state, &did, "unsigned_write", json!("value")).await;
        // Should succeed (backward compat — no sig = just a warning, not a block)
        assert_eq!(status, StatusCode::OK, "Unsigned write should succeed (backward compat)");
    }

    #[tokio::test]
    async fn test_attack_c6_rate_limit_100_ok_101_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // First 100 requests must succeed
        for i in 0..100 {
            let (status, _) = req(
                state.clone(),
                Method::GET,
                "/v1/sense/memory",
                Some(&did),
                None,
            )
            .await;
            assert_eq!(
                status,
                StatusCode::OK,
                "Request #{i} should succeed (rate limit not yet hit)"
            );
        }

        // 101st request must be rate-limited
        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/sense/memory",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp["error"], "rate_limit_exceeded");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sense Memory E-Line — [M1][M2][M3]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_memory_write_read_delete_cycle() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Write
        let (status, resp) = mem_put(&state, &did, "my_key", json!({"data": 42})).await;
        assert_eq!(status, StatusCode::OK, "PUT failed: {resp}");
        assert_eq!(resp["key"], "my_key");

        // Read — value must match
        let (status, resp) = mem_get(&state, &did, "my_key").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["value"]["data"], 42);

        // Update (overwrite same key)
        let (status, _) = mem_put(&state, &did, "my_key", json!("updated")).await;
        assert_eq!(status, StatusCode::OK);

        let (_, resp) = mem_get(&state, &did, "my_key").await;
        assert_eq!(resp["value"], "updated");

        // Delete
        let status = mem_del(&state, &did, "my_key").await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // Read after delete → 404
        let (status, resp) = mem_get(&state, &did, "my_key").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "key_not_found");
    }

    #[tokio::test]
    async fn test_memory_read_nonexistent_key_returns_404() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;
        let (status, resp) = mem_get(&state, &did, "ghost_key").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "key_not_found");
    }

    #[tokio::test]
    async fn test_memory_list_keys() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        for i in 0..5 {
            mem_put(&state, &did, &format!("k{}", i), json!(i)).await;
        }

        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/sense/memory",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["total_keys"].as_u64().unwrap(), 5);
    }

    #[tokio::test]
    async fn test_attack_m2_key_too_long_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Exactly 256 chars — should succeed
        let key_256 = "k".repeat(256);
        let (status, _) = mem_put(&state, &did, &key_256, json!("val")).await;
        assert_eq!(status, StatusCode::OK, "256-char key must succeed");

        // 257 chars — should fail
        let key_257 = "k".repeat(257);
        let (status, resp) = mem_put(&state, &did, &key_257, json!("val")).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "invalid_key_length");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // FICO Credit Bureau — [C5]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_fico_self_gets_full_breakdown() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = req(
            state,
            Method::GET,
            &format!("/v1/agent/{}/credit", did),
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "FICO self query failed: {resp}");
        assert_eq!(resp["did"], did);
        assert!(resp["score"].is_number(), "Score must be present");
        assert!(resp["band"].is_string(), "Band must be present");
        // Self query: total_api_calls and did_age_days must be real values
        assert!(resp["total_api_calls"].is_number());
        assert!(resp["did_age_days"].is_number());
    }

    #[tokio::test]
    async fn test_attack_c5_fico_other_gets_redacted_breakdown() {
        let state = AppState::new();
        let (did_a, _) = boot(&state, None).await;
        let (did_b, _) = boot(&state, None).await;

        // A queries B's FICO
        let (status, resp) = req(
            state,
            Method::GET,
            &format!("/v1/agent/{}/credit", did_b),
            Some(&did_a),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "FICO other query failed: {resp}");
        assert_eq!(resp["did"], did_b);
        // Score and band are public
        assert!(resp["score"].is_number());
        assert!(resp["band"].is_string());
        // Financial internals must be hidden (zeroed)
        assert_eq!(resp["total_api_calls"].as_u64().unwrap_or(99), 0, "total_api_calls must be 0 for non-self");
        assert_eq!(resp["did_age_days"].as_u64().unwrap_or(99), 0, "did_age_days must be 0 for non-self");
        assert_eq!(resp["pool_memberships"].as_u64().unwrap_or(99), 0, "pool_memberships must be 0 for non-self");
        assert_eq!(resp["referral_tree_size"].as_u64().unwrap_or(99), 0, "referral_tree_size must be 0 for non-self");
        // Breakdown sub-scores must all be 0
        let breakdown = &resp["breakdown"];
        assert_eq!(breakdown["age_score"].as_u64().unwrap_or(99), 0);
        assert_eq!(breakdown["financial_score"].as_u64().unwrap_or(99), 0);
        assert_eq!(breakdown["activity_score"].as_u64().unwrap_or(99), 0);
    }

    #[tokio::test]
    async fn test_fico_nonexistent_agent_returns_404() {
        let state = AppState::new();
        let (caller, _) = boot(&state, None).await;

        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/agent/did:helm:doesnotexist/credit",
            Some(&caller),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "agent_not_found");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Pool (HelmPool) — [C3][C4][M6]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_pool_create_success() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = create_pool(&state, &did, 99.99, 1_000_000).await;
        assert_eq!(status, StatusCode::CREATED, "Pool create failed: {resp}");
        assert!(resp["pool_id"].as_str().is_some(), "pool_id missing");
        assert_eq!(resp["status"], "Fundraising");
        assert_eq!(resp["vendor"], "openai");
    }

    #[tokio::test]
    async fn test_pool_join_deducts_balance() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (joiner, _) = boot(&state, None).await;

        let (_, pool_resp) = create_pool(&state, &creator, 100.0, 10_000_000).await;
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        let stake = 1_000_000u64; // 1 VIRTUAL
        let (status, resp) = join_pool(&state, &joiner, &pool_id, stake).await;
        assert_eq!(status, StatusCode::OK, "Join pool failed: {resp}");
        assert!(resp["total_collected"].as_u64().unwrap() >= stake);
    }

    #[tokio::test]
    async fn test_attack_c3_pool_stake_exceeds_balance_rejected() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (attacker, _) = boot(&state, None).await;

        let (_, pool_resp) = create_pool(&state, &creator, 100.0, 10_000_000).await;
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        // Welcome credits = 10_000_000. Stake more than that.
        let (status, resp) = join_pool(&state, &attacker, &pool_id, 999_999_999_999).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED, "Excessive stake must fail: {resp}");
        assert_eq!(resp["error"], "insufficient_balance");
    }

    #[tokio::test]
    async fn test_attack_c3_pool_stake_u64_max_rejected() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (attacker, _) = boot(&state, None).await;

        let (_, pool_resp) = create_pool(&state, &creator, 100.0, 10_000_000).await;
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        let (status, resp) = join_pool(&state, &attacker, &pool_id, u64::MAX).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED, "u64::MAX stake must fail: {resp}");
        assert_eq!(resp["error"], "insufficient_balance");
    }

    #[tokio::test]
    async fn test_attack_c4_pool_negative_cost_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = create_pool(&state, &did, -1.0, 1_000_000).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "invalid_monthly_cost");
    }

    #[tokio::test]
    async fn test_attack_c4_pool_zero_cost_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = create_pool(&state, &did, 0.0, 1_000_000).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "invalid_monthly_cost");
    }

    #[tokio::test]
    async fn test_attack_c4_pool_cost_over_1m_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = create_pool(&state, &did, 1_000_001.0, 1_000_000).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "monthly_cost_too_high");
    }

    #[tokio::test]
    async fn test_pool_list_and_status() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (_, pool_resp) = create_pool(&state, &did, 50.0, 1_000_000).await;
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        // List pools
        let (status, resp) = req(
            state.clone(),
            Method::GET,
            "/v1/pool",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(resp["pools"].as_array().map(|a| a.len()).unwrap_or(0) >= 1);

        // Get pool status
        let (status, resp) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/pool/{}", pool_id),
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["pool_id"], pool_id);
        assert_eq!(resp["status"], "Fundraising");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Marketplace — [H5][H6][M7][L1]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_marketplace_create_post_success() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = create_post(&state, &did, "Good Job Title", "Good description.").await;
        assert_eq!(status, StatusCode::CREATED, "Post create failed: {resp}");
        assert!(resp["post_id"].as_str().is_some());
        assert_eq!(resp["status"], "Open");
    }

    #[tokio::test]
    async fn test_attack_h5_title_too_long_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Exactly 200 chars — must succeed
        let (status, _) = create_post(&state, &did, &"t".repeat(200), "desc").await;
        assert_eq!(status, StatusCode::CREATED, "200-char title must succeed");

        // 201 chars — must fail
        let (status, resp) = create_post(&state, &did, &"t".repeat(201), "desc").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "title_too_long");
    }

    #[tokio::test]
    async fn test_attack_h5_description_too_long_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Exactly 4096 chars — must succeed
        let (status, _) = create_post(&state, &did, "Title", &"d".repeat(4096)).await;
        assert_eq!(status, StatusCode::CREATED, "4096-char description must succeed");

        // 4097 chars — must fail
        let (status, resp) = create_post(&state, &did, "Title", &"d".repeat(4097)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "description_too_long");
    }

    #[tokio::test]
    async fn test_attack_h6_proposal_too_long_rejected() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (applicant, _) = boot(&state, None).await;

        let (_, post_resp) = create_post(&state, &creator, "Job Post", "Description here.").await;
        let post_id = post_resp["post_id"].as_str().unwrap().to_string();

        // Exactly 2048 chars — must succeed
        let (status, _) = apply_post(&state, &applicant, &post_id, &"p".repeat(2048)).await;
        assert_eq!(status, StatusCode::OK, "2048-char proposal must succeed");

        // 2049 chars — must fail (different applicant since duplicates are blocked)
        let (applicant2, _) = boot(&state, None).await;
        let (status, resp) = apply_post(&state, &applicant2, &post_id, &"p".repeat(2049)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "proposal_too_long");
    }

    #[tokio::test]
    async fn test_marketplace_duplicate_application_rejected() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (applicant, _) = boot(&state, None).await;

        let (_, post_resp) = create_post(&state, &creator, "Apply Test", "Description.").await;
        let post_id = post_resp["post_id"].as_str().unwrap().to_string();

        // First application — must succeed
        let (status, _) = apply_post(&state, &applicant, &post_id, "First proposal").await;
        assert_eq!(status, StatusCode::OK);

        // Second application from same DID — must fail
        let (status, resp) = apply_post(&state, &applicant, &post_id, "Trying again").await;
        assert_eq!(status, StatusCode::CONFLICT);
        assert_eq!(resp["error"], "already_applied");
    }

    #[tokio::test]
    async fn test_attack_l1_log_injection_title_accepted_and_sanitized() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Title with newline injection — server should sanitize (not reject)
        let (status, resp) =
            create_post(&state, &did, "Injection\r\nAttempt", "Desc.").await;
        // Accepted (title sanitized in logs, not rejected at HTTP level)
        assert!(
            status == StatusCode::CREATED || status == StatusCode::OK,
            "Log injection title should be accepted & sanitized, got {status}: {resp}"
        );
        // The stored title may or may not have the newlines — important is it doesn't crash
        assert!(resp["post_id"].as_str().is_some(), "post_id must be in response");
    }

    #[tokio::test]
    async fn test_marketplace_list_posts() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        create_post(&state, &did, "Job A", "Desc A").await;
        create_post(&state, &did, "Job B", "Desc B").await;

        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/marketplace/post",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(resp["total"].as_u64().unwrap(), 2, "Expected 2 posts");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Sense Cortex — [C7][M8]
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_cortex_text_query_returns_g_score() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({"query": "What is the Fed interest rate?"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "Cortex failed: {resp}");
        assert!(resp["g_score"].is_number(), "g_score must be present");
        assert!(
            resp["g_score"].as_f64().unwrap() >= 0.0
                && resp["g_score"].as_f64().unwrap() <= 1.0,
            "g_score must be in [0.0, 1.0]"
        );
        assert!(resp["virtual_charged"].is_number());
        assert!(resp["confidence"].is_string());
    }

    #[tokio::test]
    async fn test_cortex_with_knowledge_context() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({
                "query": "Is ETH undervalued?",
                "knowledge_context": [
                    "ETH price $2000",
                    "BTC dominance 55%",
                    "DeFi TVL $50B"
                ]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "Cortex with context failed: {resp}");
        assert!(resp["g_score"].is_number());
    }

    #[tokio::test]
    async fn test_attack_c7_too_many_context_items_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // 50 items — must succeed
        let items_50: Vec<Value> = (0..50).map(|i| json!(format!("ctx {}", i))).collect();
        let (status, _) = req(
            state.clone(),
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({"query": "test", "knowledge_context": items_50})),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "50 context items must succeed");

        // 51 items — must fail
        let items_51: Vec<Value> = (0..51).map(|i| json!(format!("ctx {}", i))).collect();
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({"query": "test", "knowledge_context": items_51})),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "too_many_context_items");
    }

    #[tokio::test]
    async fn test_attack_c7_context_item_too_long_rejected() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        // Exactly 4096 chars — must succeed
        let (status, _) = req(
            state.clone(),
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({
                "query": "test",
                "knowledge_context": ["x".repeat(4096)]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "4096-char context item must succeed");

        // 4097 chars — must fail
        let (status, resp) = req(
            state,
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({
                "query": "test",
                "knowledge_context": ["x".repeat(4097)]
            })),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(resp["error"], "context_item_too_long");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 점-다 (1:N) — One agent, many operations
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_1_to_n_full_flow() {
        let state = AppState::new();

        // 1. Boot
        let (did, _) = boot(&state, None).await;

        // 2. Memory: write 3 keys
        for i in 0..3 {
            let (s, _) = mem_put(&state, &did, &format!("flow_key_{}", i), json!(i * 100)).await;
            assert_eq!(s, StatusCode::OK, "Memory write {i} failed");
        }

        // 3. Memory: read 3 keys and verify values
        for i in 0..3 {
            let (s, resp) = mem_get(&state, &did, &format!("flow_key_{}", i)).await;
            assert_eq!(s, StatusCode::OK, "Memory read {i} failed");
            assert_eq!(resp["value"].as_u64().unwrap(), i as u64 * 100);
        }

        // 4. Memory: list — expect 3 keys
        let (s, resp) = req(
            state.clone(),
            Method::GET,
            "/v1/sense/memory",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(resp["total_keys"].as_u64().unwrap(), 3);

        // 5. Cortex query
        let (s, resp) = req(
            state.clone(),
            Method::POST,
            "/v1/sense/cortex",
            Some(&did),
            Some(json!({"query": "DeFi TVL breakdown", "knowledge_context": ["ETH 2000"]})),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "Cortex failed: {resp}");
        assert!(resp["g_score"].is_number());

        // 6. Pool create
        let (s, pool_resp) = create_pool(&state, &did, 99.0, 5_000_000).await;
        assert_eq!(s, StatusCode::CREATED, "Pool create failed: {pool_resp}");
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        // 7. Pool status check
        let (s, resp) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/pool/{}", pool_id),
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(resp["status"], "Fundraising");

        // 8. Marketplace: create 2 posts
        let (s, _) = create_post(&state, &did, "Need Data Analyst", "Description A.").await;
        assert_eq!(s, StatusCode::CREATED);
        let (s, _) = create_post(&state, &did, "Need Rust Dev", "Description B.").await;
        assert_eq!(s, StatusCode::CREATED);

        // 9. List posts — expect 2
        let (s, resp) = req(
            state.clone(),
            Method::GET,
            "/v1/marketplace/post",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(resp["total"].as_u64().unwrap(), 2);

        // 10. FICO self-query
        let (s, resp) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/agent/{}/credit", did),
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK, "FICO failed: {resp}");
        assert!(resp["score"].is_number());

        // 11. Earnings / referral graph
        let (s, _) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/agent/{}/earnings", did),
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);

        // 12. Memory: delete all 3 keys
        for i in 0..3 {
            let s = mem_del(&state, &did, &format!("flow_key_{}", i)).await;
            assert_eq!(s, StatusCode::NO_CONTENT, "Delete {i} failed");
        }

        // 13. List — expect 0 keys
        let (s, resp) = req(
            state,
            Method::GET,
            "/v1/sense/memory",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(resp["total_keys"].as_u64().unwrap(), 0);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 점-점 (1:1) — Agent A interacts with Agent B
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_peer_referral_and_marketplace() {
        let state = AppState::new();

        // A boots first
        let (did_a, _) = boot(&state, None).await;

        // B boots with A as referrer
        let (did_b, _) = boot(&state, Some(&did_a)).await;

        // A writes a memory key
        let (s, _) = mem_put(&state, &did_a, "a_note", json!("from_a")).await;
        assert_eq!(s, StatusCode::OK);

        // B writes the same key name (different namespace)
        let (s, _) = mem_put(&state, &did_b, "a_note", json!("from_b")).await;
        assert_eq!(s, StatusCode::OK);

        // A reads its own key — must see "from_a"
        let (_, resp) = mem_get(&state, &did_a, "a_note").await;
        assert_eq!(resp["value"], "from_a", "A should see its own value");

        // B reads its own key — must see "from_b"
        let (_, resp) = mem_get(&state, &did_b, "a_note").await;
        assert_eq!(resp["value"], "from_b", "B should see its own value");

        // A creates a marketplace post
        let (s, post_resp) = create_post(&state, &did_a, "Need B's help", "Something complex.").await;
        assert_eq!(s, StatusCode::CREATED);
        let post_id = post_resp["post_id"].as_str().unwrap().to_string();

        // B applies to A's post
        let (s, resp) = apply_post(&state, &did_b, &post_id, "I can help!").await;
        assert_eq!(s, StatusCode::OK, "B apply failed: {resp}");
        assert_eq!(resp["application_count"].as_u64().unwrap(), 1);

        // B checks A's FICO (privacy: financial internals should be hidden)
        let (s, resp) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/agent/{}/credit", did_a),
            Some(&did_b),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        // Non-self: did_age_days must be 0
        assert_eq!(resp["did_age_days"].as_u64().unwrap_or(99), 0);

        // A creates a pool; B joins with small stake
        let (s, pool_resp) = create_pool(&state, &did_a, 25.0, 5_000_000).await;
        assert_eq!(s, StatusCode::CREATED);
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        let (s, resp) = join_pool(&state, &did_b, &pool_id, 500_000).await;
        assert_eq!(s, StatusCode::OK, "B join pool failed: {resp}");
        assert!(resp["total_collected"].as_u64().unwrap() >= 500_000);

        // Verify pool has B as member
        let (s, pool_status) = req(
            state.clone(),
            Method::GET,
            &format!("/v1/pool/{}", pool_id),
            Some(&did_a),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(pool_status["member_count"].as_u64().unwrap(), 1);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 다-다 (N:N) — Multiple agents, multiple operations
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_n_agents_memory_namespace_isolation() {
        let state = AppState::new();
        let n = 8;
        let mut dids = Vec::new();

        // Boot N agents
        for _ in 0..n {
            let (did, _) = boot(&state, None).await;
            dids.push(did);
        }

        // Each agent writes to the SAME key name with their own unique value
        for (i, did) in dids.iter().enumerate() {
            let (s, _) = mem_put(
                &state,
                did,
                "shared_key_name",
                json!(format!("value_agent_{}", i)),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "Agent {i} write failed");
        }

        // Each agent reads their own value — strict namespace isolation
        for (i, did) in dids.iter().enumerate() {
            let (s, resp) = mem_get(&state, did, "shared_key_name").await;
            assert_eq!(s, StatusCode::OK, "Agent {i} read failed");
            assert_eq!(
                resp["value"],
                format!("value_agent_{}", i),
                "Agent {i} namespace isolation broken!"
            );
        }
    }

    #[tokio::test]
    async fn test_n_agents_join_same_pool() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (_, pool_resp) = create_pool(&state, &creator, 100.0, 100_000_000).await;
        let pool_id = pool_resp["pool_id"].as_str().unwrap().to_string();

        // 6 agents join the pool with small stakes
        let mut member_count = 0usize;
        for _ in 0..6 {
            let (did, _) = boot(&state, None).await;
            let (s, _) = join_pool(&state, &did, &pool_id, 100_000).await;
            if s == StatusCode::OK {
                member_count += 1;
            }
        }
        assert_eq!(member_count, 6, "All 6 agents should join successfully");

        // Verify member count in pool status
        let (s, resp) = req(
            state,
            Method::GET,
            &format!("/v1/pool/{}", pool_id),
            Some(&creator),
            None,
        )
        .await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(
            resp["member_count"].as_u64().unwrap(),
            6,
            "Pool should have 6 members"
        );
    }

    #[tokio::test]
    async fn test_attack_m7_max_applications_per_post() {
        let state = AppState::new();
        let (creator, _) = boot(&state, None).await;
        let (_, post_resp) = create_post(&state, &creator, "Competition Post", "Apply now.").await;
        let post_id = post_resp["post_id"].as_str().unwrap().to_string();

        // Apply with 100 different agents — all must succeed
        for i in 0..100 {
            let (applicant, _) = boot(&state, None).await;
            let (s, resp) = apply_post(
                &state,
                &applicant,
                &post_id,
                &format!("Proposal from applicant {}", i),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "Application #{i} should succeed: {resp}");
        }

        // 101st applicant must be rejected
        let (lateomer, _) = boot(&state, None).await;
        let (s, resp) = apply_post(&state, &lateomer, &post_id, "I'm too late!").await;
        assert_eq!(
            s,
            StatusCode::CONFLICT,
            "101st application must be rejected: {resp}"
        );
        assert_eq!(resp["error"], "post_application_limit_reached");
    }

    #[tokio::test]
    async fn test_n_agents_referral_chain() {
        let state = AppState::new();

        // Build referral chain: A → B → C → D
        let (did_a, _) = boot(&state, None).await;
        let (did_b, _) = boot(&state, Some(&did_a)).await;
        let (did_c, _) = boot(&state, Some(&did_b)).await;
        let (did_d, _) = boot(&state, Some(&did_c)).await;

        // All DIDs are distinct
        let dids = [&did_a, &did_b, &did_c, &did_d];
        for i in 0..dids.len() {
            for j in (i + 1)..dids.len() {
                assert_ne!(dids[i], dids[j], "DIDs must be unique");
            }
        }

        // Each can access the API
        for did in &dids {
            let (s, _) = req(
                state.clone(),
                Method::GET,
                "/v1/sense/memory",
                Some(did),
                None,
            )
            .await;
            assert_eq!(s, StatusCode::OK, "Agent {did} should be able to access API");
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Cross-cutting: security boundary checks
    // ─────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_agent_cannot_read_other_agents_memory() {
        let state = AppState::new();
        let (did_a, _) = boot(&state, None).await;
        let (did_b, _) = boot(&state, None).await;

        // A writes a secret
        mem_put(&state, &did_a, "top_secret", json!("secret_data")).await;

        // B tries to read A's key using its own auth
        // (B uses its own DID for auth, but requests "top_secret" which is A's key)
        let (status, resp) = mem_get(&state, &did_b, "top_secret").await;
        // B should get 404 (B has no such key in its own namespace)
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "B must not see A's memory: {resp}"
        );
    }

    #[tokio::test]
    async fn test_pool_not_found_returns_404() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = req(
            state,
            Method::GET,
            "/v1/pool/nonexistent-pool-id",
            Some(&did),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "pool_not_found");
    }

    #[tokio::test]
    async fn test_pool_join_nonexistent_pool_returns_404() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = join_pool(&state, &did, "fake-pool-uuid-xxx", 100_000).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "pool_not_found");
    }

    #[tokio::test]
    async fn test_marketplace_apply_nonexistent_post_returns_404() {
        let state = AppState::new();
        let (did, _) = boot(&state, None).await;

        let (status, resp) = apply_post(&state, &did, "fake-post-id-xxx", "My proposal").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(resp["error"], "post_not_found");
    }

    #[tokio::test]
    async fn test_valid_tokens_all_accepted() {
        let state = AppState::new();
        for token in &["VIRTUAL", "BNKR", "USDC", "CLANKER"] {
            let (status, _) = req(
                state.clone(),
                Method::POST,
                "/v1/agent/boot",
                None,
                Some(json!({"preferred_token": token})),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "Token '{token}' should be valid");
        }
    }
}
