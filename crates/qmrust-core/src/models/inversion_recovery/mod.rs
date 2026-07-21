//! Inversion Recovery T1 mapping (Barral RD-NLS). BIDS suffix: `IRT1`.

pub mod config;
pub mod fit;
pub mod model;

pub use model::{build, describe};
