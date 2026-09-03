//! sentinel-core — shared types, traits, and error types for the Sentinel Runtime.
//!
//! Every other crate in the workspace depends on this crate and nothing else from
//! the workspace.  This is the dependency floor.

pub mod error;
pub mod pipeline;
pub mod types;
