//! foldz: headless downmix and channel matrix over MXL, built on the
//! samplez engine.
//!
//! [`layout`] names the channels of a flow, [`presets`] turns a preset,
//! routing or explicit matrix into an output's gain matrix, and [`config`]
//! reads and checks the start-up configuration. [`api`] serves the downmix
//! and the engine's status read-only, [`metrics`] passes the engine's
//! heartbeat and counters to the ops listener, and [`state`] is the
//! replicated state of one replica without runtime state.

pub mod api;
pub mod config;
pub mod layout;
pub mod metrics;
pub mod presets;
pub mod state;
