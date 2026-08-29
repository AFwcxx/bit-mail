//! Deterministic `bit-mail` library.
//!
//! Domain modules are added as their behavior lands; the binary owns only
//! process setup and exit reporting.

pub mod cli;
pub mod connect;
pub mod credentials;
pub mod gmail;
pub mod provider;
pub mod pull;
pub mod repository;
pub mod storage;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
