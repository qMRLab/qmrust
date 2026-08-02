//! Actual Flip-Angle Imaging B1+ mapping. BIDS suffix: `TB1AFI`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe, dump, effective};
