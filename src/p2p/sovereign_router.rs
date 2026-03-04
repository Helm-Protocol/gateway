// src/p2p/sovereign_router.rs
// ═══════════════════════════════════════════════════════════════
// GANDIVA-QUIC HYPER ARCHITECTURE — SOVEREIGN ROUTER
// ═══════════════════════════════════════════════════════════════
// This module implements the Application-Aware Multipath and 
// 8-Dimensional Sliver Shot for the Gandiva protocol.
// It bypasses TCP HOL Blocking by utilizing independent QUIC streams.

use std::sync::Arc;
use std::time::Duration;
use quinn::{Connection, Endpoint, SendStream};
use tokio::sync::Mutex;
use tracing::{info, warn};

/// 8D Sliver 데이터 조각
#[derive(Debug, Clone)]
pub struct Sliver {
    pub id: u8, // 1 to 8
    pub priority: SliverPriority,
    pub payload: Vec<u8>,
}

/// RedStuff 알고리즘 위계에 따른 우선순위
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SliverPriority {
    /// Priority 0: 모든 것의 시작 (최우선)
    Manifest = 0,
    /// Priority 1: 복구 임계값 달성용 기본 데이터
    Primary = 1,
    /// Priority 2: Reed-Solomon 여분 데이터
    Secondary = 2,
    /// Priority 3: 기타 (Texture/Asset 등)
    Tertiary = 3,
}

pub struct SovereignRouter {
    pub endpoints: Vec<Arc<Endpoint>>,
}

impl SovereignRouter {
    pub fn new(endpoints: Vec<Arc<Endpoint>>) -> Self {
        Self { endpoints }
    }

    /// [제1격] HOL-Free 사출: 8차원 Sliver Shot
    /// 각 슬라이버를 독립적인 QUIC 단방향 스트림으로 병렬 전송합니다.
    pub async fn gandiva_sliver_shot(
        &self,
        connection: Arc<Connection>,
        slivers: Vec<Sliver>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut join_handles = vec![];

        info!("🏹 Initiating Gandiva 8D Sliver Shot... (Total: {})", slivers.len());

        for sliver in slivers {
            let conn = connection.clone();
            
            // 병렬 사출 (HOL Blocking 방지)
            let handle = tokio::spawn(async move {
                // 0-RTT를 지원하는 스트림 개방
                match conn.open_uni().await {
                    Ok(mut stream) => {
                        // [제3격] 스트림 우선순위 부여
                        // Quinn 0.11에서는 set_priority를 지원합니다.
                        let weight = match sliver.priority {
                            SliverPriority::Manifest => 0,  // Highest
                            SliverPriority::Primary => 1,
                            SliverPriority::Secondary => 2,
                            SliverPriority::Tertiary => 3,
                        };
                        
                        // Note: quinn 0.11 SendStream priority 
                        if let Err(e) = stream.set_priority(weight) {
                            warn!("Failed to set stream priority: {}", e);
                        }

                        // Payload 전송 (0-RTT 즉각 사출)
                        if let Err(e) = stream.write_all(&sliver.payload).await {
                            warn!("Sliver {} shot failed: {}", sliver.id, e);
                        } else {
                            info!("Sliver {} (Priority: {:?}) successfully hit the target.", sliver.id, sliver.priority);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to open QUIC stream for Sliver {}: {}", sliver.id, e);
                    }
                }
            });
            
            join_handles.push(handle);
        }

        // 모든 사출이 완료될 때까지 대기 (비동기 병렬 처리)
        for handle in join_handles {
            let _ = handle.await;
        }

        Ok(())
    }

    /// [제3격] 멀티패스 라우팅 (Application-Aware Multipath)
    /// 인도망(Jio/Airtel)과 같은 DPI 압박이 높은 환경에서 
    /// 슬라이버를 여러 Edge Node로 분산하여 발사합니다.
    pub async fn multipath_sliver_shot(
        &self,
        connections: Vec<Arc<Connection>>, // 예: [Jio_Conn, Airtel_Conn]
        slivers: Vec<Sliver>,
    ) {
        if connections.is_empty() {
            warn!("No available paths for Multipath Shot!");
            return;
        }

        let num_paths = connections.len();
        let mut handles = vec![];

        for (i, sliver) in slivers.into_iter().enumerate() {
            // Round-robin 분산 라우팅
            let conn = connections[i % num_paths].clone();
            
            handles.push(tokio::spawn(async move {
                if let Ok(mut stream) = conn.open_uni().await {
                    let weight = sliver.priority.clone() as i32;
                    let _ = stream.set_priority(weight);
                    let _ = stream.write_all(&sliver.payload).await;
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }
        
        info!("🌐 Multipath 8D Shot completed across {} paths.", num_paths);
    }
}
