// gateway/src/gandiva_quic.rs

use quinn::{Endpoint, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info};

pub async fn spawn_gandiva_quic_engine(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    // Generate self-signed cert for Gandiva-QUIC
    let cert = rcgen::generate_simple_self_signed(vec!["gandiva.local".into()])?;
    let cert_der = cert.cert.der().to_vec();
    let priv_key_der = cert.key_pair.serialize_der();

    let cert_chain = vec![CertificateDer::from(cert_der)];
    // In rustls 0.23, PrivateKeyDer is an enum (e.g. PrivateKeyDer::Pkcs8)
    let key = PrivateKeyDer::Pkcs8(priv_key_der.into());

    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    // Enable 0-RTT for Sliver Shot
    server_crypto.max_early_data_size = 0xFFFFFFFF; // Accept large early data
    server_crypto.alpn_protocols = vec![b"gandiva-8d".to_vec(), b"h3".to_vec()];

    let quic_server_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_server_crypto));
    let mut transport_config = quinn::TransportConfig::default();
    transport_config.max_idle_timeout(Some(quinn::IdleTimeout::try_from(Duration::from_secs(30))?));
    server_config.transport_config(Arc::new(transport_config));

    let addr = format!("0.0.0.0:{}", port).parse()?;
    let endpoint = Endpoint::server(server_config, addr)?;

    info!("🛡️ Gandiva-QUIC Engine (8D Sliver Shot) initialized on UDP {}", port);

    tokio::spawn(async move {
        while let Some(conn) = endpoint.accept().await {
            tokio::spawn(async move {
                match conn.await {
                    Ok(connection) => {
                        info!("🏹 Gandiva Connection Accepted! Processing 8D Slivers...");

                        loop {
                            match connection.accept_uni().await {
                                Ok(mut recv_stream) => {
                                    tokio::spawn(async move {
                                        let mut total_read = 0;
                                        let max_payload = 2 * 1024 * 1024; // 2MB Hard Limit
                                        let mut buf = vec![0; 4096];
                                        
                                        // [Kaleidoscope] 5s First Byte Timeout
                                        let read_result = tokio::time::timeout(
                                            Duration::from_secs(5),
                                            recv_stream.read(&mut buf)
                                        ).await;

                                        match read_result {
                                            Ok(Ok(Some(bytes_read))) => {
                                                total_read += bytes_read;
                                                if total_read > max_payload {
                                                    error!("[Kaleidoscope] QUIC Memory Bomb Blocked: {} bytes", total_read);
                                                    return;
                                                }
                                                // [Logic] Handle the 8D Sliver interpolation here
                                            }
                                            Ok(Err(e)) => error!("QUIC stream read error: {}", e),
                                            Err(_) => error!("QUIC stream read timeout (Slowloris blocked)"),
                                            _ => {}
                                        }
                                    });
                                }
                                Err(e) => {
                                    error!("Gandiva stream accept failed: {}", e);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Gandiva connection failed: {}", e);
                    }
                }
            });
        }
    });

    Ok(())
}
