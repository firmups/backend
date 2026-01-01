use crate::api::cbor::codec::crypto;
use log::debug;
use minicbor::Decoder;
use minicbor::Encoder;
use minicbor::encode::write::Cursor;
use std::collections::HashMap;
use std::{future::Future, pin::Pin};
use zeroize::Zeroize;

pub enum KeyType {
    AesGcm128,
    AsconAead128,
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

#[derive(Clone, Copy)]
enum CoseAlgorithmIdentifier {
    AesGcm128 = 1,
    AsconAead128 = 35,
    Unknown,
}

impl From<u16> for CoseAlgorithmIdentifier {
    fn from(header_key: u16) -> Self {
        match header_key {
            1 => CoseAlgorithmIdentifier::AesGcm128,
            35 => CoseAlgorithmIdentifier::AsconAead128,
            _ => CoseAlgorithmIdentifier::Unknown,
        }
    }
}

struct ProtectedHeaderDecode {
    device_id: Option<u32>,
    opcode: Option<u16>,
    encryption_algorithm: Option<CoseAlgorithmIdentifier>,
    nonce: Option<Vec<u8>>,
}

struct ProtectedHeader {
    device_id: u32,
    opcode: u16,
    encryption_algorithm: CoseAlgorithmIdentifier,
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

impl TryFrom<CoseAlgorithmIdentifier> for crypto::CryptoAlgorithm {
    type Error = minicbor::decode::Error;
    fn try_from(src: CoseAlgorithmIdentifier) -> Result<Self, Self::Error> {
        match src {
            CoseAlgorithmIdentifier::AesGcm128 => Ok(crypto::CryptoAlgorithm::AesGcm128),
            CoseAlgorithmIdentifier::AsconAead128 => Ok(crypto::CryptoAlgorithm::AsconAead128),
            CoseAlgorithmIdentifier::Unknown => Err(minicbor::decode::Error::message(
                "Unknown COSE algorithm identifier",
            )),
        }
    }
}

impl From<crypto::CryptoAlgorithm> for CoseAlgorithmIdentifier {
    fn from(src: crypto::CryptoAlgorithm) -> CoseAlgorithmIdentifier {
        match src {
            crypto::CryptoAlgorithm::AesGcm128 => CoseAlgorithmIdentifier::AesGcm128,
            crypto::CryptoAlgorithm::AsconAead128 => CoseAlgorithmIdentifier::AsconAead128,
        }
    }
}

pub enum KeyProviderError {
    KeyMismatch,
    KeyNotFound,
    DbError,
}

pub trait KeyProvider: Send + Sync {
    fn key_for_device(
        &self,
        device_id: u32,
        key_type: KeyType,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, KeyProviderError>> + Send + 'static>>;
}

pub struct CoseHandler {
    device_id: Option<u32>,
    key_provider: Box<dyn KeyProvider>,
    algs: HashMap<crypto::CryptoAlgorithm, Box<dyn crypto::CryptoAead>>,
    crypto_algo: Option<crypto::CryptoAlgorithm>,
    key_bytes: Option<Vec<u8>>,
}

impl Drop for CoseHandler {
    fn drop(&mut self) {
        self.key_bytes.zeroize();
    }
}

impl CoseHandler {
    pub fn new(key_provider: Box<dyn KeyProvider>) -> Self {
        let mut algs: HashMap<crypto::CryptoAlgorithm, Box<dyn crypto::CryptoAead>> =
            HashMap::new();
        algs.insert(
            crypto::CryptoAlgorithm::AsconAead128,
            Box::new(crypto::crypto_ascon::CryptoAsconAead128),
        );
        algs.insert(
            crypto::CryptoAlgorithm::AesGcm128,
            Box::new(crypto::crypto_aes::CryptoAes128Gcm),
        );

        CoseHandler {
            device_id: None,
            key_provider: key_provider,
            key_bytes: None,
            algs: algs,
            crypto_algo: None,
        }
    }

