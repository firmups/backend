use log::info;

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
            7 => OperationType::GetFirmwareRequest,
            8 => OperationType::GetFirmwareResponse,
            _ => OperationType::Invalid,
        }
    }
}

impl From<OperationType> for u16 {
    fn from(op: OperationType) -> Self {
        op as u16
    }
}

pub enum ParameterType {
    Integer = 1,
    Boolean = 2,
    Float = 3,
    Double = 4,
    String = 5,
    Binary = 6,
}

pub struct GetParameterRequest {
    pub parameter_id: Option<u32>,
    pub parameter_type: Option<ParameterType>,
}

pub struct GetParameterResponse {
    pub parameter_id: u32,
    pub parameter_type: ParameterType,
    pub parameter_value: Vec<u8>,
}

pub struct GetDeviceInfoResponse {
    pub firmware: u32,
    pub desired_firmware: u32,
    pub status: u8,
}

pub struct SetDeviceInfoRequest {
    pub firmware: Option<u32>,
    pub desired_firmware: Option<u32>,
    pub status: Option<u8>,
}

pub struct SetDeviceInfoResponse {
    pub firmware: u32,
    pub desired_firmware: u32,
    pub status: u8,
}

pub struct GetFirmwareRequest {
    pub firmware: Option<u32>,
    pub offset: Option<u32>,
    pub length: Option<u32>,
}

pub struct GetFirmwareResponse {
    pub firmware: u32,
    pub offset: u32,
    pub length: u32,
    pub data: Vec<u8>,
}

pub fn decode_get_parameter_request(
    operation: &[u8],
) -> Result<GetParameterRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut parameter_request = GetParameterRequest {
        parameter_id: None,
        parameter_type: None,
    };
    info!("Starting operation decoding");
    if decoder.array()? != Some(2) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 2",
        ));
    }
    parameter_request.parameter_id = Some(decoder.u32()?);
    parameter_request.parameter_type = Some(match decoder.u64()? {
        1 => ParameterType::Integer,
        2 => ParameterType::Boolean,
        3 => ParameterType::Float,
        4 => ParameterType::Double,
        5 => ParameterType::String,
        6 => ParameterType::Binary,
        _ => return Err(minicbor::decode::Error::message("Unknown parameter type")),
    });

    Ok(parameter_request)
}

pub fn encode_get_parameter_response(
    parameter_response: &GetParameterResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut cursor: minicbor::encode::write::Cursor<[u8; 1024]> =
        minicbor::encode::write::Cursor::new([0u8; 1024]);
    let mut enc = minicbor::Encoder::new(&mut cursor);
    enc.array(3);
    enc.u32(parameter_response.parameter_id);
    enc.u8(match parameter_response.parameter_type {
        ParameterType::Integer => 1,
        ParameterType::Boolean => 2,
        ParameterType::Float => 3,
        ParameterType::Double => 4,
        ParameterType::String => 5,
        ParameterType::Binary => 6,
        _ => return Err(minicbor::decode::Error::message("Unknown parameter type")),
    });

    match parameter_response.parameter_type {
        ParameterType::Integer => {
            let int_bytes: [u8; 8] = parameter_response.parameter_value[..8]
                .try_into()
                .expect("Slice with incorrect length");
            let int_value = u64::from_be_bytes(int_bytes);
            info!("Int value {}", int_value);
            enc.u64(int_value);
        }
        ParameterType::Boolean => {
            let bool_byte = parameter_response.parameter_value[0];
            let bool_value = match bool_byte {
                0 => false,
                1 => true,
                _ => false,
            };
            enc.bool(bool_value);
        }
        ParameterType::Float => {
            let float_bytes: [u8; 4] = parameter_response.parameter_value[..4]
                .try_into()
                .expect("Slice with incorrect length");
            let float_value = f32::from_be_bytes(float_bytes);
            enc.f32(float_value);
        }
        ParameterType::Double => {
            let double_bytes: [u8; 8] = parameter_response.parameter_value[..8]
                .try_into()
                .expect("Slice with incorrect length");
            let double_value = f64::from_be_bytes(double_bytes);
            enc.f64(double_value);
        }
        ParameterType::String => {
            let string_value = std::str::from_utf8(&parameter_response.parameter_value).unwrap();
            enc.str(string_value);
        }
        ParameterType::Binary => {
            enc.bytes(&parameter_response.parameter_value);
        }
    }

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}

pub fn encode_get_device_info_response(
    device_info_response: &GetDeviceInfoResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut cursor: minicbor::encode::write::Cursor<[u8; 1024]> =
        minicbor::encode::write::Cursor::new([0u8; 1024]);
    let mut enc = minicbor::Encoder::new(&mut cursor);
    let _ = enc.array(3);
    let _ = enc.u32(device_info_response.firmware);
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}

pub fn decode_set_device_info_request(
    operation: &[u8],
) -> Result<SetDeviceInfoRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut set_device_info_request = SetDeviceInfoRequest {
        firmware: None,
        desired_firmware: None,
        status: None,
    };
    info!("Starting operation decoding");
    if decoder.array()? != Some(3) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 3",
        ));
    }
    set_device_info_request.firmware = Some(decoder.u32()?);
    set_device_info_request.desired_firmware = Some(decoder.u32()?);
    set_device_info_request.status = Some(decoder.u8()?);

    Ok(set_device_info_request)
}

pub fn encode_set_device_info_response(
    device_info_response: &SetDeviceInfoResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    // ToDO: implement
    let mut cursor: minicbor::encode::write::Cursor<[u8; 1024]> =
        minicbor::encode::write::Cursor::new([0u8; 1024]);
    let mut enc = minicbor::Encoder::new(&mut cursor);
    let _ = enc.array(3);
    let _ = enc.u32(device_info_response.firmware);
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}

pub fn decode_get_firmware_request(
    operation: &[u8],
) -> Result<GetFirmwareRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut firmware_request = GetFirmwareRequest {
        firmware: None,
        offset: None,
        length: None,
    };
    info!("Starting operation decoding");
    if decoder.array()? != Some(3) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 3",
        ));
    }
    firmware_request.firmware = Some(decoder.u32()?);
    firmware_request.offset = Some(decoder.u32()?);
    firmware_request.length = Some(decoder.u32()?);

    Ok(firmware_request)
}

pub fn encode_get_firmware_response(
    firmware_response: &GetFirmwareResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut cursor: minicbor::encode::write::Cursor<[u8; 1024]> =
        minicbor::encode::write::Cursor::new([0u8; 1024]);
    let mut enc = minicbor::Encoder::new(&mut cursor);
    let _ = enc.array(4);
    let _ = enc.u32(firmware_response.firmware);
    let _ = enc.u32(firmware_response.offset);
    let _ = enc.u32(firmware_response.length);
    let _ = enc.bytes(&firmware_response.data);

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}
