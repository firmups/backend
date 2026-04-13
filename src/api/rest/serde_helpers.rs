use crate::db::models::ParameterType;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serializer};
use serde_json::Value as JsonValue;

pub fn as_base64<S>(bytes: &Vec<u8>, ser: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = STANDARD.encode(bytes);
    ser.serialize_str(&s)
}

pub fn from_base64<'de, D>(de: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(de)?;
    STANDARD
        .decode(s.as_bytes())
        .map_err(serde::de::Error::custom)
}

pub fn parameter_to_json_value(type_: ParameterType, bytes: Vec<u8>) -> Result<JsonValue, String> {
    match type_ {
        ParameterType::String => String::from_utf8(bytes)
            .map(JsonValue::String)
            .map_err(|e| format!("stored string value is not valid UTF-8: {e}")),
        ParameterType::Integer => {
            let arr: [u8; 8] = bytes
                .try_into()
                .map_err(|_| "stored integer value must be exactly 8 bytes".to_string())?;
            Ok(JsonValue::Number(i64::from_be_bytes(arr).into()))
        }
        ParameterType::Float => {
            let arr: [u8; 8] = bytes
                .try_into()
                .map_err(|_| "stored float value must be exactly 8 bytes".to_string())?;
            serde_json::Number::from_f64(f64::from_be_bytes(arr))
                .map(JsonValue::Number)
                .ok_or_else(|| "stored float value is non-finite".to_string())
        }
        ParameterType::Boolean => {
            if bytes.is_empty() {
                return Err("stored boolean value must be exactly 1 byte".to_string());
            }
            Ok(JsonValue::Bool(bytes[0] != 0))
        }
        ParameterType::Binary => Ok(JsonValue::String(STANDARD.encode(&bytes))),
    }
}

pub fn json_value_to_parameter(type_: ParameterType, value: JsonValue) -> Result<Vec<u8>, String> {
    match type_ {
        ParameterType::String => match value {
            JsonValue::String(s) => Ok(s.into_bytes()),
            _ => Err("expected a string value for a String parameter".to_string()),
        },
        ParameterType::Integer => match value {
            JsonValue::Number(n) => n
                .as_i64()
                .map(|i| i.to_be_bytes().to_vec())
                .ok_or_else(|| "expected an integer value for an Integer parameter".to_string()),
            _ => Err("expected a number value for an Integer parameter".to_string()),
        },
        ParameterType::Float => match value {
            JsonValue::Number(n) => n
                .as_f64()
                .map(|f| f.to_be_bytes().to_vec())
                .ok_or_else(|| "expected a float value for a Float parameter".to_string()),
            _ => Err("expected a number value for a Float parameter".to_string()),
        },
        ParameterType::Boolean => match value {
            JsonValue::Bool(b) => Ok(vec![if b { 1u8 } else { 0u8 }]),
            _ => Err("expected a boolean value for a Boolean parameter".to_string()),
        },
        ParameterType::Binary => match value {
            JsonValue::String(s) => STANDARD
                .decode(s.as_bytes())
                .map_err(|e| format!("invalid base64 for Binary parameter: {e}")),
            _ => Err("expected a base64 string value for a Binary parameter".to_string()),
        },
    }
}
