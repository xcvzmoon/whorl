//! PostgreSQL schema and lossless storage of unsigned signature hashes.

pub(crate) const APPLICATION_MIGRATION: &str =
    include_str!("../../migrations/0001_application.sql");

pub(crate) const fn hash_to_sql(value: u32) -> i32 {
    i32::from_ne_bytes(value.to_ne_bytes())
}

pub(crate) const fn hash_from_sql(value: i32) -> u32 {
    u32::from_ne_bytes(value.to_ne_bytes())
}
