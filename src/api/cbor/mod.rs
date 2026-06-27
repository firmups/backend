use log::{debug, error, info};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::select;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

mod codec;
mod cose_handler;
mod operation_handler;

/// Upper bound on datagrams processed concurrently. The receive loop hands each
/// datagram to its own task; this caps how many are in flight so a flood can't
/// spawn unbounded tasks. Acts as backpressure: once saturated the loop waits
/// for a permit before reading the next datagram.
const MAX_INFLIGHT: usize = 2048;

#[derive(Clone)]
pub struct CborApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
    pub storage: Arc<Box<dyn crate::storage::Storage>>,
}

pub struct CborApi {
    config: CborApiConfig,
    joiner: Option<tokio::task::JoinHandle<()>>,
    cancel: CancellationToken,
}

impl CborApi {
    pub fn new(config: CborApiConfig) -> Self {
        CborApi {
            config,
            joiner: None,
            cancel: CancellationToken::new(),
        }
    }
    pub async fn start(&mut self) {
        let socket = UdpSocket::bind(self.config.listen_address)
            .await
            .expect("Failed to bind UDP socket");
        let cancel = self.cancel.clone();
        let config = self.config.clone();
        self.joiner = Some(tokio::spawn(async move {
            udp_loop(socket, config, cancel).await
        }));
        info!(
            "CBOR listening on {}:{}/UDP",
            self.config.listen_address.ip(),
            self.config.listen_address.port()
        );
    }

    pub async fn shutdown(&mut self) {
        self.cancel.cancel();
        if self.joiner.is_some() {
            let handle = self.joiner.take().expect("Failed to take join handle");
            let _ = handle.await;
        }
    }
}

async fn udp_loop(socket: UdpSocket, config: CborApiConfig, cancellation_token: CancellationToken) {
    // Shared so each per-datagram task can send its own response concurrently.
    let socket = Arc::new(socket);
    // Caps concurrent in-flight requests; provides backpressure under a flood.
    let limiter = Arc::new(Semaphore::new(MAX_INFLIGHT));
    let mut buf = [0u8; 2048];
    loop {
        select! {
            res = socket.recv_from(&mut buf[..]) => {
                let (len, addr) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        error!("UDP recv error: {e}");
                        continue;
                    }
                };

                // Acquire a permit before spawning. When MAX_INFLIGHT requests are
                // already in flight this awaits here, pausing recv as backpressure.
                let permit = match limiter.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        error!("Concurrency limiter closed: {e}");
                        continue;
                    }
                };

                // Hand the datagram to its own task and immediately loop back to
                // recv_from, so requests are processed concurrently instead of one
                // at a time. `buf` is reused, so copy out the bytes first.
                let data = buf[..len].to_vec();
                let socket = socket.clone();
                let config = config.clone();
                tokio::spawn(async move {
                    let _permit = permit; // released when this task finishes
                    let mut cose_handler = cose_handler::CoseHandler::new(
                        config.shared_pool.clone(),
                    );
                    let operation_handler = operation_handler::OperationHandler::new(config.clone(), addr);
                    let mut opcode: u16 = 0;
                    let mut device_id: u32 = 0;

                    let operation_bytes =
                        match cose_handler.decode_msg(&mut device_id, &mut opcode, &data[..]).await {
                            Ok(op) => op,
                            Err(_e) => {
                                error!("Failed to decode message from {addr}");//: {e}");
                                return;
                            }
                        };

                    let (opcode_response, operation_response) = operation_handler.handle_operation(device_id, opcode, &operation_bytes[..]).await;

                    let response_buf = match cose_handler.encode_msg(opcode_response, &operation_response[..]).await {
                        Ok(b) => b,
                        Err(_e) => {
                            error!("Failed to encode COSE response");//: {e}");
                            return;
                        }
                    };
                    if let Err(e) = socket.send_to(&response_buf[..], addr).await {
                        error!("Failed to send to {addr}: {e}");
                    } else {
                        debug!("Sent response with opcode {opcode_response} to device {device_id} at {addr}");
                    }
                });
            }
            _ = cancellation_token.cancelled() => {
                debug!("UDP loop received shutdown; exiting");
                break;
            }
        }
    }
}