    pub async fn decode_msg(
        &mut self,
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
        let crypto_algo_identifier: crypto::CryptoAlgorithm;
        let crypto_key_type: KeyType;
        match protected_header.encryption_algorithm {
            CoseAlgorithmIdentifier::AesGcm128 => {
                crypto_algo_identifier = crypto::CryptoAlgorithm::AesGcm128;
                crypto_key_type = KeyType::AesGcm128;
            }
            CoseAlgorithmIdentifier::AsconAead128 => {
                crypto_algo_identifier = crypto::CryptoAlgorithm::AsconAead128;
                crypto_key_type = KeyType::AsconAead128;
            }
            CoseAlgorithmIdentifier::Unknown => {
                return Err(minicbor::decode::Error::message(
                    "Unsupported encryption algorithm",
                ));
            }
        };

        self.crypto_algo = Some(protected_header.encryption_algorithm.try_into()?);
        let crypto_alg = self
            .algs
            .get(&crypto_algo_identifier)
            .ok_or_else(|| minicbor::decode::Error::message("Algorithm not configured"))?;
        if protected_header.nonce.len() != crypto_alg.nonce_len() {
            return Err(minicbor::decode::Error::message("Invalid nonce length"));
        }

        if decoder.map()? != Some(0) {
            return Err(minicbor::decode::Error::message(
                "Expected empty unprotected header",
            ));
        }

        let encrypted_operation_buffer = decoder.bytes()?;
        if encrypted_operation_buffer.len() < crypto_alg.tag_len() {
            return Err(minicbor::decode::Error::message(
                "Ciphertext too short for tag",
            ));
        }

        let key_bytes = match &self.key_bytes {
            Some(key_bytes) => key_bytes,
            _ => {
                let device_key = (self
                    .key_provider
                    .key_for_device(protected_header.device_id, crypto_key_type))
                .await
                .map_err(|e| {
                    minicbor::decode::Error::message(match e {
                        KeyProviderError::KeyMismatch => format!(
                            "Key type mismatch for device {}",
                            protected_header.device_id
                        ),
                        KeyProviderError::KeyNotFound => {
                            format!("No key found for device {}", protected_header.device_id)
                        }
                        KeyProviderError::DbError => "Key store error".to_string(),
                    })
                })?;

                self.key_bytes = Some(device_key);
                self.key_bytes.as_ref().unwrap()
            }
        };

        let pt = crypto_alg
            .decrypt(
                &key_bytes,
                &protected_header.nonce,
                protected_header_buffer,
                encrypted_operation_buffer,
            )
            .map_err(|_| minicbor::decode::Error::message("Decryption failed"))?;

        self.device_id = Some(protected_header.device_id);
        *device_id = protected_header.device_id;
        *opcode = protected_header.opcode;
        debug!(
            "Decrypted operation with opcode: {}",
            protected_header.opcode
        );
        Ok(pt)
    }

