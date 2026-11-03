use std::{net::SocketAddr, num::NonZeroUsize};

#[derive(thiserror::Error, Debug)]
#[error("invalid or missing configuration: {0}")]
pub(crate) struct ConfigError(&'static str);

pub(crate) struct ServiceConfig {
    pub(crate) database: sqlx::postgres::PgConnectOptions,
}

impl ServiceConfig {
    pub(crate) fn load_local() -> Result<(), ConfigError> {
        match dotenvy::from_path(".env") {
            Ok(()) => Ok(()),
            Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(())
            }
            Err(_) => Err(ConfigError(".env")),
        }
    }

    pub(crate) fn required(key: &'static str) -> Result<String, ConfigError> {
        std::env::var(key)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .ok_or(ConfigError(key))
    }

    pub(crate) fn load() -> Result<Self, ConfigError> {
        let database = Self::required("DATABASE_URL")?
            .parse()
            .map_err(|_| ConfigError("DATABASE_URL"))?;
        Ok(Self { database })
    }

    pub(crate) fn storage() -> Result<StorageConfig, ConfigError> {
        Ok(StorageConfig {
            endpoint: Self::required("WHORL_S3_ENDPOINT")?,
            bucket: Self::required("WHORL_S3_BUCKET")?,
            region: Self::required("WHORL_S3_REGION")?,
        })
    }
}

pub(crate) struct StorageConfig {
    pub(crate) endpoint: String,
    pub(crate) bucket: String,
    pub(crate) region: String,
}

#[allow(
    dead_code,
    reason = "shared configuration compiled by role-specific binaries"
)]
pub(crate) fn listen() -> Result<SocketAddr, ConfigError> {
    std::env::var("WHORL_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:8080".into())
        .parse()
        .map_err(|_| ConfigError("WHORL_LISTEN"))
}

#[allow(
    dead_code,
    reason = "shared configuration compiled by role-specific binaries"
)]
pub(crate) fn workers() -> Result<NonZeroUsize, ConfigError> {
    parse_workers(&std::env::var("WHORL_CPU_WORKERS").unwrap_or_else(|_| "2".into()))
}

fn parse_workers(value: &str) -> Result<NonZeroUsize, ConfigError> {
    value
        .parse::<NonZeroUsize>()
        .ok()
        .filter(|v| v.get() <= 2)
        .ok_or(ConfigError("WHORL_CPU_WORKERS"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_bounds() {
        for value in ["0", "3", "-1", "abc", ""] {
            assert!(parse_workers(value).is_err());
        }
        assert_eq!(parse_workers("1").unwrap().get(), 1);
        assert_eq!(parse_workers("2").unwrap().get(), 2);
    }
}
