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

    pub async fn handle_operation(
        &self,
        device_id: u32,
        opcode: u16,
        operation: &[u8],
    ) -> Result<Vec<u8>, operation::OperationError> {
        let opcode_type = operation::OperationType::from(opcode);
        let response_buf: Vec<u8>;

        match opcode_type {
            operation::OperationType::GetParameterRequest => {
                let req = match operation::parameter::decode_get_parameter_request(&operation[..]) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to decode operation from {}: {}", self.addr, e);
                        return Err(operation::OperationError::DecodingError);
                    }
                };
                let param_id = req.parameter_id.unwrap();
                let param_type = req.parameter_type.unwrap();
                info!("UDP get_parameter for id={param_id}");

                // Build a response (example)
                let param_value: u64 = 42;
                let response = operation::parameter::GetParameterResponse {
                    parameter_id: param_id,
                    parameter_type: param_type,
                    parameter_value: param_value.to_be_bytes().to_vec(),
                };

                response_buf = match operation::parameter::encode_get_parameter_response(&response)
                {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to encode operation: {e}");
                        return Err(operation::OperationError::EncodingError);
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
                            return Err(operation::OperationError::DecodingError);
                        }
                    };

                let mut conn = self.config.shared_pool.clone().get_owned().await.unwrap();
                let result = device
                    .select(Device::as_select())
                    .filter(id.eq(req.device_id.unwrap() as i32))
                    .first(&mut conn)
                    .await
                    .unwrap();

                let response = operation::device_info::GetDeviceInfoResponse {
                    firmware: result.firmware.unwrap() as u32,
                    desired_firmware: result.desired_firmware as u32,
                    status: result.status as u8,
                };

                response_buf =
                    match operation::device_info::encode_get_device_info_response(&response) {
                        Ok(b) => b,
                        Err(e) => {
                            error!("Failed to encode operation: {e}");
                            return Err(operation::OperationError::EncodingError);
                        }
                    };
            }
            operation::OperationType::GetFirmwareRequest => {
                use crate::db::schema::firmware::dsl::*;

                let req = match operation::firmware::decode_get_firmware_request(&operation[..]) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to decode operation from {}: {}", self.addr, e);
                        return Err(operation::OperationError::DecodingError);
                    }
                };

                let mut conn = self.config.shared_pool.clone().get_owned().await.unwrap();
                let result = firmware
                    .select(Firmware::as_select())
                    .filter(id.eq(req.firmware.unwrap() as i32))
                    .first(&mut conn)
                    .await
                    .unwrap();

                let safe_name = format!("{}.bin", result.file_id);
                let mut path = self.config.data_storage_location.clone();
                path.push("firmware");
                path.push(safe_name);

                let mut file = fs::File::open(path).await.unwrap();
                file.seek(io::SeekFrom::Start(req.offset.unwrap() as u64))
                    .await
                    .unwrap();

                //ToDo: Dangerous!!
                let mut buf = vec![0u8; req.length.unwrap() as usize];
                let read = file.read(&mut buf).await.unwrap();
                buf.truncate(read);

                let response = operation::firmware::GetFirmwareResponse {
                    firmware: result.id as u32,
                    offset: req.offset.unwrap() as u32,
                    length: read as u32,
                    data: buf,
                };

                response_buf = match operation::firmware::encode_get_firmware_response(&response) {
                    Ok(b) => b,
                    Err(e) => {
                        error!("Failed to encode operation: {e}");
                        return Err(operation::OperationError::EncodingError);
                    }
                }
            }
            _ => {
                error!("Unsupported opcode {} from {}", opcode, self.addr);
                return Err(operation::OperationError::InvalidOperation);
            }
        }
        return Ok(response_buf);
    }
}
