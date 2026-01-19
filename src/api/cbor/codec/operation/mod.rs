pub mod device_info;
pub mod firmware;
pub mod operation_error;
// ToDo: re-enable parameter module when implementing
//pub mod parameter;

pub enum OperationError {
    InvalidOperation = 0,
    DecodingError = 1,
    EncodingError = 2,
    UnknownParameter = 3,
    DeviceNotFound = 4,
    FirmwareNotFound = 5,
    InternalError = 6,
}

impl From<u16> for OperationError {
    fn from(value: u16) -> Self {
        match value {
            0 => OperationError::InvalidOperation,
            1 => OperationError::DecodingError,
            2 => OperationError::EncodingError,
            3 => OperationError::UnknownParameter,
            4 => OperationError::DeviceNotFound,
            5 => OperationError::FirmwareNotFound,
            6 => OperationError::InternalError,
            _ => OperationError::InvalidOperation,
        }
    }
}

pub enum OperationType {
    Invalid = 0,
    Error = 1,
    GetParameterRequest = 2,
    GetParameterResponse = 3,
    SetParameterRequest = 4,
    SetParameterResponse = 5,
    GetDeviceInfoRequest = 6,
    GetDeviceInfoResponse = 7,
    SetDeviceInfoRequest = 8,
    SetDeviceInfoResponse = 9,
    GetFirmwareRequest = 10,
    GetFirmwareResponse = 11,
}

impl From<u16> for OperationType {
    fn from(value: u16) -> Self {
        match value {
            1 => OperationType::Error,
            2 => OperationType::GetParameterRequest,
            3 => OperationType::GetParameterResponse,
            4 => OperationType::SetParameterRequest,
            5 => OperationType::SetParameterResponse,
            6 => OperationType::GetDeviceInfoRequest,
            7 => OperationType::GetDeviceInfoResponse,
            8 => OperationType::SetDeviceInfoRequest,
            9 => OperationType::SetDeviceInfoResponse,
            10 => OperationType::GetFirmwareRequest,
            11 => OperationType::GetFirmwareResponse,
            _ => OperationType::Invalid,
        }
    }
}

impl From<OperationType> for u16 {
    fn from(op: OperationType) -> Self {
        op as u16
    }
}
