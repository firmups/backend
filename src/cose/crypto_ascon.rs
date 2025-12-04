use ascon_aead128::{
    AsconAead128, Error, Key, Nonce,
    aead::{Aead, KeyInit, Payload},
};

pub fn decrypt_operation_ascon(
    encrypted_operation_buffer: &[u8],
    aad_bytes: &[u8],
    nonce_bytes: &[u8],
    key_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let key = Key::<AsconAead128>::try_from(key_bytes).expect("Invalid key length");
    let nonce = Nonce::<AsconAead128>::try_from(nonce_bytes).expect("Invalid nonce length");
    let cipher = AsconAead128::new(&key);

    let plaintext = cipher.decrypt(
        &nonce,
        Payload {
            msg: encrypted_operation_buffer,
            aad: aad_bytes,
        },
    )?;

    Ok(plaintext)
}

pub fn encrypt_operation_ascon(
    operation_buffer: &[u8],
    aad_bytes: &[u8],
    nonce_bytes: &[u8],
    key_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let key = Key::<AsconAead128>::try_from(key_bytes).expect("Invalid key length");
    let nonce = Nonce::<AsconAead128>::try_from(nonce_bytes).expect("Invalid nonce length");
    let cipher = AsconAead128::new(&key);

    let plaintext = cipher.encrypt(
        &nonce,
        Payload {
            msg: operation_buffer,
            aad: aad_bytes,
        },
    )?;

    Ok(plaintext)
}
