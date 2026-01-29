//! Utility modules for XPDK
//!
//! This module provides various utility functions and helpers for the XPDK system.

pub mod config;
pub mod cpu;
pub mod logging;
pub mod time;

#[cfg(feature = "numa")]
pub mod numa;

#[cfg(feature = "hardware-offload")]
pub mod offload;
