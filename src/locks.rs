//! Stable PostgreSQL advisory-lock identities shared by runtime and maintenance.
//!
//! Preserve these values across replicas and rolling deployments. Changing one
//! participant independently breaks mutual exclusion even if its SQL is valid.

/// Serializes capacity reservations and expired-request reclamation.
///
/// Admission, maintenance, and the `reserve_capacity` function in the application
/// migration must use this same identity. It is an arbitrary reserved namespace,
/// separate from deployment migrations and hashed source/file identities.
pub const CAPACITY: i64 = 746_831_904;

/// Serializes deployment application migrations before queue schema migration.
pub const MIGRATIONS: i64 = 746_831_905;

/// Hash seed for upload writing, attachment, discard, and reconciliation locks.
///
/// Zero preserves the deployed object-key protocol. All participants hash the
/// complete object key with PostgreSQL `hashtextextended` under this seed.
pub const UPLOAD: i64 = 0;

/// Hash seed for per-file processing and deadline recovery locks.
///
/// The stable UUID string is hashed with this seed. Workers and recovery must
/// agree so expired-work cleanup cannot invalidate a live attempt.
pub const ATTEMPT: i64 = 15;
