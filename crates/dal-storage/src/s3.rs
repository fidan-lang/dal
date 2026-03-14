use aws_sdk_s3::{Client, presigning::PresigningConfig, primitives::ByteStream};
use dal_common::error::DalError;
use std::time::Duration;

/// Thin wrapper around the S3 SDK client.
pub struct StorageClient {
    inner: Client,
    bucket: String,
}

impl StorageClient {
    pub async fn new(
        aws_cfg: &aws_config::SdkConfig,
        bucket: String,
        endpoint_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let mut builder = aws_sdk_s3::config::Builder::from(aws_cfg);
        if let Some(url) = endpoint_url {
            // LocalStack / MinIO override
            builder = builder.endpoint_url(url);
        }
        let s3_config = builder
            .force_path_style(true) // required for LocalStack
            .build();
        let inner = Client::from_conf(s3_config);
        Ok(Self { inner, bucket })
    }

    /// S3 key for a package archive:  `packages/{name}/{name}-{version}.tar.gz`
    pub fn object_key(pkg_name: &str, version: &str) -> String {
        format!("packages/{pkg_name}/{pkg_name}-{version}.tar.gz")
    }

    /// Upload a package archive to S3.
    pub async fn upload(
        &self,
        key: &str,
        data: Vec<u8>,
        content_type: &str,
    ) -> Result<(), DalError> {
        let stream = ByteStream::from(data);
        self.inner
            .put_object()
            .bucket(&self.bucket)
            .key(key)
            .content_type(content_type)
            .body(stream)
            .send()
            .await
            .map_err(|e| DalError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Generate a presigned download URL (TTL default: 60 seconds).
    pub async fn presigned_url(&self, key: &str, ttl_secs: u64) -> Result<String, DalError> {
        let config = PresigningConfig::expires_in(Duration::from_secs(ttl_secs))
            .map_err(|e| DalError::Storage(e.to_string()))?;

        let presigned = self
            .inner
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(config)
            .await
            .map_err(|e| DalError::Storage(e.to_string()))?;

        Ok(presigned.uri().to_string())
    }

    /// Delete a package archive (e.g. after a failed publish transaction).
    pub async fn delete(&self, key: &str) -> Result<(), DalError> {
        self.inner
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| DalError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Check whether an object exists in S3.
    pub async fn exists(&self, key: &str) -> Result<bool, DalError> {
        match self
            .inner
            .head_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let svc = e.into_service_error();
                if svc.is_not_found() {
                    Ok(false)
                } else {
                    Err(DalError::Storage(svc.to_string()))
                }
            }
        }
    }
}
