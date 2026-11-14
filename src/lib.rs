//! foldz: headless downmix and channel matrix over MXL, built on the
//! samplez engine.
//!
//! [`layout`] names the channels of a flow, [`presets`] turns a preset,
//! routing or explicit matrix into an output's gain matrix, and [`config`]
//! reads and checks the start-up configuration.

pub mod config;
pub mod layout;
pub mod presets;
