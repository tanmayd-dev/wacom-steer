//! Wacom pen steering and analog gamepad input bridge for Linux, Wine, and Proton.

pub mod config;
pub mod device;
pub mod launch;
pub mod mapping;
pub mod mouse;
pub mod output;
pub mod pedal;
pub mod relative;

/// Backwards compatibility alias for [`mouse`].
pub use mouse as minecraft;
