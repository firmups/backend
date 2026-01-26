use log::{debug, error, info};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::select;
use tokio_util::sync::CancellationToken;

mod codec;
mod cose_handler;
mod operation_handler;

#[derive(Clone)]
pub struct CborApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
    pub data_storage_location: PathBuf,
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
                let mut cose_handler = cose_handler::CoseHandler::new(
                    config.shared_pool.clone(),
                );
                let operation_handler = operation_handler::OperationHandler::new(config.clone(), addr);
                let mut opcode: u16 = 0;
                let mut device_id: u32 = 0;

                let operation_bytes =
                    match cose_handler.decode_msg(&mut device_id, &mut opcode, &buf[..len]).await {
                        Ok(op) => op,
                        Err(_e) => {
                            error!("Failed to decode message from {addr}");//: {e}");
                            continue;
                        }
                    };

                let (opcode_response, operation_response) = operation_handler.handle_operation(device_id, opcode, &operation_bytes[..]).await;

                let response_buf = match cose_handler.encode_msg(opcode_response, &operation_response[..]).await {
                    Ok(b) => b,
                    Err(_e) => {
                        error!("Failed to encode COSE response");//: {e}");
                        continue;
                    }
                };
                if let Err(e) = socket.send_to(&response_buf[..], addr).await {
                    error!("Failed to send to {addr}: {e}");
                } else {
                    debug!("Sent response with opcode {opcode_response} to device {device_id} at {addr}");
                }
            }
            _ = cancellation_token.cancelled() => {
                debug!("UDP loop received shutdown; exiting");
                break;
            }
        }
    }
}
