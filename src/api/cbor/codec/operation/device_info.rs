use log::info;

pub struct GetDeviceInfoRequest {
    pub device_id: Option<u32>,
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

pub fn decode_get_device_info_request(
    operation: &[u8],
) -> Result<GetDeviceInfoRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut parameter_request = GetDeviceInfoRequest { device_id: None };
    info!("Starting operation decoding");
    if decoder.array()? != Some(1) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 1",
        ));
    }
    parameter_request.device_id = Some(decoder.u32()?);

    Ok(parameter_request)
}

pub fn encode_get_device_info_response(
    device_info_response: &GetDeviceInfoResponse,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = minicbor::Encoder::new(&mut buf);
    let _ = enc.array(3);
    let _ = enc.u32(device_info_response.firmware);
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    Ok(buf)
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
    let mut buf = Vec::with_capacity(256);
    let mut enc = minicbor::Encoder::new(&mut buf);
    let _ = enc.array(3);
    let _ = enc.u32(device_info_response.firmware);
    let _ = enc.u32(device_info_response.desired_firmware);
    let _ = enc.u8(device_info_response.status);

    Ok(buf)
}
