//! Local-first backend daemon, cryptographic protocol core, and TCP transport.

pub mod config;
pub mod crypto;
pub mod daemon;
pub mod demo_log;
pub mod encoding;
pub mod error;
pub mod ids;
pub mod keystore;
pub mod net;
pub mod protocol;
pub mod store;
pub mod sync;

pub use error::{Error, Result};
