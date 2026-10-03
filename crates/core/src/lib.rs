//! Core of the launcher. Knows nothing about UI.
//!
//! Protocol details live in `docs/protocol.md`; comments here point to its sections.

pub mod addr;
pub mod content_db;
pub mod download;
pub mod engine;
pub mod error;
pub mod hash;
pub mod http;
pub mod hub;
pub mod launch;
pub mod manifest;
pub mod modules;
pub mod paths;
pub mod process;
pub mod server;
pub mod signature;
pub mod updater;
pub mod zipmode;

#[cfg(test)]
mod test_util;

pub use addr::ServerAddr;
pub use error::{Error, Result};
