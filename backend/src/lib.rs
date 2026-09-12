//! Backend daemon scaffold. Real cryptography and networking are pending.

pub mod config;
pub mod crypto;
pub mod daemon;
pub mod encoding;
pub mod error;
pub mod ids;
pub mod keystore;
pub mod protocol;
pub mod store;
pub mod sync;

pub use error::{Error, Result};
