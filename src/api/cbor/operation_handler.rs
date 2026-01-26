use crate::api::cbor;
use crate::api::cbor::codec::operation;
use crate::db::models::{Device, DeviceStatus, Firmware, UpdateDevice};
use diesel::ExpressionMethods;
use diesel::SelectableHelper;
use diesel::query_dsl::methods::{FilterDsl, FindDsl, SelectDsl};
use diesel::result::DatabaseErrorKind;
use diesel_async::RunQueryDsl;
use log::{error, info, warn};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::{fs, io};

pub struct OperationHandler {
    config: cbor::CborApiConfig,
    addr: std::net::SocketAddr,
}

impl TryFrom<u8> for DeviceStatus {
    type Error = minicbor::decode::Error;
    fn try_from(src: u8) -> Result<Self, Self::Error> {
        match src {
            0 => Ok(DeviceStatus::Active),
            1 => Ok(DeviceStatus::Inactive),
            2 => Ok(DeviceStatus::Maintenance),
            _ => Err(minicbor::decode::Error::message(format!(
                "Unknown device status {}",
                src
            ))),
        }
    }
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
    ) -> (u16, Vec<u8>) {
        let opcode_type = operation::OperationType::from(opcode);
        let response_buf: (u16, Vec<u8>);

        match opcode_type {
            // ToDo: Implement parameter handling
            // operation::OperationType::GetParameterRequest => {
            //     let req = match operation::parameter::decode_get_parameter_request(&operation[..]) {
            //         Ok(r) => r,
            //         Err(e) => {
            //             error!("Failed to decode operation from {}: {}", self.addr, e);
            //             return self
            //                 .handle_error_operation(operation::OperationError::DecodingError);
            //         }
            //     };
            //     info!("UDP get_parameter for id={}", req.parameter_id);

            //     // Build a response (example)
            //     let param_value: u64 = 42;
            //     let response = operation::parameter::GetParameterResponse {
            //         parameter_id: req.parameter_id,
            //         parameter_type: req.parameter_type,
            //         parameter_value: param_value.to_be_bytes().to_vec(),
            //     };

            //     response_buf = match operation::parameter::encode_get_parameter_response(&response)
            //     {
            //         Ok(b) => b,
            //         Err(e) => {
            //             error!("Failed to encode operation: {e}");
            //             return self
            //                 .handle_error_operation(operation::OperationError::EncodingError);
            //         }
            //     };
            // }
            operation::OperationType::GetDeviceInfoRequest => {
                use crate::db::schema::device::dsl::*;

                let req = match operation::device_info::decode_get_device_info_request(operation) {
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

                let fw = result.firmware.map(|fw| fw as u32);
                info!("get_device_info request from device={}", req.device_id);
                let response = operation::device_info::GetDeviceInfoResponse {
                    firmware: fw,
                    desired_firmware: result.desired_firmware as u32,
                    status: result.status as u8,
                };

                response_buf =
                    match operation::device_info::encode_get_device_info_response(&response) {
                        Ok(b) => (operation::OperationType::GetDeviceInfoResponse as u16, b),
                        Err(e) => {
                            error!("Failed to encode operation: {e}");
                            return self
                                .handle_error_operation(operation::OperationError::EncodingError);
                        }
                    };
            }
            operation::OperationType::SetDeviceInfoRequest => {
                use crate::db::schema::device::dsl::*;

                let req = match operation::device_info::decode_set_device_info_request(operation) {
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

                let ds: DeviceStatus = match req.status.try_into() {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Invalid device status from {}: {}", self.addr, e);
                        return self
                            .handle_error_operation(operation::OperationError::InvalidOperation);
                    }
                };

                let payload = UpdateDevice {
                    firmware: Some(req.firmware as i32),
                    desired_firmware: None,
                    status: Some(ds),
                    name: None,
                    type_: None,
                };

                // Perform the insert and return the created row
                let result: Result<Device, (u16, Vec<u8>)> = match diesel::update(
                    device.find(device_id as i32),
                )
                .set(&payload)
                .returning(Device::as_returning())
                .get_result(&mut conn)
                .await
                {
                    Ok(d) => Ok(d),
                    Err(diesel::result::Error::DatabaseError(
                        DatabaseErrorKind::ForeignKeyViolation,
                        info,
                    )) => {
                        // Optional: check which constraint failed for more specific messages.
                        match info.constraint_name() {
                            Some("fk_device_type") => {
                                warn!("Foreign key violation: unknown device type");
                                Err(self.handle_error_operation(
                                    operation::OperationError::InternalError,
                                ))
                            }
                            Some("fk_firmware") => {
                                warn!("Foreign key violation: unknown firmware");
                                Err(self.handle_error_operation(
                                    operation::OperationError::InternalError,
                                ))
                            }
                            Some("fk_desired_firmware") => {
                                warn!("Foreign key violation: unknown desired firmware");
                                Err(self.handle_error_operation(
                                    operation::OperationError::InternalError,
                                ))
                            }
                            Some("fk_device_type_current") => {
                                warn!(
                                    "Foreign key violation: device type has no link to current firmware"
                                );
                                Err(self.handle_error_operation(
                                    operation::OperationError::InternalError,
                                ))
                            }
                            Some("fk_device_type_desired") => {
                                warn!(
                                    "Foreign key violation: device type has no link to desired firmware"
                                );
                                Err(self.handle_error_operation(
                                    operation::OperationError::InternalError,
                                ))
                            }
                            _ => Err(self
                                .handle_error_operation(operation::OperationError::InternalError)),
                        }
                    }
                    Err(diesel::result::Error::DatabaseError(
                        DatabaseErrorKind::UniqueViolation,
                        _info,
                    )) => {
                        warn!("Unique constraint violation when updating device");
                        Err(self.handle_error_operation(operation::OperationError::InternalError))
                    }
                    Err(diesel::result::Error::NotFound) => {
                        warn!("Device {} not found", device_id);
                        Err(self.handle_error_operation(operation::OperationError::InternalError))
                    }
                    Err(e) => {
                        warn!("Unhandled database error for device {}: {}", device_id, e);
                        Err(self.handle_error_operation(operation::OperationError::InternalError))
                    }
                };
                let result = match result {
                    Ok(r) => r,
                    Err(b) => return b,
                };

                let Some(fw) = result.firmware else {
                    error!("Firmware missing after update for device {}", device_id);
                    return self.handle_error_operation(operation::OperationError::InternalError);
                };

                info!(
                    "Device {} set its firmware to {} and its status to {:?}",
                    device_id, req.firmware, ds
                );
                let response = operation::device_info::SetDeviceInfoResponse {
                    firmware: fw as u32,
                    desired_firmware: result.desired_firmware as u32,
                    status: result.status as u8,
                };

                response_buf =
                    match operation::device_info::encode_set_device_info_response(&response) {
                        Ok(b) => (operation::OperationType::SetDeviceInfoResponse as u16, b),
                        Err(e) => {
                            error!("Failed to encode operation: {e}");
                            return self
                                .handle_error_operation(operation::OperationError::EncodingError);
                        }
                    };
            }
            operation::OperationType::GetFirmwareRequest => {
                use crate::db::schema::firmware::dsl::*;

                let req = match operation::firmware::decode_get_firmware_request(operation) {
                    Ok(r) => r,
                    Err(e) => {
                        error!("Failed to decode operation from {}: {}", self.addr, e);
                        return self
                            .handle_error_operation(operation::OperationError::DecodingError);
                    }
                };
                if req.offset == 0 {
                    info!(
                        "Device {} started download of firmware {}",
                        device_id, req.firmware
                    );
                }

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

                if (req.length as usize) > 1024 * 1024 {
                    error!("Requested length too large: {}", req.length);
                    return self
                        .handle_error_operation(operation::OperationError::InvalidOperation);
                }
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

                if (read as u32) < req.length {
                    info!(
                        "Device {} finished downloading firmware {}",
                        device_id, req.firmware
                    );
                }

                let response = operation::firmware::GetFirmwareResponse {
                    firmware: result.id as u32,
                    offset: req.offset as u32,
                    length: read as u32,
                    data: buf,
                };

                response_buf = match operation::firmware::encode_get_firmware_response(&response) {
                    Ok(b) => (operation::OperationType::GetFirmwareResponse as u16, b),
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
        response_buf
    }

    fn handle_error_operation(&self, error: operation::OperationError) -> (u16, Vec<u8>) {
        (
            operation::OperationType::Error as u16,
            operation::operation_error::encode_operation_error(error),
        )
    }
}
