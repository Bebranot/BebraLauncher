//! Core of the launcher. Knows nothing about UI.
//!
//! Protocol details live in `docs/protocol.md`; comments here point to its sections.

pub mod accounts;
pub mod addr;
pub mod auth;
pub mod content_db;
pub mod download;
pub mod dump;
pub mod engine;
pub mod error;
pub mod fsutil;
pub mod hash;
pub mod http;
pub mod hub;
pub mod hwid;
pub mod launch;
pub mod manifest;
pub mod modules;
pub mod packs;
pub mod patches;
pub mod paths;
pub mod play;
pub mod process;
pub mod secret;
pub mod self_update;
pub mod server;
pub mod signature;
pub mod storage;
pub mod updater;
pub mod zipmode;

#[cfg(test)]
mod test_util;

pub use addr::ServerAddr;
pub use error::{Error, Result};
