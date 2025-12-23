use log::{debug, error, info};
use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::encode::write::Cursor;

use crate::crypto::crypto_aes;
use crate::crypto::crypto_ascon;

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
    device_id: Option<u32>,
    opcode: Option<u16>,
    encryption_algorithm: Option<EncryptionAlgorithm>,
    nonce: Option<Vec<u8>>,
}

pub struct CoseHandler {
    key_bytes: Vec<u8>,
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
        let mut decoder = Decoder::new(&msg);
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

        let encrypted_operation_buffer = decoder.bytes()?;
        let nonce_bytes: &[u8] = &protected_header.nonce.unwrap();
        if matches!(
            protected_header.encryption_algorithm,
            Some(EncryptionAlgorithm::Aes128Gcm)
        ) {
            let res = crypto_aes::decrypt_operation_aes(
                encrypted_operation_buffer,
                protected_header_buffer,
                &nonce_bytes,
                &self.key_bytes,
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
                &self.key_bytes,
            );
            match res {
                Ok(vec) => {
                    debug!(
                        "Decoded COSE message with opcode {:?}",
                        protected_header.opcode
                    );
                    *device_id = protected_header.device_id.unwrap();
                    *opcode = protected_header.opcode.unwrap();
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

    pub fn encode_msg(
        &self,
        device_id: u32,
        operation_id: u16,
        operation: &[u8],
    ) -> Result<Vec<u8>, minicbor::decode::Error> {
        let mut cursor: Cursor<[u8; 1024]> = Cursor::new([0u8; 1024]);
        let mut enc = Encoder::new(&mut cursor);

        let mut nonce = [0u8; 16];
        let random_res = getrandom::fill(&mut nonce);
        match random_res {
            Ok(()) => (),
            Err(_) => {
                return Err(minicbor::decode::Error::message("Randomness failed"));
            }
        }

        let protected_header = ProtectedHeader {
            device_id: Some(device_id),
            opcode: Some(operation_id),
            encryption_algorithm: Some(EncryptionAlgorithm::AsconAead128),
            nonce: Some(nonce.to_vec()),
        };

        let protected_header_buf = encode_protected_header(protected_header)?;
        debug!("protected header size: {}", protected_header_buf.len());
        let crypto_result = crypto_ascon::encrypt_operation_ascon(
            operation,
            &protected_header_buf,
            &nonce,
            &self.key_bytes,
        );
        let ciphertext = match crypto_result {
            Ok(ct) => ct,
            Err(_) => {
                return Err(minicbor::decode::Error::message("Encryption failed"));
            }
        };
        debug!("Ciphertext size: {}", ciphertext.len());

        enc.array(3).unwrap();
        enc.bytes(&protected_header_buf).unwrap();
        enc.map(0).unwrap();
        enc.bytes(&ciphertext).unwrap();

        let pos = cursor.position() as usize;
        let inner = cursor.into_inner();

        Ok(inner[..pos].to_vec())
    }
}

fn encode_protected_header(
    protected_header: ProtectedHeader,
) -> Result<Vec<u8>, minicbor::decode::Error> {
    let mut cursor: Cursor<[u8; 256]> = Cursor::new([0u8; 256]);
    let mut enc = Encoder::new(&mut cursor);

    enc.map(5);
    enc.u16(ProtectedHeaderKey::EncryptionAlgorithm as u16);
    enc.u16(protected_header.encryption_algorithm.unwrap() as u16);
    enc.u16(ProtectedHeaderKey::DeviceId as u16);
    enc.u32(protected_header.device_id.unwrap());
    enc.u16(ProtectedHeaderKey::Opcode as u16);
    enc.u16(protected_header.opcode.unwrap());
    enc.u16(ProtectedHeaderKey::EncryptionNonce as u16);
    enc.bytes(&protected_header.nonce.unwrap());
    enc.u16(ProtectedHeaderKey::CriticalHeaderList as u16);
    enc.array(2);
    enc.u16(ProtectedHeaderKey::DeviceId as u16);
    enc.u16(ProtectedHeaderKey::Opcode as u16);

    let pos = cursor.position() as usize;
    let inner = cursor.into_inner();

    Ok(inner[..pos].to_vec())
}

fn decode_protected_header(
    protected_header_buf: &[u8],
) -> Result<ProtectedHeader, minicbor::decode::Error> {
    let mut decoder = Decoder::new(&protected_header_buf);
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
