use log::debug;

pub struct GetDeviceInfoRequestDecode {
    pub device_id: Option<u32>,
}

pub struct GetDeviceInfoRequest {
    pub device_id: u32,
}

impl TryFrom<GetDeviceInfoRequestDecode> for GetDeviceInfoRequest {
    type Error = minicbor::decode::Error;

    fn try_from(src: GetDeviceInfoRequestDecode) -> Result<Self, Self::Error> {
        let Some(id) = src.device_id else {
            return Err(minicbor::decode::Error::message("Missing device_id"));
        };
        Ok(GetDeviceInfoRequest { device_id: id })
    }
}

pub struct GetDeviceInfoResponse {
    pub firmware: Option<u32>,
    pub desired_firmware: u32,
    pub status: u8,
}

pub struct SetDeviceInfoRequestDecode {
    pub firmware: Option<u32>,
    pub status: Option<u8>,
    // None = field absent (no change), Some(None) = clear gateway, Some(Some(id)) = set gateway
    pub gateway_id: Option<Option<u32>>,
}

pub struct SetDeviceInfoRequest {
    pub firmware: u32,
    pub status: u8,
    pub gateway_id: Option<Option<u32>>,
}

impl TryFrom<SetDeviceInfoRequestDecode> for SetDeviceInfoRequest {
    type Error = minicbor::decode::Error;

    fn try_from(src: SetDeviceInfoRequestDecode) -> Result<Self, Self::Error> {
        let Some(fw) = src.firmware else {
            return Err(minicbor::decode::Error::message("Missing firmware"));
        };
        let Some(st) = src.status else {
            return Err(minicbor::decode::Error::message("Missing status"));
        };

        Ok(SetDeviceInfoRequest {
            firmware: fw,
            status: st,
            gateway_id: src.gateway_id,
        })
    }
}

pub struct SetDeviceInfoResponse {
    pub firmware: u32,
    pub desired_firmware: u32,
    pub status: u8,
}

pub fn decode_get_device_info_request(
    operation: &[u8],
) -> Result<GetDeviceInfoRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut parameter_request = GetDeviceInfoRequestDecode { device_id: None };
    debug!("Starting operation decoding");
    if decoder.array()? != Some(1) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 1",
        ));
    }
    parameter_request.device_id = Some(decoder.u32()?);

    parameter_request.try_into()
}

pub fn encode_get_device_info_response(
    device_info_response: &GetDeviceInfoResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = minicbor::Encoder::new(&mut buf);
    let _ = enc.array(3);
    if let Some(fw) = device_info_response.firmware {
        let _ = enc.u32(fw);
    } else {
        let _ = enc.null();
    }
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    Ok(buf)
}

pub fn decode_set_device_info_request(
    operation: &[u8],
) -> Result<SetDeviceInfoRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut req = SetDeviceInfoRequestDecode {
        firmware: None,
        status: None,
        gateway_id: None,
    };
    debug!("Starting operation decoding");
    let len = decoder.array()?;
    match len {
        Some(2) => {}
        Some(3) => {}
        _ => {
            return Err(minicbor::decode::Error::message(
                "Expected array of length 2 or 3",
            ));
        }
    }
    req.firmware = Some(decoder.u32()?);
    req.status = Some(decoder.u8()?);
    if len == Some(3) {
        req.gateway_id = Some(if decoder.datatype()? == minicbor::data::Type::Null {
            decoder.null()?;
            None
        } else {
            Some(decoder.u32()?)
        });
    }

    req.try_into()
}

pub fn encode_set_device_info_response(
    device_info_response: &SetDeviceInfoResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = minicbor::Encoder::new(&mut buf);
    let _ = enc.array(3);
    let _ = enc.u32(device_info_response.firmware);
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    Ok(buf)
}
