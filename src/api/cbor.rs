use crate::codec::operation::OperationType;
use crate::db::models::{
    Device, DeviceType, DeviceTypeFirmware, Firmware, NewDevice, NewDeviceType,
    NewDeviceTypeFirmware, NewFirmware,
};
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use log::{error, info};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::UdpSocket;
use tokio::select;
use tokio_util::sync::CancellationToken;

pub struct CborApiConfig {
    pub listen_address: SocketAddr,
    pub shared_pool: Arc<crate::DbPool>,
}

pub struct CborApi {
    config: CborApiConfig,
    joiner: Option<tokio::task::JoinHandle<()>>,
    cancel: CancellationToken,
}

impl CborApi {
    pub fn new(config: CborApiConfig) -> Self {
        CborApi {
            config: config,
            joiner: None,
            cancel: CancellationToken::new(),
        }
    }
    pub async fn start(&mut self) {
        let socket = UdpSocket::bind(self.config.listen_address).await.unwrap();
        let pool = self.config.shared_pool.clone();
        let cancel = self.cancel.clone();
        self.joiner = Some(tokio::spawn(async move {
            udp_loop(socket, pool.clone(), cancel.clone()).await
        }));
        info!(
            "CBOR listening on {}:{}",
            self.config.listen_address.ip(),
            self.config.listen_address.port()
        );
    }

    pub async fn shutdown(&mut self) {
        self.cancel.cancel();
        if self.joiner.is_some() {
            let handle = self.joiner.take().unwrap();
            let _ = handle.await;
        }
    }
}

async fn udp_loop(
    socket: UdpSocket,
    shared_pool: Arc<crate::DbPool>,
    cancellation_token: CancellationToken,
) {
    let mut buf = [0u8; 2048];

    loop {
        select! {
            res = socket.recv_from(&mut buf) => {
                let (len, addr) = match res {
                    Ok(v) => v,
                    Err(e) => {
                        error!("UDP recv error: {e}");
                        continue;
                    }
                };
                let cose_handler = crate::codec::cose::CoseHandler::new([0u8; 16].to_vec());
                let mut opcode: u16 = 0;
                let mut device_id: u32 = 0;

                let operation_bytes =
                    match cose_handler.decode_msg(&mut device_id, &mut opcode, &buf[..len]) {
                        Ok(op) => op,
                        Err(e) => {
                            error!("Failed to decode message from {addr}: {e}");
                            continue;
                        }
                    };

                let opcode_type = OperationType::from(opcode);

                match opcode_type {
                    OperationType::GetParameterRequest => {
                        let req = match crate::codec::operation::decode_get_parameter_request(&operation_bytes[..])
                        {
                            Ok(r) => r,
                            Err(e) => {
                                error!("Failed to decode operation from {addr}: {e}");
                                continue;
                            }
                        };
                        let param_id = req.parameter_id.unwrap();
                        let param_type = req.parameter_type.unwrap();
                        info!("UDP get_parameter for id={param_id}");

                        // Build a response (example)
                        let param_value: u64 = 42;
                        let response = crate::codec::operation::GetParameterResponse {
                            parameter_id: param_id,
                            parameter_type: param_type,
                            parameter_value: param_value.to_be_bytes().to_vec(),
                        };

                        let operation_buf = match crate::codec::operation::encode_get_parameter_response(&response)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode operation: {e}");
                                continue;
                            }
                        };

                        let response_buf = match cose_handler.encode_msg(
                            device_id,
                            crate::codec::operation::OperationType::GetParameterResponse as u16,
                            &operation_buf[..],
                        ) {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode COSE response: {e}");
                                continue;
                            }
                        };

                        // Send response
                        if let Err(e) = socket.send_to(&response_buf[..], addr).await {
                            error!("Failed to send to {addr}: {e}");
                        }
                    }
                    OperationType::GetDeviceInfoRequest => {
                        use crate::db::schema::device::dsl::*;

                        let mut conn = shared_pool
                            .clone()
                            .get_owned()
                            .await.unwrap();
                        let result = device
                            .select(Device::as_select())
                            .filter(id.eq(device_id as i32))
                            .first(&mut conn)
                            .await.unwrap();

                        let response = crate::codec::operation::GetDeviceInfoResponse {
                            firmware: result.firmware.unwrap() as u32,
                            desired_firmware: result.desired_firmware as u32,
                            status: result.status as u8,
                        };

                        let operation_buf = match crate::codec::operation::encode_get_device_info_response(&response)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode operation: {e}");
                                continue;
                            }
                        };

                        let response_buf = match cose_handler.encode_msg(
                            device_id,
                            crate::codec::operation::OperationType::GetParameterResponse as u16,
                            &operation_buf[..],
                        ) {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode COSE response: {e}");
                                continue;
                            }
                        };

                        // Send response
                        if let Err(e) = socket.send_to(&response_buf[..], addr).await {
                            error!("Failed to send to {addr}: {e}");
                        }
                    }
                    _ => {
                        error!("Unsupported opcode {opcode} from {addr}");
                        continue;
                    }
                }
            }
            _ = cancellation_token.cancelled() => {
                info!("UDP loop received shutdown; exiting");
                break;
            }
        }
    }
}
