use super::codec::cose;
use crate::db::models::{DeviceKey, KeyStatus, LightweightKeyDetails};
use diesel::ExpressionMethods;
use diesel::QueryDsl;
use diesel::SelectableHelper;
use diesel_async::RunQueryDsl;
use log::warn;
use std::sync::Arc;
use std::{future::Future, pin::Pin};
use zeroize::Zeroize;

pub enum CoseHandlerError {
    DecodingError,
    EncodingError,
}

#[derive(Clone)]
struct DbKeyProvider {
    shared_pool: Arc<crate::DbPool>,
    key_bytes: Option<Vec<u8>>,
}

impl cose::KeyProvider for DbKeyProvider {
    fn key_for_device<'a>(
        &'a mut self,
        device_id: u32,
        key_type: cose::KeyType,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, cose::KeyProviderError>> + Send + 'a>> {
        let pool = Arc::clone(&self.shared_pool);
        Box::pin(async move {
            use crate::db::schema::device_key::dsl as device_key_dsl;
            use crate::db::schema::lightweight_key_details::dsl as details_dsl;
            let mut conn = pool
                .get_owned()
                .await
                .map_err(|_| cose::KeyProviderError::DbError)?;

            let (active_key, details): (DeviceKey, LightweightKeyDetails) =
                device_key_dsl::device_key
                    .inner_join(details_dsl::lightweight_key_details)
                    .filter(device_key_dsl::device.eq(device_id as i32))
                    .filter(device_key_dsl::status.eq(KeyStatus::ACTIVE))
                    .select((DeviceKey::as_select(), LightweightKeyDetails::as_select()))
                    .first(&mut conn)
                    .await
                    .map_err(|e| match e {
                        diesel::result::Error::NotFound => {
                            warn!("Key not found for device {}", device_id);
                            cose::KeyProviderError::KeyNotFound
                        }
                        _ => {
                            warn!("Database error for device {}", device_id);
                            cose::KeyProviderError::DbError
                        }
                    })?;
            if active_key.key_type != crate::db::models::KeyType::LIGHTWEIGHT {
                warn!("Key type mismatch for device {}", device_id);
                return Err(cose::KeyProviderError::KeyMismatch);
            }
            match details.algorithm {
                crate::db::models::CryptoAlgorithm::AesGcm128 => match key_type {
                    cose::KeyType::AesGcm128 => {
                        self.key_bytes = details.key.clone().into();
                        Ok(details.key)
                    }
                    _ => {
                        warn!("Key algorithm mismatch for device {}", device_id);
                        Err(cose::KeyProviderError::KeyMismatch)
                    }
                },
                crate::db::models::CryptoAlgorithm::AsconAead128 => match key_type {
                    cose::KeyType::AsconAead128 => {
                        self.key_bytes = details.key.clone().into();
                        Ok(details.key)
                    }
                    _ => {
                        warn!("Key algorithm mismatch for device {}", device_id);
                        Err(cose::KeyProviderError::KeyMismatch)
                    }
                },
            }
        })
    }
}

impl Drop for DbKeyProvider {
    fn drop(&mut self) {
        if let Some(key_bytes) = &mut self.key_bytes {
            key_bytes.zeroize();
        }
    }
}

struct StaticKeyProvider {
    device_id: u32,
    key_type: cose::KeyType,
    key_bytes: Vec<u8>,
}

impl cose::KeyProvider for StaticKeyProvider {
    fn key_for_device<'a>(
        &'a mut self,
        device_id: u32,
        key_type: cose::KeyType,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, cose::KeyProviderError>> + Send + 'a>> {
        let key_bytes = self.key_bytes.clone();
        Box::pin(async move {
            if self.device_id != device_id {
                return Err(cose::KeyProviderError::KeyNotFound);
            }
            if self.key_type != key_type {
                return Err(cose::KeyProviderError::KeyMismatch);
            }
            Ok(key_bytes)
        })
    }
}

impl Drop for StaticKeyProvider {
    fn drop(&mut self) {
        self.key_bytes.zeroize();
    }
}

pub struct CoseHandler {
    shared_pool: Arc<crate::DbPool>,
    device_id: Option<u32>,
    key_bytes: Option<Vec<u8>>,
    key_type: Option<cose::KeyType>,
}

impl Drop for CoseHandler {
    fn drop(&mut self) {
        if let Some(key_bytes) = &mut self.key_bytes {
            key_bytes.zeroize();
        }
    }
}

impl CoseHandler {
    pub fn new(shared_pool: Arc<crate::DbPool>) -> Self {
        CoseHandler {
            shared_pool,
            device_id: None,
            key_bytes: None,
            key_type: None,
        }
    }

    pub async fn decode_msg(
        &mut self,
        device_id: &mut u32,
        opcode: &mut u16,
        msg: &[u8],
    ) -> Result<Vec<u8>, CoseHandlerError> {
        let mut key_provider = Box::new(DbKeyProvider {
            shared_pool: Arc::clone(&self.shared_pool),
            key_bytes: None,
        });
        let mut key_type: cose::KeyType = cose::KeyType::AesGcm128; // Default, will be set by decode_msg
        let res = cose::decode_msg(key_provider.as_mut(), &mut key_type, device_id, opcode, msg)
            .await
            .map_err(|_| CoseHandlerError::DecodingError)?;
        self.device_id = Some(*device_id);
        self.key_bytes = match key_provider.key_bytes.clone() {
            Some(k) => Some(k),
            _ => return Err(CoseHandlerError::DecodingError),
        };
        self.key_type = Some(key_type);
        Ok(res)
    }

    pub async fn encode_msg(
        &self,
        operation_id: u16,
        operation: &[u8],
    ) -> Result<Vec<u8>, CoseHandlerError> {
        let Some(device_id) = self.device_id else {
            return Err(CoseHandlerError::EncodingError);
        };
        let Some(key_bytes) = &self.key_bytes else {
            return Err(CoseHandlerError::EncodingError);
        };
        let Some(key_type) = self.key_type else {
            return Err(CoseHandlerError::EncodingError);
        };

        let mut key_provider = Box::new(StaticKeyProvider {
            device_id,
            key_type,
            key_bytes: key_bytes.clone(),
        });

        match cose::encode_msg(
            key_provider.as_mut(),
            key_type,
            device_id,
            operation_id,
            operation,
        )
        .await
        {
            Ok(res) => Ok(res),
            Err(_) => Err(CoseHandlerError::EncodingError),
        }
    }
}
