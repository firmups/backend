use log::{error, info};

pub mod device_info;
pub mod firmware;
pub mod parameter;

pub enum OperationType {
    Invalid = 0,
    GetParameterRequest = 1,
    GetParameterResponse = 2,
    SetParameterRequest = 3,
    SetParameterResponse = 4,
    GetDeviceInfoRequest = 5,
    GetDeviceInfoResponse = 6,
    SetDeviceInfoRequest = 7,
    SetDeviceInfoResponse = 8,
    GetFirmwareRequest = 9,
    GetFirmwareResponse = 10,
    ErrorResponse = 255,
}

impl From<u16> for OperationType {
    fn from(value: u16) -> Self {
        match value {
            1 => OperationType::GetParameterRequest,
            2 => OperationType::GetParameterResponse,
            3 => OperationType::SetParameterRequest,
            4 => OperationType::SetParameterResponse,
            5 => OperationType::GetDeviceInfoRequest,
            6 => OperationType::GetDeviceInfoResponse,
            7 => OperationType::SetDeviceInfoRequest,
            8 => OperationType::SetDeviceInfoResponse,
            9 => OperationType::GetFirmwareRequest,
            10 => OperationType::GetFirmwareResponse,
            255 => OperationType::ErrorResponse,
            _ => OperationType::Invalid,
        }
    }
}

impl From<OperationType> for u16 {
    fn from(op: OperationType) -> Self {
        op as u16
    }
}

pub enum OperationError {
    InvalidOperation,
    DecodingError,
    EncodingError,
    UnknownParameter,
    DeviceNotFound,
    FirmwareNotFound,
}
