pub fn encode_operation_error(error: super::OperationError) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = minicbor::Encoder::new(&mut buf);
    let _ = enc.array(1);
    let _ = enc.u16(error as u16);
    buf
}

pub fn decode_operation_error(
    operation: &[u8],
) -> Result<super::OperationError, minicbor::decode::Error> {
    let mut decoder = minicbor::Decoder::new(operation);
    if decoder.array()? != Some(1) {
        return Err(minicbor::decode::Error::message(
            "Expected error array of length 1",
        ));
    }
    let error = decoder.u16()?;

    Ok(error.into())
}
