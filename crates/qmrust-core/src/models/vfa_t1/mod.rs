//! Variable flip angle T1 mapping (Fram linearized SPGR). BIDS suffix: `VFA`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe, dump, effective};
