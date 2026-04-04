use async_trait::async_trait;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Region};
use aws_sdk_s3::{Client, error::SdkError, primitives::ByteStream};
use std::pin::Pin;
use tokio::io::AsyncRead;
use tokio_util::io::ReaderStream;

pub struct S3Storage {
    client: Client,
    bucket: String,
}

impl S3Storage {
    pub async fn new(
        endpoint: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
    ) -> anyhow::Result<Self> {
        // Base config from environment (reads AWS_* env vars if present)
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(Region::new("garage"));

        // If you want to force static credentials instead of env/IMDS, set:
        loader = loader.credentials_provider(aws_sdk_s3::config::Credentials::new(
            access_key, secret_key, None, None, "static",
        ));

        let base = loader.load().await;

        // Build S3 config with optional endpoint and path-style
        let mut s3_builder: S3ConfigBuilder = aws_sdk_s3::config::Builder::from(&base);

        if !endpoint.is_empty() {
            s3_builder = s3_builder.endpoint_url(endpoint);
        }

        let s3_config = s3_builder.build();
        let client = Client::from_conf(s3_config);

        Ok(Self {
            client,
            bucket: bucket.to_owned(),
        })
    }
}

#[async_trait]
impl super::Storage for S3Storage {
    async fn save(&self, key: &str, bytes: &[u8]) -> anyhow::Result<()> {
        // Convert bytes to ByteStream
        let body = ByteStream::from(bytes.to_vec());

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .body(body)
            // .content_type("application/octet-stream") // optional
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("s3 put_object {} failed: {}", key, display_sdk_err(e)))?;

        Ok(())
    }

    async fn load_range(&self, key: &str, offset: u64, length: u64) -> anyhow::Result<Vec<u8>> {
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .range(range)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("s3 get_object {} failed: {}", key, display_sdk_err(e)))?;

        let data = out
            .body
            .collect()
            .await
            .map_err(|e| anyhow::anyhow!("s3 body collect {} failed: {}", key, e))?
            .into_bytes()
            .to_vec();

        Ok(data)
    }

    async fn stream(
        &self,
        key: &str,
    ) -> anyhow::Result<ReaderStream<Pin<Box<dyn AsyncRead + Send>>>> {
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("s3 get_object {} failed: {}", key, display_sdk_err(e)))?;

        let stream = out.body.into_async_read();
        let reader: Pin<Box<dyn AsyncRead + Send>> = Box::pin(stream);
        // Finally, wrap it as a ReaderStream (Stream of Bytes).
        Ok(ReaderStream::new(reader))
    }

    async fn delete(&self, key: &str) -> anyhow::Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!("s3 delete_object {} failed: {}", key, display_sdk_err(e))
            })?;
        Ok(())
    }
}

/// Utility to prettify SdkError display for logs/errors
fn display_sdk_err<E: std::fmt::Debug>(err: SdkError<E>) -> String {
    format!("{err:?}")
}
