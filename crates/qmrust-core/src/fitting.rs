//! Generic parallel voxel-wise fitting engine.
//!
//! This is the Rust equivalent of qMRLab's FitData.m — a reusable engine
//! that takes a closure for per-voxel fitting and parallelizes across the volume.

use ndarray::Array3;
use std::collections::HashMap;

/// Named 3D output maps from fitting.
pub type FitResults = HashMap<String, Array3<f64>>;
