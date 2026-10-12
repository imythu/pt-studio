//! PT Studio 内核（设计见 docs/platform/kernel/）。

pub mod catalog;
pub mod cluster;
pub mod config;
pub mod events;
pub mod frontend;
pub mod gateway;
pub mod http_client;
pub mod kernel;
pub mod limits;
pub mod loader;
pub mod runtime;
pub mod scheduler;
pub mod storage;
pub mod wasm;

pub use kernel::{Kernel, StartError};
