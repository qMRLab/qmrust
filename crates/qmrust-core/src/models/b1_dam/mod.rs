//! Double-Angle Method B1+ mapping. BIDS suffix: `TB1DAM`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe, dump, effective};
