use log::info;
use minicbor::{Decoder, Encoder};

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
    let mut decoder = Decoder::new(operation);
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
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    // Encoding cannot fail as we are writing to a Vec
    let _ = enc.array(3);
    let _ = enc.u32(parameter_response.parameter_id);
    let _ = enc.u8(match parameter_response.parameter_type {
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
            let _ = enc.u64(int_value);
        }
        ParameterType::Boolean => {
            let bool_byte = parameter_response.parameter_value[0];
            let bool_value = match bool_byte {
                0 => false,
                1 => true,
                _ => false,
            };
            let _ = enc.bool(bool_value);
        }
        ParameterType::Float => {
            let float_bytes: [u8; 4] = parameter_response.parameter_value[..4]
                .try_into()
                .expect("Slice with incorrect length");
            let float_value = f32::from_be_bytes(float_bytes);
            let _ = enc.f32(float_value);
        }
        ParameterType::Double => {
            let double_bytes: [u8; 8] = parameter_response.parameter_value[..8]
                .try_into()
                .expect("Slice with incorrect length");
            let double_value = f64::from_be_bytes(double_bytes);
            let _ = enc.f64(double_value);
        }
        ParameterType::String => {
            let string_value = std::str::from_utf8(&parameter_response.parameter_value).unwrap();
            let _ = enc.str(string_value);
        }
        ParameterType::Binary => {
            let _ = enc.bytes(&parameter_response.parameter_value);
        }
    }

    Ok(buf)
}
