use crate::api::cbor::codec::crypto;
use log::debug;
use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::encode::write::Cursor;
use zeroize::Zeroize;

enum EncryptionAlgorithm {
    Aes128Gcm = 1,
    AsconAead128 = 35,
}

enum ProtectedHeaderKey {
    EncryptionAlgorithm = 1,
    CriticalHeaderList = 2,
    EncryptionNonce = 5,
    DeviceId = 8608,
    Opcode = 8633,
    Unknown = 65535,
}

impl From<u16> for ProtectedHeaderKey {
    fn from(header_key: u16) -> Self {
        match header_key {
            1 => ProtectedHeaderKey::EncryptionAlgorithm,
            2 => ProtectedHeaderKey::CriticalHeaderList,
            5 => ProtectedHeaderKey::EncryptionNonce,
            8608 => ProtectedHeaderKey::DeviceId,
            8633 => ProtectedHeaderKey::Opcode,
            _ => ProtectedHeaderKey::Unknown,
        }
    }
}

enum CoseAlgorithmIdentifier {
    Aes128Gcm = 1,
    AsconAead128 = 35,
    Unknown,
}

impl From<u16> for CoseAlgorithmIdentifier {
    fn from(header_key: u16) -> Self {
        match header_key {
            1 => CoseAlgorithmIdentifier::Aes128Gcm,
            35 => CoseAlgorithmIdentifier::AsconAead128,
            _ => CoseAlgorithmIdentifier::Unknown,
        }
    }
}

struct ProtectedHeaderDecode {
    device_id: Option<u32>,
    opcode: Option<u16>,
    encryption_algorithm: Option<EncryptionAlgorithm>,
    nonce: Option<Vec<u8>>,
}

struct ProtectedHeader {
    device_id: u32,
    opcode: u16,
    encryption_algorithm: EncryptionAlgorithm,
    nonce: Vec<u8>,
}

impl TryFrom<ProtectedHeaderDecode> for ProtectedHeader {
    type Error = minicbor::decode::Error;
    fn try_from(src: ProtectedHeaderDecode) -> Result<Self, Self::Error> {
        let ProtectedHeaderDecode {
            device_id: Some(device_id),
            opcode: Some(opcode),
            encryption_algorithm: Some(encryption_algorithm),
            nonce: Some(nonce),
        } = src
        else {
            return Err(minicbor::decode::Error::message(
                "Missing required protected header fields",
            ));
        };
        Ok(Self {
            device_id,
            opcode,
            encryption_algorithm,
            nonce,
        })
    }
}

// pub struct CoseHandler {
//     key_bytes: Option<Vec<u8>>,
//     key_for_device_callback: fn(u32) -> Vec<u8>,
// }

// impl CoseHandler {
//     pub fn new(key_for_device_callback: fn(u32) -> Vec<u8>) -> Self {
//         CoseHandler {
//             key_bytes: None,
//             key_for_device_callback,
//         }
//     }

pub struct CoseHandler {
    key_bytes: Vec<u8>,
}

impl Drop for CoseHandler {
    fn drop(&mut self) {
        self.key_bytes.zeroize();
    }
}

impl CoseHandler {
    pub fn new(key_bytes: Vec<u8>) -> Self {
        CoseHandler { key_bytes }
    }

    pub fn decode_msg(
        &self,
        device_id: &mut u32,
        opcode: &mut u16,
        msg: &[u8],
    ) -> Result<Vec<u8>, minicbor::decode::Error> {
        let mut decoder = Decoder::new(msg);
        if decoder.array()? != Some(3) {
            return Err(minicbor::decode::Error::message(
                "Expected cose array of length 3",
            ));
        }

        let protected_header_buffer = decoder.bytes()?;
        let protected_header_decode = decode_protected_header(protected_header_buffer)?;
        let protected_header = ProtectedHeader::try_from(protected_header_decode)?;

        if decoder.map()? != Some(0) {
            return Err(minicbor::decode::Error::message(
                "Expected empty unprotected header",
            ));
        }

        let encrypted_operation_buffer = decoder.bytes()?;
        match protected_header.encryption_algorithm {
            EncryptionAlgorithm::Aes128Gcm => {
                let res = crypto::crypto_aes::decrypt_operation_aes(
                    encrypted_operation_buffer,
                    protected_header_buffer,
                    &protected_header.nonce,
                    &self.key_bytes,
                );
                match res {
                    Ok(vec) => {
                        *device_id = protected_header.device_id;
                        *opcode = protected_header.opcode;
                        return Ok(vec);
                    }
                    Err(_) => {
                        return Err(minicbor::decode::Error::message("Decryption failed"));
                    }
                }
            }
            EncryptionAlgorithm::AsconAead128 => {
                let res = crypto::crypto_ascon::decrypt_operation_ascon(
                    encrypted_operation_buffer,
                    protected_header_buffer,
                    &protected_header.nonce,
                    &self.key_bytes,
                );
                match res {
                    Ok(vec) => {
                        debug!(
                            "Decoded COSE message with opcode {:?}",
                            protected_header.opcode
                        );
                        *device_id = protected_header.device_id;
                        *opcode = protected_header.opcode;
                        return Ok(vec);
                    }
                    Err(_) => {
                        return Err(minicbor::decode::Error::message("Decryption failed"));
                    }
                }
            }
        }
    }

