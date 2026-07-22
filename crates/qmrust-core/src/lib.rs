//! qmrust-core — the pure functional core of qmrust: the `Model` trait and
//! value types (`core`), per-model signal/fitting math (`models`), the parallel
//! voxel engine (`engine`), the model `registry`, simulation (`sim`), config
//! parsing (`config`), and fit-result types (`fitting`). No CLI, no file I/O
//! beyond the (native-only, cfg-gated) qMRLab validation helper in
//! `models::qmt_spgr::sf`; designed to compile to `wasm32-unknown-unknown`.

pub mod config;
pub mod core;
pub mod engine;
pub mod fitting;
pub mod models;
pub mod quad;
pub mod registry;
pub mod sim;
