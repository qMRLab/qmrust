//! Magnetization transfer saturation (MTsat), Helms 2008. BIDS suffix: `MTS`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, build_calibration, describe, dump};
