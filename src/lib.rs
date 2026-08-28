//! Deterministic `bit-mail` library.
//!
//! Domain modules are added as their behavior lands; the binary owns only
//! process setup and exit reporting.

pub mod cli;
pub mod repository;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
