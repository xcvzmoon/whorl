//! Document similarity primitives and client-scoped candidate matching.

pub mod cpu_pool;
pub mod similarity;

#[path = "persistence/documents.rs"]
mod documents;
pub mod service;

pub mod storage;

pub mod extraction;

pub mod queue;

mod telemetry;

pub mod locks;

pub mod logging;

#[path = "persistence/results.rs"]
mod results;
