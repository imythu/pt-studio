//! PT Studio 内核（设计见 docs/platform/kernel/）。

pub mod catalog;
pub mod cluster;
pub mod config;
pub mod events;
pub mod gateway;
pub mod kernel;
pub mod loader;
pub mod runtime;
pub mod storage;

pub use kernel::{Kernel, StartError};
