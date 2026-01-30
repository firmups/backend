use crate::api::cbor::codec::crypto;
use log::debug;
use minicbor::Decoder;
use minicbor::Encoder;
use std::pin::Pin;

pub enum KeyProviderError {
    KeyMismatch,
    KeyNotFound,
    DbError,
}

pub trait KeyProvider: Send + Sync {
    fn key_for_device<'a>(
        &'a mut self,
        device_id: u32,
        key_type: KeyType,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, KeyProviderError>> + Send + 'a>>;
}

pub enum CoseCodecError {
    MissingHeaderField,
    UnknownHeaderKey,
    UnknownCriticalHeader,
    DecryptionError,
    EncryptionError,
    UnknownAlgorithm,
    InvalidMessage,
    RandomnessFailed,
}

#[derive(PartialEq, Eq, Clone, Copy)]
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
    type Error = CoseCodecError;
    fn try_from(src: ProtectedHeaderDecode) -> Result<Self, Self::Error> {
        let ProtectedHeaderDecode {
            device_id: Some(device_id),
            opcode: Some(opcode),
            encryption_algorithm: Some(encryption_algorithm),
            nonce: Some(nonce),
        } = src
        else {
            return Err(CoseCodecError::MissingHeaderField);
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
    type Error = CoseCodecError;
    fn try_from(src: CoseAlgorithmIdentifier) -> Result<Self, Self::Error> {
        match src {
            CoseAlgorithmIdentifier::AesGcm128 => Ok(crypto::CryptoAlgorithm::AesGcm128),
            CoseAlgorithmIdentifier::AsconAead128 => Ok(crypto::CryptoAlgorithm::AsconAead128),
            CoseAlgorithmIdentifier::Unknown => Err(CoseCodecError::UnknownAlgorithm),
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

impl From<minicbor::decode::Error> for CoseCodecError {
    fn from(_src: minicbor::decode::Error) -> CoseCodecError {
        CoseCodecError::InvalidMessage
    }
}

pub async fn decode_msg(
    key_provider: &mut dyn KeyProvider,
    key_type: &mut KeyType,
    device_id: &mut u32,
    opcode: &mut u16,
    msg: &[u8],
) -> Result<Vec<u8>, CoseCodecError> {
    let mut decoder = Decoder::new(msg);
    if decoder.array()? != Some(3) {
        return Err(CoseCodecError::InvalidMessage);
    }

    let protected_header_buffer = decoder.bytes()?;
    let protected_header_decode = decode_protected_header(protected_header_buffer)?;
    let protected_header = ProtectedHeader::try_from(protected_header_decode)?;
    let crypto_key_type: KeyType;
    let crypto_alg: Box<dyn crypto::CryptoAead>;
    match protected_header.encryption_algorithm {
        CoseAlgorithmIdentifier::AesGcm128 => {
            crypto_key_type = KeyType::AesGcm128;
            crypto_alg = Box::new(crypto::crypto_aes::CryptoAes128Gcm);
        }
        CoseAlgorithmIdentifier::AsconAead128 => {
            crypto_key_type = KeyType::AsconAead128;
            crypto_alg = Box::new(crypto::crypto_ascon::CryptoAsconAead128);
        }
        CoseAlgorithmIdentifier::Unknown => {
            return Err(CoseCodecError::UnknownAlgorithm);
        }
    };

    if protected_header.nonce.len() != crypto_alg.nonce_len() {
        debug!(
            "Invalid nonce length: expected {}, got {}",
            crypto_alg.nonce_len(),
            protected_header.nonce.len()
        );
        return Err(CoseCodecError::InvalidMessage);
    }

    if decoder.map()? != Some(0) {
        debug!("Expected empty unprotected header map");
        return Err(CoseCodecError::InvalidMessage);
    }

    let encrypted_operation_buffer = decoder.bytes()?;
    if encrypted_operation_buffer.len() < crypto_alg.tag_len() {
        debug!("Ciphertext too short for tag");
        return Err(CoseCodecError::InvalidMessage);
    }

    let pt = crypto_alg
        .decrypt(
            &key_provider
                .key_for_device(protected_header.device_id, crypto_key_type)
                .await
                .map_err(|_| CoseCodecError::DecryptionError)?,
            &protected_header.nonce,
            &create_aad(protected_header_buffer)[..],
            encrypted_operation_buffer,
        )
        .map_err(|_| CoseCodecError::DecryptionError)?;

    *key_type = crypto_key_type;
    *device_id = protected_header.device_id;
    *opcode = protected_header.opcode;
    debug!(
        "Decrypted operation with opcode: {}",
        protected_header.opcode
    );
    Ok(pt)
}

pub async fn encode_msg(
    key_provider: &mut dyn KeyProvider,
    key_type: KeyType,
    device_id: u32,
    operation_id: u16,
    operation: &[u8],
) -> Result<Vec<u8>, CoseCodecError> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    let crypto_alg: Box<dyn crypto::CryptoAead> = match key_type {
        KeyType::AesGcm128 => Box::new(crypto::crypto_aes::CryptoAes128Gcm),
        KeyType::AsconAead128 => Box::new(crypto::crypto_ascon::CryptoAsconAead128),
    };

    let mut nonce = vec![0u8; crypto_alg.nonce_len()];
    getrandom::fill(&mut nonce[..]).map_err(|_| CoseCodecError::RandomnessFailed)?;
    let protected_header = ProtectedHeader {
        device_id,
        opcode: operation_id,
        encryption_algorithm: crypto_alg.alg_id().into(),
        nonce: nonce.to_vec(),
    };

    let protected_header_buf = encode_protected_header(protected_header);
    debug!("protected header size: {}", protected_header_buf.len());
    let ct = crypto_alg
        .encrypt(
            &key_provider
                .key_for_device(device_id, key_type)
                .await
                .map_err(|_| CoseCodecError::EncryptionError)?,
            &nonce,
            &create_aad(&protected_header_buf)[..],
            operation,
        )
        .map_err(|_| CoseCodecError::EncryptionError)?;
    debug!("Ciphertext size: {}", ct.len());

    // Encoding cannot fail as we are writing to a Vec
    let _ = enc.array(3);
    let _ = enc.bytes(&protected_header_buf);
    let _ = enc.map(0);
    let _ = enc.bytes(&ct);

    debug!("Encrypted operation with opcode: {}", operation_id);
    Ok(buf)
}

fn encode_protected_header(protected_header: ProtectedHeader) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    // Encoding cannot fail as we are writing to a Vec
    let _ = enc.map(5);
    let _ = enc.u16(ProtectedHeaderKey::EncryptionAlgorithm as u16);
    let _ = enc.u16(protected_header.encryption_algorithm as u16);
    let _ = enc.u16(ProtectedHeaderKey::DeviceId as u16);
    let _ = enc.u32(protected_header.device_id);
    let _ = enc.u16(ProtectedHeaderKey::Opcode as u16);
    let _ = enc.u16(protected_header.opcode);
    let _ = enc.u16(ProtectedHeaderKey::EncryptionNonce as u16);
    let _ = enc.bytes(&protected_header.nonce[..]);
    let _ = enc.u16(ProtectedHeaderKey::CriticalHeaderList as u16);
    let _ = enc.array(2);
    let _ = enc.u16(ProtectedHeaderKey::DeviceId as u16);
    let _ = enc.u16(ProtectedHeaderKey::Opcode as u16);

    buf
}

fn decode_protected_header(
    protected_header_buf: &[u8],
) -> Result<ProtectedHeaderDecode, CoseCodecError> {
    let mut decoder = Decoder::new(protected_header_buf);
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
        } else if decoder.datatype()? == minicbor::data::Type::Break {
            decoder.skip()?;
            break;
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
                        return Err(CoseCodecError::UnknownAlgorithm);
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
                    } else if decoder.datatype()? == minicbor::data::Type::Break {
                        decoder.skip()?;
                        break;
                    }

                    let header_id = decoder.u16()?;
                    match ProtectedHeaderKey::from(header_id) {
                        ProtectedHeaderKey::DeviceId | ProtectedHeaderKey::Opcode => {}
                        _ => {
                            return Err(CoseCodecError::UnknownCriticalHeader);
                        }
                    }
                }
            }
            _ => {
                return Err(CoseCodecError::UnknownHeaderKey);
            }
        }
    }

    Ok(header)
}

fn create_aad(protected_header_buf: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(256);
    let mut enc = Encoder::new(&mut buf);

    // Encoding cannot fail as we are writing to a Vec
    let _ = enc.array(3);
    let _ = enc.str("Encrypt0");
    let _ = enc.bytes(protected_header_buf);
    let _ = enc.bytes(&[][..]);

    buf
}
