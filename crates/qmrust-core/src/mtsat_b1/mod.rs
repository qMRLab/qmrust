//! Model-based MTsat B1+ correction (TardifLab port): a 5-state
//! Bloch–McConnell FLASH signal engine, tricubic surface fit, single-MTw
//! self-calibration, and the voxelwise correction factor. Pure; no I/O.

/// Gyromagnetic ratio of the proton (MHz/T = Hz/µT), shared across the
/// pulse-shape and rate-matrix math.
pub(crate) const GAMMA: f64 = 42.577478518;

pub mod calibrate;
pub mod correct;
pub mod fitvalues;
pub mod mat5;
pub mod pulse;
pub mod rate;
pub mod sim;
pub mod surface;
