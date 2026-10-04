//! The workbench CLI as a library, so `tests/golden.rs` drives exactly the command
//! surface a user drives — one implementation of the closed loop, not two.

pub mod app;
pub mod baseline;
pub mod capture;
pub mod context;
pub mod export;
pub mod policy;
pub mod views;
