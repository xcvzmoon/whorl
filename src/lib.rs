//! Document similarity primitives and client-scoped candidate matching.

pub mod similarity;

#[path = "persistence/documents.rs"]
mod documents;
pub mod service;