    pub fn encode_msg(
        &self,
        device_id: u32,
        operation_id: u16,
        operation: &[u8],
    ) -> Result<Vec<u8>, minicbor::encode::Error<minicbor::encode::write::EndOfArray>> {
        let mut cursor: Cursor<[u8; 1024]> = Cursor::new([0u8; 1024]);
        let mut enc = Encoder::new(&mut cursor);

        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce[..])
            .map_err(|_| minicbor::encode::Error::message("Randomness failed"))?;

        let protected_header = ProtectedHeader {
            device_id: device_id,
            opcode: operation_id,
            encryption_algorithm: EncryptionAlgorithm::AsconAead128,
            nonce: nonce.to_vec(),
        };

        let protected_header_buf = encode_protected_header(protected_header)?;
        debug!("protected header size: {}", protected_header_buf.len());
        let crypto_result = crypto::crypto_ascon::encrypt_operation_ascon(
            operation,
            &protected_header_buf,
            &nonce[..],
            &self.key_bytes,
        );
        let ciphertext = match crypto_result {
            Ok(ct) => ct,
            Err(_) => {
                return Err(minicbor::encode::Error::message("Encryption failed"));
            }
        };
        debug!("Ciphertext size: {}", ciphertext.len());

        enc.array(3)?;
        enc.bytes(&protected_header_buf)?;
        enc.map(0)?;
        enc.bytes(&ciphertext)?;

        let pos = cursor.position() as usize;
        let inner = cursor.into_inner();

        Ok(inner[..pos].to_vec())
    }
}

fn encode_protected_header(
    protected_header: ProtectedHeader,
) -> Result<Vec<u8>, minicbor::encode::Error<minicbor::encode::write::EndOfArray>> {
    let mut cursor: Cursor<[u8; 256]> = Cursor::new([0u8; 256]);
    let mut enc = Encoder::new(&mut cursor);

    enc.map(5)?;
    enc.u16(ProtectedHeaderKey::EncryptionAlgorithm as u16)?;
    enc.u16(protected_header.encryption_algorithm as u16)?;
    enc.u16(ProtectedHeaderKey::DeviceId as u16)?;
    enc.u32(protected_header.device_id)?;
    enc.u16(ProtectedHeaderKey::Opcode as u16)?;
    enc.u16(protected_header.opcode)?;
    enc.u16(ProtectedHeaderKey::EncryptionNonce as u16)?;
    enc.bytes(&protected_header.nonce[..])?;
    enc.u16(ProtectedHeaderKey::CriticalHeaderList as u16)?;
    enc.array(2)?;
    enc.u16(ProtectedHeaderKey::DeviceId as u16)?;
    enc.u16(ProtectedHeaderKey::Opcode as u16)?;

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}

fn decode_protected_header(
    protected_header_buf: &[u8],
) -> Result<ProtectedHeaderDecode, minicbor::decode::Error> {
    let mut decoder = Decoder::new(&protected_header_buf);
    let map_size = decoder.map()?.unwrap_or(0);

    let mut header = ProtectedHeaderDecode {
        device_id: None,
        opcode: None,
        encryption_algorithm: None,
        nonce: None,
    };
    for _ in 0..map_size {
        let key = decoder.u16()?;
        match ProtectedHeaderKey::from(key) {
            ProtectedHeaderKey::DeviceId => header.device_id = Some(decoder.u32()?),
            ProtectedHeaderKey::Opcode => header.opcode = Some(decoder.u16()?),
            ProtectedHeaderKey::EncryptionAlgorithm => {
                let alg = decoder.u16()?;
                header.encryption_algorithm = match CoseAlgorithmIdentifier::from(alg) {
                    CoseAlgorithmIdentifier::Aes128Gcm => Some(EncryptionAlgorithm::Aes128Gcm),
                    CoseAlgorithmIdentifier::AsconAead128 => {
                        Some(EncryptionAlgorithm::AsconAead128)
                    }
                    _ => {
                        return Err(minicbor::decode::Error::message(
                            "Unknown encryption algorithm",
                        ));
                    }
                };
            }
            ProtectedHeaderKey::EncryptionNonce => header.nonce = Some(decoder.bytes()?.to_vec()),
            ProtectedHeaderKey::CriticalHeaderList => {
                let critical_header_list_size = decoder.array()?.unwrap_or(0);
                for _ in 0..critical_header_list_size {
                    let header_id = decoder.u16()?;
                    match ProtectedHeaderKey::from(header_id) {
                        ProtectedHeaderKey::DeviceId | ProtectedHeaderKey::Opcode => {}
                        _ => {
                            return Err(minicbor::decode::Error::message(
                                "Unknown critical header key",
                            ));
                        }
                    }
                }
            }
            _ => {
                return Err(minicbor::decode::Error::message(
                    "Unknown protected header key",
                ));
            }
        }
    }

    Ok(header)
}
