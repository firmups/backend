use log::debug;
use minicbor::{Decoder, Encoder};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParameterType {
    Integer = 1,
    Boolean = 2,
    Float = 3,
    Double = 4,
    String = 5,
    Binary = 6,
}

impl TryFrom<u8> for ParameterType {
    type Error = minicbor::decode::Error;

    fn try_from(src: u8) -> Result<Self, Self::Error> {
        match src {
            1 => Ok(ParameterType::Integer),
            2 => Ok(ParameterType::Boolean),
            3 => Ok(ParameterType::Float),
            4 => Ok(ParameterType::Double),
            5 => Ok(ParameterType::String),
            6 => Ok(ParameterType::Binary),
            _ => return Err(minicbor::decode::Error::message("Unknown parameter type")),
        }
    }
}

impl From<ParameterType> for u8 {
    fn from(src: ParameterType) -> Self {
        match src {
            ParameterType::Integer => 1,
            ParameterType::Boolean => 2,
            ParameterType::Float => 3,
            ParameterType::Double => 4,
            ParameterType::String => 5,
            ParameterType::Binary => 6,
        }
    }
}

pub struct GetParameterRequest {
    pub parameter_id: u32,
    pub parameter_type: ParameterType,
}

pub struct GetParameterRequestDecode {
    pub parameter_id: Option<u32>,
    pub parameter_type: Option<ParameterType>,
}

impl TryFrom<GetParameterRequestDecode> for GetParameterRequest {
    type Error = minicbor::decode::Error;

    fn try_from(src: GetParameterRequestDecode) -> Result<Self, Self::Error> {
        let Some(id) = src.parameter_id else {
            return Err(minicbor::decode::Error::message("Missing parameter_id"));
        };
        let Some(p_ty) = src.parameter_type else {
            return Err(minicbor::decode::Error::message("Missing parameter_type"));
        };

        Ok(GetParameterRequest {
            parameter_id: id,
            parameter_type: p_ty,
        })
    }
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
    let mut parameter_request = GetParameterRequestDecode {
        parameter_id: None,
        parameter_type: None,
    };
    debug!("Starting operation decoding");
    if decoder.array()? != Some(2) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 2",
        ));
    }
    parameter_request.parameter_id = Some(decoder.u32()?);
    parameter_request.parameter_type = Some(decoder.u8()?.try_into()?);

    Ok(parameter_request.try_into()?)
}

pub fn encode_get_parameter_response(
    parameter_response: &GetParameterResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    // Encoding cannot fail as we are writing to a Vec
    let _ = enc.array(3);
    let _ = enc.u32(parameter_response.parameter_id);
    let _ = enc.u8(parameter_response.parameter_type.into());

    match parameter_response.parameter_type {
        ParameterType::Integer => {
            let int_bytes: [u8; 8] =
                parameter_response.parameter_value[..8]
                    .try_into()
                    .map_err(|_| {
                        minicbor::decode::Error::message(
                            "Expected 8 bytes for integer parameter value",
                        )
                    })?;
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
            let float_bytes: [u8; 4] =
                parameter_response.parameter_value[..4]
                    .try_into()
                    .map_err(|_| {
                        minicbor::decode::Error::message(
                            "Expected 4 bytes for float parameter value",
                        )
                    })?;
            let float_value = f32::from_be_bytes(float_bytes);
            let _ = enc.f32(float_value);
        }
        ParameterType::Double => {
            let double_bytes: [u8; 8] = parameter_response.parameter_value[..8]
                .try_into()
                .map_err(|_| {
                    minicbor::decode::Error::message("Expected 8 bytes for double parameter value")
                })?;
            let double_value = f64::from_be_bytes(double_bytes);
            let _ = enc.f64(double_value);
        }
        ParameterType::String => {
            let string_value =
                std::str::from_utf8(&parameter_response.parameter_value).map_err(|_| {
                    minicbor::decode::Error::message("Invalid UTF-8 in string parameter value")
                })?;
            let _ = enc.str(string_value);
        }
        ParameterType::Binary => {
            let _ = enc.bytes(&parameter_response.parameter_value);
        }
    }

    Ok(buf)
}
