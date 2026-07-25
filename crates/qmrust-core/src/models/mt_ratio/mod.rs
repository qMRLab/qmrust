//! Magnetization transfer ratio (MTR). BIDS suffix: `MTR`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe, dump};
