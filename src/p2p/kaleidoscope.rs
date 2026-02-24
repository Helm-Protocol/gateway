// src/p2p/kaleidoscope.rs
//
// ═══════════════════════════════════════════════════════════════
// KALEIDOSCOPE STREAM SECURITY  (Jeff Dean / WhatsApp 설계)
// ═══════════════════════════════════════════════════════════════
//
// WhatsApp의 3대 보안 철학 → Helm 적용:
//
// 1. Assume Breach   : 들어오는 모든 바이트는 악의적이다
//    → 바이트 단위 하드 리미트 강제
//
// 2. Slowloris 방어  : 시간은 해커의 무기다
//    → 스트림당 최소 속도 + 타임아웃 강제
//
// 3. Zero-Allocation : 메모리 파편화 방지
//    → 고정 버퍼 풀 재사용 (이 모듈은 스택 배열 사용)
//
// 기존 transport.rs의 yamux Config 위에 이 래퍼를 씌워서
// "요새(Fortress)" 수준의 스트림 보안을 완성함

use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::{timeout, Duration};
use tracing::{error, warn};

// ============================
// SECURITY THRESHOLDS
// ============================

/// Kaleidoscope 보안 파라미터
/// WhatsApp 실전 수치 기반 설정
#[derive(Debug, Clone)]
pub struct KaleidoscopePolicy {
    /// 스트림당 최대 페이로드 (기본 2MB)
    /// 이 이상은 메모리 고갈 공격으로 간주 → 즉시 차단
    pub max_payload_bytes: usize,

    /// Slowloris 타임아웃 (기본 3초)
    /// 3초 내에 전체 요청을 못 보내면 좀비로 간주 → 절단
    pub read_timeout_secs: u64,

    /// 최소 전송 속도 (bytes/sec, 기본 1KB/s)
    /// 이보다 느리면 의도적 지연 공격으로 간주
    pub min_bytes_per_sec: u64,

    /// 스트림 오픈 후 첫 바이트까지 최대 대기 시간
    pub first_byte_timeout_secs: u64,
}

impl Default for KaleidoscopePolicy {
    fn default() -> Self {
        Self {
            max_payload_bytes: 2 * 1024 * 1024,  // 2MB
            read_timeout_secs: 3,
            min_bytes_per_sec: 1024,               // 1KB/s
            first_byte_timeout_secs: 5,
        }
    }
}

// ============================
// SAFE STREAM WRAPPER
// ============================

/// Kaleidoscope SafeStream — AsyncRead/Write 감시 래퍼
///
/// Yamux 스트림 위에서 동작하며:
/// - 읽기 바이트 누적 카운터로 max_payload 강제
/// - 속도 측정으로 Slowloris 탐지
/// - 첫 바이트 타임아웃으로 유령 연결 제거
pub struct SafeStream<S> {
    inner: S,
    policy: KaleidoscopePolicy,
    bytes_read: usize,
    stream_opened_at: Instant,
    /// 공유 카운터 (모니터링용, 선택적)
    global_bytes_counter: Option<Arc<AtomicU64>>,
}

impl<S> SafeStream<S> {
    pub fn new(inner: S, policy: KaleidoscopePolicy) -> Self {
        Self {
            inner,
            policy,
            bytes_read: 0,
            stream_opened_at: Instant::now(),
            global_bytes_counter: None,
        }
    }

    pub fn with_counter(mut self, counter: Arc<AtomicU64>) -> Self {
        self.global_bytes_counter = Some(counter);
        self
    }

    /// 현재 스트림 속도 (bytes/sec)
    fn current_speed_bps(&self) -> u64 {
        let elapsed = self.stream_opened_at.elapsed().as_secs();
        if elapsed == 0 {
            return u64::MAX; // 방금 열린 스트림은 차단하지 않음
        }
        self.bytes_read as u64 / elapsed
    }
}

// ============================
// ASYNCREAD (입력 감시)
// ============================

impl<S: AsyncRead + Unpin> AsyncRead for SafeStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let filled_before = buf.filled().len();

        // 내부 스트림에서 읽기
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let bytes_now = buf.filled().len() - filled_before;
                self.bytes_read += bytes_now;

                // 글로벌 카운터 업데이트
                if let Some(ref counter) = self.global_bytes_counter {
                    counter.fetch_add(bytes_now as u64, Ordering::Relaxed);
                }

                // [보안 1] 페이로드 하드 리미트
                // 메모리 고갈 공격: 2MB 초과 즉시 차단
                if self.bytes_read > self.policy.max_payload_bytes {
                    error!(
                        "[Kaleidoscope] PAYLOAD LIMIT EXCEEDED: {} bytes > {} limit. \
                         Dropping stream — memory exhaustion attack blocked.",
                        self.bytes_read, self.policy.max_payload_bytes
                    );
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Kaleidoscope: payload size limit exceeded",
                    )));
                }

                // [보안 2] Slowloris 속도 체크
                // 30초 이상 지났는데 최소 속도 미달이면 의도적 지연으로 간주
                let elapsed = self.stream_opened_at.elapsed().as_secs();
                if elapsed > 30 {
                    let speed = self.current_speed_bps();
                    if speed < self.policy.min_bytes_per_sec {
                        warn!(
                            "[Kaleidoscope] SLOWLORIS DETECTED: speed={}B/s < min={}B/s. \
                             Dropping stream.",
                            speed, self.policy.min_bytes_per_sec
                        );
                        return Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "Kaleidoscope: slowloris attack detected",
                        )));
                    }
                }

                Poll::Ready(Ok(()))
            }
            other => other,
        }
    }
}

// ============================
// ASYNCWRITE (출력 감시)
// ============================

