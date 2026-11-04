//! S3-compatible client construction for the application's shared source bucket.
//!
//! [`connect`] reads process-delivered AWS credentials and constructs a path-style
//! client without creating a bucket or sending a request. Production endpoints
//! should use HTTPS; HTTP is accepted for isolated local `MinIO` verification.
//! The operator owns bucket provisioning and incomplete-multipart expiration.
//!
//! Pass the client to [`super::UploadStore`] with the application database pool.
//! That store journals uploads and retains capacity charges until deletion succeeds.
//! Sharing the same bucket and database across replicas is required for recovery.
//!
//! # Examples
//!
//! ```
//! let client = whorl::storage::s3::connect("https://minio.example.com", "uploads", "us-east-1")?;
//! # Ok::<(), object_store::Error>(())
//! ```

use object_store::aws::{AmazonS3, AmazonS3Builder};

/// Builds a path-style S3 client using process-delivered AWS credentials.
///
/// # Arguments
/// * `endpoint` - `MinIO` endpoint, including its scheme. HTTP is for local tests only.
/// * `bucket` - Existing bucket owned by this service.
/// * `region` - Signing region configured on the provider.
///
/// # Returns
/// A client with bounded provider retries and request timeouts.
///
/// # Errors
/// Returns [`object_store::Error`] for invalid client configuration.
///
/// # Examples
/// ```
/// let client = whorl::storage::s3::connect("https://minio.example.com", "uploads", "us-east-1")?;
/// # Ok::<(), object_store::Error>(())
/// ```
pub fn connect(endpoint: &str, bucket: &str, region: &str) -> object_store::Result<AmazonS3> {
    AmazonS3Builder::from_env()
        .with_endpoint(endpoint)
        .with_bucket_name(bucket)
        .with_region(region)
        .with_virtual_hosted_style_request(false)
        .with_allow_http(endpoint.starts_with("http://"))
        .build()
}
