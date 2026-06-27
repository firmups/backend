use async_trait::async_trait;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Region};
use aws_sdk_s3::{Client, error::SdkError, primitives::ByteStream};
use log::debug;
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
        debug!("Initializing S3 storage with bucket '{}'", bucket);

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
        s3_builder = s3_builder.force_path_style(true);

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
        debug!("Saving object '{}' to S3 bucket '{}'", key, self.bucket);
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
        debug!(
            "Loading range bytes {}-{} of object '{}' from S3 bucket '{}'",
            offset,
            offset + length - 1,
            key,
            self.bucket
        );
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let out = match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .range(range)
            .send()
            .await
        {
            Ok(o) => o,
            // A range whose start is at or past EOF returns 416 (Range Not
            // Satisfiable). Treat it as a clean EOF (empty read) to match the
            // local backend, so a firmware whose size is an exact multiple of the
            // requested chunk size doesn't fail on the device's final past-EOF read.
            Err(e) if e.raw_response().map(|r| r.status().as_u16()) == Some(416) => {
                return Ok(Vec::new());
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "s3 get_object {} failed: {}",
                    key,
                    display_sdk_err(e)
                ));
            }
        };

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
        debug!(
            "Streaming object '{}' from S3 bucket '{}'",
            key, self.bucket
        );
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
        debug!("Deleting object '{}' from S3 bucket '{}'", key, self.bucket);
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
