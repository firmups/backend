use crate::api::cbor::codec::crypto;
use ascon_aead128::{
    AsconAead128, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};

pub struct CryptoAsconAead128;

impl crypto::CryptoAead for CryptoAsconAead128 {
    fn alg_id(&self) -> crypto::CryptoAlgorithm {
        crypto::CryptoAlgorithm::AsconAead128
    }

    fn nonce_len(&self) -> usize {
        16
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
        let key = Key::<AsconAead128>::try_from(key).map_err(|_| crypto::CryptoError::KeyError)?;
        let nonce =
            Nonce::<AsconAead128>::try_from(nonce).map_err(|_| crypto::CryptoError::NonceError)?;
        let cipher = AsconAead128::new(&key);

        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad: aad,
                },
            )
            .map_err(|_| crypto::CryptoError::EncryptionError)?;

        Ok(ciphertext)
    }

    fn decrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, crypto::CryptoError> {
        let key = Key::<AsconAead128>::try_from(key).map_err(|_| crypto::CryptoError::KeyError)?;
        let nonce =
            Nonce::<AsconAead128>::try_from(nonce).map_err(|_| crypto::CryptoError::NonceError)?;
        let cipher = AsconAead128::new(&key);

        let plaintext = cipher
            .decrypt(
                &nonce,
                Payload {
                    msg: ciphertext,
                    aad: aad,
                },
            )
            .map_err(|_| crypto::CryptoError::DecryptionError)?;

        Ok(plaintext)
    }
}
