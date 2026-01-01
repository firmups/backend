pub mod crypto_aes;
pub mod crypto_ascon;

#[derive(Eq, Hash, PartialEq, Clone, Copy)]
pub enum CryptoAlgorithm {
    AesGcm128,
    AsconAead128,
}

pub enum CryptoError {
    KeyError,
    NonceError,
    EncryptionError,
    DecryptionError,
}

pub trait CryptoAead: Send + Sync {
    /// Return the COSE/enum identifier for this algorithm.
    fn alg_id(&self) -> CryptoAlgorithm;

    /// Expected nonce length at runtime.
    fn nonce_len(&self) -> usize;

    fn tag_len(&self) -> usize;

    fn encrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;

    fn decrypt(
        &self,
        key: &[u8],
        nonce: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, CryptoError>;
}
