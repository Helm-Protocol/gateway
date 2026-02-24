// src/dashboard.rs
// ─────────────────────────────────────────────────────────────────────────────
// HELM COMMAND CENTER — Axum SSE 텔레메트리 스트리밍
//
// 라우터:
//   GET /dashboard        → static/index.html 서빙
//   GET /api/telemetry    → SSE 스트림 (1초 간격)
//
// main.rs에 추가:
//   app.merge(dashboard::build_dashboard_router())
// ─────────────────────────────────────────────────────────────────────────────

use axum::{
    response::{Html, sse::{Event, Sse}},
    routing::get,
    Router,
    extract::State,
};
use futures::stream::{self, Stream};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio_stream::StreamExt as _;
use serde::Serialize;

use crate::AppState;

// ─────────────────────────────────
// 텔레메트리 스냅샷 (SSE 페이로드)
// ─────────────────────────────────
#[derive(Serialize, Clone)]
pub struct TelemetrySnapshot {
    pub tps:              u64,
    pub total_bnkr:       f64,
    pub cache_hit_rate:   u8,
    pub blocked_attacks:  u64,
    pub active_visas:     u64,
    pub nonce_replays:    u64,
    pub rollup_queue:     u64,
    pub total_calls:      u64,
    pub treasury_usd:     f64,
    // 4전선 TPS
    pub tps_a: u64,
    pub tps_b: u64,
    pub tps_c: u64,
    pub tps_d: u64,
    // G-Metric 분포 (10구간)
    pub g_distribution: [u32; 10],
}

// ─────────────────────────────────
// 대시보드 HTML 서빙
// ─────────────────────────────────
async fn serve_dashboard() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

// ─────────────────────────────────
// SSE 텔레메트리 핸들러
// ─────────────────────────────────
async fn sse_telemetry(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {

    let stream = stream::unfold(state, |app| async move {
        tokio::time::sleep(Duration::from_secs(1)).await;

        // AppState에서 실제 지표를 읽어온다
        // (각 모듈이 Arc<AtomicU64> 카운터로 공유)
        let snapshot = TelemetrySnapshot {
            tps:             app.metrics.tps.load(std::sync::atomic::Ordering::Relaxed),
            total_bnkr:      app.metrics.total_bnkr_f64(),
            cache_hit_rate:  app.metrics.cache_hit_rate(),
            blocked_attacks: app.metrics.blocked_attacks.load(std::sync::atomic::Ordering::Relaxed),
            active_visas:    app.metrics.active_visas.load(std::sync::atomic::Ordering::Relaxed),
            nonce_replays:   app.metrics.nonce_replays.load(std::sync::atomic::Ordering::Relaxed),
            rollup_queue:    app.metrics.rollup_queue.load(std::sync::atomic::Ordering::Relaxed),
            total_calls:     app.metrics.total_calls.load(std::sync::atomic::Ordering::Relaxed),
            treasury_usd:    app.metrics.treasury_usd_f64(),
            tps_a: app.metrics.tps_a.load(std::sync::atomic::Ordering::Relaxed),
            tps_b: app.metrics.tps_b.load(std::sync::atomic::Ordering::Relaxed),
            tps_c: app.metrics.tps_c.load(std::sync::atomic::Ordering::Relaxed),
            tps_d: app.metrics.tps_d.load(std::sync::atomic::Ordering::Relaxed),
            g_distribution: app.metrics.g_distribution_snapshot(),
        };

        let event = Event::default()
            .json_data(&snapshot)
            .expect("serialization never fails");

        Some((Ok(event), app))
    });

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"))
}

// ─────────────────────────────────
// 라우터 빌더
// ─────────────────────────────────
pub fn build_dashboard_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/dashboard",      get(serve_dashboard))
        .route("/api/telemetry",  get(sse_telemetry))
}
