use log::info;

pub enum OperationType {
    Invalid = 0,
    GetParameterRequest = 1,
    GetParameterResponse = 2,
    SetParameterRequest = 3,
    SetParameterResponse = 4,
    GetFirmwareRequest = 5,
    GetFirmwareResponse = 6,
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