    pub fn encode_msg(
        &self,
        operation_id: u16,
        operation: &[u8],
    ) -> Result<Vec<u8>, minicbor::encode::Error<minicbor::encode::write::EndOfArray>> {
        let Some(crypto_alg_identifier) = self.crypto_algo else {
            return Err(minicbor::encode::Error::message(
                "Message needs to be decoded first to determine crypto algorithm",
            ));
        };
        let Some(key_bytes) = &self.key_bytes else {
            return Err(minicbor::encode::Error::message(
                "Key bytes not set for encryption",
            ));
        };
        let Some(device_id) = self.device_id else {
            return Err(minicbor::encode::Error::message(
                "Device ID not set for encryption",
            ));
        };

        let mut cursor: Cursor<[u8; 1024]> = Cursor::new([0u8; 1024]);
        let mut enc = Encoder::new(&mut cursor);
        let crypto_alg = self
            .algs
            .get(&crypto_alg_identifier)
            .ok_or_else(|| minicbor::encode::Error::message("Algorithm not configured"))?;
        let mut nonce = vec![0u8; crypto_alg.nonce_len()];
        getrandom::fill(&mut nonce[..])
            .map_err(|_| minicbor::encode::Error::message("Randomness failed"))?;
        let protected_header = ProtectedHeader {
            device_id: device_id,
            opcode: operation_id,
            encryption_algorithm: crypto_alg_identifier.into(),
            nonce: nonce.to_vec(),
        };

        let protected_header_buf = encode_protected_header(protected_header);
        debug!("protected header size: {}", protected_header_buf.len());
        let ct = crypto_alg
            .encrypt(&key_bytes, &nonce, &protected_header_buf, operation)
            .map_err(|_| minicbor::encode::Error::message("Encryption failed"))?;
        debug!("Ciphertext size: {}", ct.len());

        enc.array(3)?;
        enc.bytes(&protected_header_buf)?;
        enc.map(0)?;
        enc.bytes(&ct)?;

        let pos = cursor.position() as usize;
        let inner = cursor.into_inner();

        debug!("Encrypted operation with opcode: {}", operation_id);
        Ok(inner[..pos].to_vec())
    }
}

fn encode_protected_header(protected_header: ProtectedHeader) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    enc.map(5);
    enc.u16(ProtectedHeaderKey::EncryptionAlgorithm as u16);
    enc.u16(protected_header.encryption_algorithm as u16);
    enc.u16(ProtectedHeaderKey::DeviceId as u16);
    enc.u32(protected_header.device_id);
    enc.u16(ProtectedHeaderKey::Opcode as u16);
    enc.u16(protected_header.opcode);
    enc.u16(ProtectedHeaderKey::EncryptionNonce as u16);
    enc.bytes(&protected_header.nonce[..]);
    enc.u16(ProtectedHeaderKey::CriticalHeaderList as u16);
    enc.array(2);
    enc.u16(ProtectedHeaderKey::DeviceId as u16);
    enc.u16(ProtectedHeaderKey::Opcode as u16);

    buf
}

fn decode_protected_header(
    protected_header_buf: &[u8],
) -> Result<ProtectedHeaderDecode, minicbor::decode::Error> {
    let mut decoder = Decoder::new(&protected_header_buf);
    let map_size = decoder.map()?;
    let mut header_count: u64 = 0;

    let mut header = ProtectedHeaderDecode {
        device_id: None,
        opcode: None,
        encryption_algorithm: None,
        nonce: None,
    };
    loop {
        // Map can be either infinite length (none) or fixed length
        if let Some(limit) = map_size {
            if header_count >= limit {
                break;
            }
            header_count += 1;
        } else {
            if decoder.datatype()? == minicbor::data::Type::Break {
                decoder.skip()?;
                break;
            }
        }

        let header_key = decoder.u16()?;
        match ProtectedHeaderKey::from(header_key) {
            ProtectedHeaderKey::DeviceId => header.device_id = Some(decoder.u32()?),
            ProtectedHeaderKey::Opcode => header.opcode = Some(decoder.u16()?),
            ProtectedHeaderKey::EncryptionAlgorithm => {
                let alg = decoder.u16()?;
                header.encryption_algorithm = match CoseAlgorithmIdentifier::from(alg) {
                    CoseAlgorithmIdentifier::AesGcm128 => Some(CoseAlgorithmIdentifier::AesGcm128),
                    CoseAlgorithmIdentifier::AsconAead128 => {
                        Some(CoseAlgorithmIdentifier::AsconAead128)
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
                let critical_header_list_size = decoder.array()?;
                let mut critical_header_count: u64 = 0;
                loop {
                    // Array can be either infinite length (none) or fixed length
                    if let Some(limit) = critical_header_list_size {
                        if critical_header_count >= limit {
                            break;
                        }
                        critical_header_count += 1;
                    } else {
                        if decoder.datatype()? == minicbor::data::Type::Break {
                            decoder.skip()?;
                            break;
                        }
                    }

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
