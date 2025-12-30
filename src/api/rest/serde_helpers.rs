use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Deserializer, Serializer};

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
