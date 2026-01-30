use crate::api::cbor::codec::crypto;
use aes_gcm::{
    Aes128Gcm, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};

pub struct CryptoAes128Gcm;

impl crypto::CryptoAead for CryptoAes128Gcm {
    fn alg_id(&self) -> crypto::CryptoAlgorithm {
        crypto::CryptoAlgorithm::AesGcm128
    }

    fn nonce_len(&self) -> usize {
        12
    }

    fn tag_len(&self) -> usize {
        16
    }

    fn encrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, crypto::CryptoError> {
        if key.len() != 16 {
            return Err(crypto::CryptoError::Key);
        }
        if nonce.len() != self.nonce_len() {
            return Err(crypto::CryptoError::Nonce);
        }

        let key = Key::<Aes128Gcm>::from_slice(key);
        let nonce = Nonce::from_slice(nonce);
        let cipher = Aes128Gcm::new(key);

        let payload: Payload = Payload {
            msg: plaintext,
            aad,
        };

        let ciphertext = cipher
            .encrypt(nonce, payload)
            .map_err(|_| crypto::CryptoError::Encryption)?;

        Ok(ciphertext)
    }

    fn decrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, crypto::CryptoError> {
        if key.len() != 16 {
            return Err(crypto::CryptoError::Key);
        }
        if nonce.len() != self.nonce_len() {
            return Err(crypto::CryptoError::Nonce);
        }

        let key = Key::<Aes128Gcm>::from_slice(key);
        let nonce = Nonce::from_slice(nonce);
        let cipher = Aes128Gcm::new(key);

        let payload: Payload = Payload {
            msg: ciphertext,
            aad,
        };

        let plaintext = cipher
            .decrypt(nonce, payload)
            .map_err(|_| crypto::CryptoError::Decryption)?;

        Ok(plaintext)
    }
}
