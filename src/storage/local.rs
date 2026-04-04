use std::pin::Pin;
use tokio::{
    fs::File,
    io::{AsyncRead, AsyncReadExt, AsyncSeekExt},
};
use tokio_util::io::ReaderStream;

use crate::storage::Storage;

pub struct LocalStorage {
    base_path: std::path::PathBuf,
}

impl LocalStorage {
    pub fn new(base_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            base_path: base_path.into(),
        }
    }
}

#[async_trait::async_trait]
impl Storage for LocalStorage {
    async fn save(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let mut path = self.base_path.clone();
        path.push(key);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    async fn load_range(&self, key: &str, offset: u64, length: u64) -> anyhow::Result<Vec<u8>> {
        let mut path = self.base_path.clone();
        path.push(key);
        let mut file = File::open(path).await?;
        file.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut buf = vec![0u8; length as usize];
        let read = file.read(&mut buf).await?;
        buf.truncate(read);
        Ok(buf)
    }

    async fn stream(
        &self,
        key: &str,
    ) -> anyhow::Result<ReaderStream<Pin<Box<dyn AsyncRead + Send>>>> {
        let mut path = self.base_path.clone();
        path.push(key);
        let file = File::open(path).await?;
        // File implements AsyncRead; box+pin it so we can have a single return type.
        let reader: Pin<Box<dyn AsyncRead + Send>> = Box::pin(file);
        // Convert AsyncRead -> Stream of Bytes via ReaderStream
        Ok(ReaderStream::new(reader)) // yields io::Result<Bytes> per item
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        let mut path = self.base_path.clone();
        path.push(key);
        tokio::fs::remove_file(path).await?;
        Ok(())
    }
}