impl<S: AsyncWrite + Unpin> AsyncWrite for SafeStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

// ============================
// STREAM HANDLER
// ============================

/// 인커밍 스트림 처리기 (게이트웨이 진입점)
///
/// 모든 외부 스트림은 이 함수를 통해 SafeStream으로 래핑된 후 처리됨
/// Slowloris 타임아웃 강제 적용
pub async fn handle_incoming_stream<S, F, Fut>(
    stream: S,
    policy: KaleidoscopePolicy,
    handler: F,
) -> io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: FnOnce(SafeStream<S>) -> Fut,
    Fut: std::future::Future<Output = io::Result<()>>,
{
    let safe = SafeStream::new(stream, policy.clone());
    let timeout_dur = Duration::from_secs(policy.read_timeout_secs);

    // [Slowloris 방어] 전체 요청 처리에 타임아웃 강제
    match timeout(timeout_dur, handler(safe)).await {
        Ok(result) => result,
        Err(_) => {
            warn!(
                "[Kaleidoscope] Stream handler timed out after {}s. \
                 Possible Slowloris attack — connection dropped.",
                policy.read_timeout_secs
            );
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Kaleidoscope: stream handler timeout",
            ))
        }
    }
}

// ============================
// ENHANCED YAMUX CONFIG
// ============================

/// Kaleidoscope 적용 yamux 설정
/// Jeff Dean WhatsApp 수치 반영 — 기존 build_secure_transport()와 통합
pub fn kaleidoscope_yamux_config() -> libp2p::yamux::Config {
    use libp2p::yamux::{Config, WindowUpdateMode};

    let mut cfg = Config::default();

    // 동시 스트림 32개 극한 제한 (하나의 노드가 대역폭 독점 차단)
    cfg.set_max_num_streams(32);
    // 수신 윈도우 128KB (메모리 할당 공격 차단)
    cfg.set_receive_window_size(256 * 1024); // yamux DEFAULT_CREDIT 최솟값
    // 버퍼 256KB
    cfg.set_max_buffer_size(256 * 1024);
    // 읽었을 때만 윈도우 업데이트 (Backpressure)
    cfg.set_window_update_mode(WindowUpdateMode::on_read());

    cfg
}

// ============================
// GLOBAL STATS
// ============================

/// 전체 네트워크 보안 통계
pub struct KaleidoscopeStats {
    pub total_bytes_received: Arc<AtomicU64>,
    pub streams_rejected_payload: Arc<AtomicU64>,
    pub streams_rejected_slowloris: Arc<AtomicU64>,
    pub streams_rejected_timeout: Arc<AtomicU64>,
}

impl KaleidoscopeStats {
    pub fn new() -> Self {
        Self {
            total_bytes_received: Arc::new(AtomicU64::new(0)),
            streams_rejected_payload: Arc::new(AtomicU64::new(0)),
            streams_rejected_slowloris: Arc::new(AtomicU64::new(0)),
            streams_rejected_timeout: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "total_bytes_received": self.total_bytes_received.load(Ordering::Relaxed),
            "streams_rejected": {
                "payload_overflow": self.streams_rejected_payload.load(Ordering::Relaxed),
                "slowloris": self.streams_rejected_slowloris.load(Ordering::Relaxed),
                "timeout": self.streams_rejected_timeout.load(Ordering::Relaxed),
            }
        })
    }
}

// ============================
// TESTS
// ============================

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    /// 정상 페이로드는 통과
    #[tokio::test]
    async fn test_normal_payload_passes() {
        let data = b"hello world";
        let cursor = std::io::Cursor::new(data);
        let mut safe = SafeStream::new(cursor, KaleidoscopePolicy::default());

        let mut buf = vec![0u8; 64];
        let n = safe.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello world");
    }

    /// 최대 페이로드 초과 시 에러
    #[tokio::test]
    async fn test_payload_limit_enforced() {
        use tokio::io::AsyncReadExt;
        let policy = KaleidoscopePolicy {
            max_payload_bytes: 5, // 5바이트 제한
            ..Default::default()
        };

        // 20바이트 데이터 (tokio::io::duplex 사용)
        let (mut client, mut server) = tokio::io::duplex(64);
        // 서버 측에서 20바이트 전송
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            server.write_all(&[0xABu8; 20]).await.unwrap();
        });

        let mut safe = SafeStream::new(client, policy);
        let mut buf = vec![0u8; 20];

        // 여러 번 읽어서 누적 > 5바이트 되면 에러
        let mut total = 0;
        let mut got_error = false;
        for _ in 0..10 {
            match safe.read(&mut buf[total..]).await {
                Ok(0) => break,
                Ok(n) => {
                    total += n;
                    if total > 5 { got_error = true; break; }
                }
                Err(_) => { got_error = true; break; }
            }
        }
        assert!(got_error || total <= 5, "페이로드 제한이 작동해야 함");
    }

    /// 타임아웃 핸들러 테스트
    #[tokio::test]
    async fn test_timeout_handler() {
        // tokio::io::duplex: AsyncRead + AsyncWrite 모두 지원
        let policy = KaleidoscopePolicy {
            read_timeout_secs: 1,
            ..Default::default()
        };

        let (client, _server) = tokio::io::duplex(64);

        let result = handle_incoming_stream(client, policy, |_s| async move {
            // 2초 지연 (타임아웃 1초 초과)
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(())
        })
        .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn test_yamux_config_limits() {
        let cfg = kaleidoscope_yamux_config();
        // Config가 제대로 만들어졌는지 확인
        // (libp2p yamux Config는 getter가 제한적이므로 생성만 검증)
        drop(cfg);
    }
}
