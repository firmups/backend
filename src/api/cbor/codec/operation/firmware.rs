pub struct GetFirmwareRequestDecode {
    pub firmware: Option<u32>,
    pub offset: Option<u32>,
    pub length: Option<u32>,
}

pub struct GetFirmwareRequest {
    pub firmware: u32,
    pub offset: u32,
    pub length: u32,
}

impl TryFrom<GetFirmwareRequestDecode> for GetFirmwareRequest {
    type Error = minicbor::decode::Error;

    fn try_from(src: GetFirmwareRequestDecode) -> Result<Self, Self::Error> {
        let Some(fw) = src.firmware else {
            return Err(minicbor::decode::Error::message("Missing firmware"));
        };
        let Some(off) = src.offset else {
            return Err(minicbor::decode::Error::message("Missing offset"));
        };
        let Some(len) = src.length else {
            return Err(minicbor::decode::Error::message("Missing length"));
        };

        Ok(GetFirmwareRequest {
            firmware: fw,
            offset: off,
            length: len,
        })
    }
}

pub struct GetFirmwareResponse {
    pub firmware: u32,
    pub offset: u32,
    pub length: u32,
    pub data: Vec<u8>,
}

pub fn decode_get_firmware_request(
    operation: &[u8],
) -> Result<GetFirmwareRequest, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    let mut firmware_request = GetFirmwareRequestDecode {
        firmware: None,
        offset: None,
        length: None,
    };
    if decoder.array()? != Some(3) {
        return Err(minicbor::decode::Error::message(
            "Expected firmware request array of length 3",
        ));
    }
    firmware_request.firmware = Some(decoder.u32()?);
    firmware_request.offset = Some(decoder.u32()?);
    firmware_request.length = Some(decoder.u32()?);

    firmware_request.try_into()
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

    let pos = cursor.position();
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}
