use crate::api::cbor;
use crate::api::cbor::codec::operation;
use crate::db::models::{Device, Firmware};
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, SelectDsl};
use diesel_async::RunQueryDsl;
use log::{error, info};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::{fs, io};

pub struct OperationHandler {
    config: cbor::CborApiConfig,
    addr: std::net::SocketAddr,
}

impl OperationHandler {
    pub fn new(config: cbor::CborApiConfig, addr: std::net::SocketAddr) -> Self {
        OperationHandler { config, addr }
    }

    pub async fn handle_operation(&self, device_id: u32, opcode: u16, operation: &[u8]) -> Vec<u8> {
        let opcode_type = operation::OperationType::from(opcode);
        let response_buf: Vec<u8>;

        match opcode_type {
            operation::OperationType::GetParameterRequest => {
                let req = match operation::parameter::decode_get_parameter_request(&operation[..]) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to decode operation from {}: {}", self.addr, e);
                        return self
                            .handle_error_operation(operation::OperationError::DecodingError);
                    }
                };
                info!("UDP get_parameter for id={}", req.parameter_id);

                // Build a response (example)
                let param_value: u64 = 42;
                let response = operation::parameter::GetParameterResponse {
                    parameter_id: req.parameter_id,
                    parameter_type: req.parameter_type,
                    parameter_value: param_value.to_be_bytes().to_vec(),
                };

                response_buf = match operation::parameter::encode_get_parameter_response(&response)
                {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to encode operation: {e}");
                        return self
                            .handle_error_operation(operation::OperationError::EncodingError);
                    }
                };
            }
            operation::OperationType::GetDeviceInfoRequest => {
                use crate::db::schema::device::dsl::*;

                let req =
                    match operation::device_info::decode_get_device_info_request(&operation[..]) {
                        Ok(r) => r,
                        Err(e) => {
                            error!("Failed to decode operation from {}: {}", self.addr, e);
                            return self
                                .handle_error_operation(operation::OperationError::DecodingError);
                        }
                    };

                let mut conn = match self.config.shared_pool.clone().get_owned().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to get DB connection: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };
                let result = match device
                    .select(Device::as_select())
                    .filter(id.eq(req.device_id as i32))
                    .first(&mut conn)
                    .await
                {
                    Ok(r) => r,
                    Err(diesel::result::Error::NotFound) => {
                        error!("Device {} not found", req.device_id);
                        return self
                            .handle_error_operation(operation::OperationError::DeviceNotFound);
                    }
                    Err(e) => {
                        error!("Failed to query device: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };

                let fw = if result.firmware.is_some() {
                    Some(result.firmware.expect("Firmware must be some") as u32)
                } else {
                    None
                };

                let response = operation::device_info::GetDeviceInfoResponse {
                    firmware: fw,
                    desired_firmware: result.desired_firmware as u32,
                    status: result.status as u8,
                };

                response_buf =
                    match operation::device_info::encode_get_device_info_response(&response) {
                        Ok(b) => b,
                        Err(e) => {
                            error!("Failed to encode operation: {e}");
                            return self
                                .handle_error_operation(operation::OperationError::EncodingError);
                        }
                    };
            }
            operation::OperationType::GetFirmwareRequest => {
                use crate::db::schema::firmware::dsl::*;

                let req = match operation::firmware::decode_get_firmware_request(&operation[..]) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to decode operation from {}: {}", self.addr, e);
                        return self
                            .handle_error_operation(operation::OperationError::DecodingError);
                    }
                };

                let mut conn = match self.config.shared_pool.clone().get_owned().await {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to get DB connection: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };
                let result = match firmware
                    .select(Firmware::as_select())
                    .filter(id.eq(req.firmware as i32))
                    .first(&mut conn)
                    .await
                {
                    Ok(r) => r,
                    Err(diesel::result::Error::NotFound) => {
                        error!("Firmware {} not found", req.firmware);
                        return self
                            .handle_error_operation(operation::OperationError::FirmwareNotFound);
                    }
                    Err(e) => {
                        error!("Failed to query firmware: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };

                let safe_name = format!("{}.bin", result.file_id);
                let mut path = self.config.data_storage_location.clone();
                path.push("firmware");
                path.push(safe_name);

                let mut file = match fs::File::open(path).await {
                    Ok(f) => f,
                    Err(e) => {
                        error!("Failed to open firmware file: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };
                match file.seek(io::SeekFrom::Start(req.offset as u64)).await {
                    Ok(_) => {}
                    Err(e) => {
                        error!("Failed to seek firmware file: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                }

                //ToDo: Dangerous!!
                let mut buf = vec![0u8; req.length as usize];
                let read = match file.read(&mut buf).await {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to read firmware file: {}", e);
                        return self
                            .handle_error_operation(operation::OperationError::InternalError);
                    }
                };
                buf.truncate(read);

                let response = operation::firmware::GetFirmwareResponse {
                    firmware: result.id as u32,
                    offset: req.offset as u32,
                    length: read as u32,
                    data: buf,
                };

                response_buf = match operation::firmware::encode_get_firmware_response(&response) {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to encode operation: {e}");
                        return self
                            .handle_error_operation(operation::OperationError::EncodingError);
                    }
                }
            }
            _ => {
                error!("Unsupported opcode {} from {}", opcode, self.addr);
                return self.handle_error_operation(operation::OperationError::InvalidOperation);
            }
        }
        return response_buf;
    }

    fn handle_error_operation(&self, error: operation::OperationError) -> Vec<u8> {
        operation::operation_error::encode_operation_error(error)
    }
}
