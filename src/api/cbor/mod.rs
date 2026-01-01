use crate::db::models::LightweightKeyDetails;
use crate::db::models::{Device, DeviceKey, Firmware, KeyStatus};
use codec::cose;
use codec::operation;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use log::{error, info};
use std::path::PathBuf;
use std::pin::Pin;
use std::{net::SocketAddr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::UdpSocket;
use tokio::{fs, io, select};
use tokio_util::sync::CancellationToken;

mod codec;

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

pub enum CborApiInternalError {
    DecodeError,
    EncodeError,
    EncryptionError,
    DecryptionError,
}

pub enum CborApiError {
    InternalError(CborApiInternalError),
    InvalidMessage,
    UnsupportedOperation,
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
        let cancel = self.cancel.clone();
        let config = self.config.clone();
        self.joiner = Some(tokio::spawn(async move {
            udp_loop(socket, config, cancel).await
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

#[derive(Clone)]
struct DbKeyProvider {
    shared_pool: Arc<crate::DbPool>,
}

impl codec::cose::KeyProvider for DbKeyProvider {
    fn key_for_device(
        &self,
        device_id: u32,
        key_type: codec::cose::KeyType,
    ) -> Pin<
        Box<dyn Future<Output = Result<Vec<u8>, codec::cose::KeyProviderError>> + Send + 'static>,
    > {
        let pool = Arc::clone(&self.shared_pool);
        Box::pin(async move {
            use crate::db::schema::device_key::dsl as device_key_dsl;
            use crate::db::schema::lightweight_key_details::dsl as details_dsl;
            let mut conn = pool
                .get_owned()
                .await
                .map_err(|_| codec::cose::KeyProviderError::DbError)?;

            let (active_key, details): (DeviceKey, LightweightKeyDetails) =
                device_key_dsl::device_key
                    .inner_join(details_dsl::lightweight_key_details)
                    .filter(device_key_dsl::device.eq(device_id as i32))
                    .filter(device_key_dsl::status.eq(KeyStatus::ACTIVE))
                    .select((DeviceKey::as_select(), LightweightKeyDetails::as_select()))
                    .first(&mut conn)
                    .await
                    .map_err(|e| match e {
                        diesel::result::Error::NotFound => {
                            codec::cose::KeyProviderError::KeyNotFound
                        }
                        _ => codec::cose::KeyProviderError::DbError,
                    })?;
            if active_key.key_type != crate::db::models::KeyType::LIGHTWEIGHT {
                return Err(codec::cose::KeyProviderError::KeyMismatch);
            }
            match details.algorithm {
                crate::db::models::CryptoAlgorithm::AesGcm128 => match key_type {
                    codec::cose::KeyType::AesGcm128 => Ok(details.key),
                    _ => Err(codec::cose::KeyProviderError::KeyMismatch),
                },
                crate::db::models::CryptoAlgorithm::AsconAead128 => match key_type {
                    codec::cose::KeyType::AsconAead128 => Ok(details.key),
                    _ => Err(codec::cose::KeyProviderError::KeyMismatch),
                },
            }
        })
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
                let mut cose_handler = cose::CoseHandler::new(Box::new(DbKeyProvider {
                    shared_pool: config.shared_pool.clone(),
                }));
                let mut opcode: u16 = 0;
                let mut device_id: u32 = 0;

                let operation_bytes =
                    match cose_handler.decode_msg(&mut device_id, &mut opcode, &buf[..len]).await {
                        Ok(op) => op,
                        Err(e) => {
                            error!("Failed to decode message from {addr}: {e}");
                            continue;
                        }
                    };

                let opcode_type = operation::OperationType::from(opcode);

                match opcode_type {
                    operation::OperationType::GetParameterRequest => {
                        let req = match operation::decode_get_parameter_request(&operation_bytes[..])
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
                        let response = operation::GetParameterResponse {
                            parameter_id: param_id,
                            parameter_type: param_type,
                            parameter_value: param_value.to_be_bytes().to_vec(),
                        };

                        let operation_buf = match operation::encode_get_parameter_response(&response)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode operation: {e}");
                                continue;
                            }
                        };

                        let response_buf = match cose_handler.encode_msg(
                            operation::OperationType::GetParameterResponse as u16,
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
                        } else {
                            info!("Sent GetParameterResponse");
                        }
                    }
                    operation::OperationType::GetDeviceInfoRequest => {
                        use crate::db::schema::device::dsl::*;

                        let req = match operation::decode_get_device_info_request(&operation_bytes[..])
                        {
                            Ok(r) => r,
                            Err(e) => {
                                error!("Failed to decode operation from {addr}: {e}");
                                continue;
                            }
                        };

                        let mut conn = config.shared_pool
                            .clone()
                            .get_owned()
                            .await.unwrap();
                        let result = device
                            .select(Device::as_select())
                            .filter(id.eq(req.device_id.unwrap() as i32))
                            .first(&mut conn)
                            .await.unwrap();

                        let response = operation::GetDeviceInfoResponse {
                            firmware: result.firmware.unwrap() as u32,
                            desired_firmware: result.desired_firmware as u32,
                            status: result.status as u8,
                        };

                        let operation_buf = match operation::encode_get_device_info_response(&response)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode operation: {e}");
                                continue;
                            }
                        };

                        let response_buf = match cose_handler.encode_msg(
                            operation::OperationType::GetDeviceInfoResponse as u16,
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
                        } else {
                            info!("Sent GetDeviceInfoResponse");
                        }
                    }
                    operation::OperationType::GetFirmwareRequest => {
                        use crate::db::schema::firmware::dsl::*;

                        let req = match operation::decode_get_firmware_request(&operation_bytes[..])
                        {
                            Ok(r) => r,
                            Err(e) => {
                                error!("Failed to decode operation from {addr}: {e}");
                                continue;
                            }
                        };

                        let mut conn = config.shared_pool
                            .clone()
                            .get_owned()
                            .await.unwrap();
                        let result = firmware
                            .select(Firmware::as_select())
                            .filter(id.eq(req.firmware.unwrap() as i32))
                            .first(&mut conn)
                            .await.unwrap();

                        let safe_name = format!("{}.bin", result.file_id);
                        let mut path = config.data_storage_location.clone();
                        path.push("firmware");
                        path.push(safe_name);

                        let mut file = fs::File::open(path).await.unwrap();
                        file.seek(io::SeekFrom::Start(req.offset.unwrap() as u64)).await.unwrap();

                        //ToDo: Dangerous!!
                        let mut buf = vec![0u8; req.length.unwrap() as usize];
                        let read = file.read(&mut buf).await.unwrap();
                        buf.truncate(read);

                        let response = operation::GetFirmwareResponse {
                            firmware: result.id as u32,
                            offset: req.offset.unwrap() as u32,
                            length: read as u32,
                            data: buf,
                        };

                        let operation_buf = match operation::encode_get_firmware_response(&response)
                        {
                            Ok(b) => b,
                            Err(e) => {
                                error!("Failed to encode operation: {e}");
                                continue;
                            }
                        };

                        let response_buf = match cose_handler.encode_msg(
                            operation::OperationType::GetFirmwareResponse as u16,
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
                        } else {
                            info!("Sent GetFirmwareResponse");
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
