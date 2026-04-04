use crate::storage::local::LocalStorage;
use crate::storage::s3::S3Storage;
use async_trait::async_trait;
use std::pin::Pin;
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

mod local;
mod s3;

#[derive(Clone)]
pub enum StorageConfig {
    Local {
        path: String,
    },
    S3 {
        endpoint: String,
        bucket: String,
        access_key: String,
        secret_key: String,
    },
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()>;
    async fn delete(&self, key: &str) -> anyhow::Result<()>;
    async fn load_range(&self, key: &str, offset: u64, length: u64) -> anyhow::Result<Vec<u8>>;
    async fn stream(
        &self,
        key: &str,
    ) -> anyhow::Result<ReaderStream<Pin<Box<dyn AsyncRead + Send>>>>;
}

pub async fn create_storage(cfg: StorageConfig) -> anyhow::Result<Box<dyn Storage>> {
    match cfg {
        StorageConfig::Local { path } => Ok(Box::new(LocalStorage::new(path))),
        StorageConfig::S3 {
            endpoint,
            bucket,
            access_key,
            secret_key,
        } => Ok(Box::new(
            S3Storage::new(&endpoint, &bucket, &access_key, &secret_key).await?,
        )),
    }
}
