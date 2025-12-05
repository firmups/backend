use aes_gcm::{
    Aes128Gcm, Error, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};

pub fn decrypt_operation_aes(
    encrypted_operation_buffer: &[u8],
    aad_bytes: &[u8],
    nonce_bytes: &[u8],
    key_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let key = Key::<Aes128Gcm>::from_slice(key_bytes);
    let cipher = Aes128Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes); // 96-bits; unique per message

    let payload: Payload = Payload {
        msg: encrypted_operation_buffer,
        aad: aad_bytes,
    };

    let plaintext = cipher.decrypt(nonce, payload)?;

    Ok(plaintext)
}
