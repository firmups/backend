use axum::http::header;
use minicbor::Decoder;

pub mod crypto_aes;
pub mod crypto_ascon;

enum EncryptionAlgorithm {
    Aes128Gcm = 1,
    AsconAead128 = 2,
}

enum ProtectedHeaderKey {
    EncryptionAlgorithm = 1,
    CriticalHeaderList = 2,
    EncryptionNonce = 5,
    DeviceId = 8608,
    Opcode = 8633,
    Unknown,
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

struct ProtectedHeader {
    device_id: Option<u16>,
    opcode: Option<u16>,
    encryption_algorithm: Option<EncryptionAlgorithm>,
    nonce: Option<Vec<u8>>,
}

fn decode_protected_header(input: &[u8]) -> Result<ProtectedHeader, minicbor::decode::Error> {
    let mut decoder = Decoder::new(&input);
    let map_size = decoder.map()?.unwrap_or(0);

    let mut header = ProtectedHeader {
        device_id: None,
        opcode: None,
        encryption_algorithm: None,
        nonce: None,
    };
    for _ in 0..map_size {
        let key = decoder.u16()?;
        match ProtectedHeaderKey::from(key) {
            ProtectedHeaderKey::DeviceId => header.device_id = Some(decoder.u16()?),
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

pub fn decode_msg(input: &[u8]) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut decoder = Decoder::new(&input);
    if decoder.array()? != Some(3) {
        return Err(minicbor::decode::Error::message(
            "Expected cose array of length 3",
        ));
    }

    let protected_header_buffer = decoder.bytes()?;
    let protected_header = decode_protected_header(protected_header_buffer)?;
    if protected_header.encryption_algorithm.is_none()
        || protected_header.nonce.is_none()
        || protected_header.device_id.is_none()
        || protected_header.opcode.is_none()
    {
        return Err(minicbor::decode::Error::message(
            "Missing required protected header fields",
        ));
    }

    if decoder.map()? != Some(0) {
        return Err(minicbor::decode::Error::message(
            "Expected empty unprotected header",
        ));
    }

    let mut encrypted_operation_buffer = decoder.bytes()?;
    let nonce_bytes: &[u8] = &protected_header.nonce.unwrap();
    let key_bytes: [u8; 16] = [0u8; 16];
    if matches!(
        protected_header.encryption_algorithm,
        Some(EncryptionAlgorithm::Aes128Gcm)
    ) {
        let res = crypto_aes::decrypt_operation_aes(
            encrypted_operation_buffer,
            protected_header_buffer,
            &nonce_bytes,
            &key_bytes,
        );
        match res {
            Ok(vec) => {
                return Ok(vec);
            }
            Err(_) => {
                return Err(minicbor::decode::Error::message("Decryption failed"));
            }
        }
    } else if matches!(
        protected_header.encryption_algorithm,
        Some(EncryptionAlgorithm::AsconAead128)
    ) {
        let res = crypto_ascon::decrypt_operation_ascon(
            encrypted_operation_buffer,
            protected_header_buffer,
            &nonce_bytes,
            &key_bytes,
        );
        match res {
            Ok(vec) => {
                return Ok(vec);
            }
            Err(_) => {
                return Err(minicbor::decode::Error::message("Decryption failed"));
            }
        }
    } else {
        Err(minicbor::decode::Error::message(
            "Unsupported encryption algorithm",
        ))
    }
}
