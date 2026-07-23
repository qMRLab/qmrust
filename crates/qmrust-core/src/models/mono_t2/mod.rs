//! Monoexponential T2 mapping (multi-echo spin-echo). BIDS suffix: `MESE`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe, dump};
