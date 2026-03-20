use aws_sdk_s3::{Client, presigning::PresigningConfig, primitives::ByteStream};
use dal_common::error::DalError;
use std::time::Duration;

/// Thin wrapper around an S3-compatible object storage SDK client.
pub struct StorageClient {
    inner: Client,
    bucket: String,
}

impl StorageClient {
    pub async fn new(
        aws_cfg: &aws_config::SdkConfig,
        bucket: String,
        endpoint_url: Option<String>,
        access_key_id: Option<String>,
        secret_access_key: Option<String>,
        region: String,
    ) -> anyhow::Result<Self> {
        let mut builder = aws_sdk_s3::config::Builder::from(aws_cfg);
        let mut force_path_style = false;
        builder = builder.region(aws_sdk_s3::config::Region::new(region));
        if let Some(ref url) = endpoint_url {
            // LocalStack / MinIO / R2 override
            builder = builder.endpoint_url(url);
            force_path_style = is_path_style_endpoint(url);
        }
        if let (Some(access_key_id), Some(secret_access_key)) = (access_key_id, secret_access_key) {
            let credentials = aws_sdk_s3::config::Credentials::new(
                access_key_id,
                secret_access_key,
                None,
                None,
                "dal-storage-config",
            );
            builder = builder.credentials_provider(credentials);
        }
        let s3_config = builder.force_path_style(force_path_style).build();
        let inner = Client::from_conf(s3_config);
        Ok(Self { inner, bucket })
    }

    /// Object key for a package archive: `packages/{name}/{name}-{version}.tar.gz`
    pub fn object_key(pkg_name: &str, version: &str) -> String {
        format!("packages/{pkg_name}/{pkg_name}-{version}.tar.gz")
    }

    /// Upload a package archive to object storage.
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

    /// Check whether an object exists in storage.
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

fn is_path_style_endpoint(url: &str) -> bool {
    let lowered = url.to_ascii_lowercase();
    lowered.contains("localhost") || lowered.contains("127.0.0.1") || lowered.contains("localstack")
}
