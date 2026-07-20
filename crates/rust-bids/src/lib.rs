//! A wasm-clean qMRI-BIDS layout resolver, in two layers: Layer 1 (`table`)
//! parses a dataset into flat rows via the `fs::DatasetFs` trait; Layer 2
//! (`resolve`) groups those rows into `Collection`s per a declarative grouping
//! config (plain/named/sequential sets). No `std::fs` — the shell supplies I/O.

pub mod collection;
pub mod config;
pub mod entities;
pub mod fs;
pub mod protocol;
pub mod resolve;
pub mod scan;
pub mod sidecar;
pub mod table;

pub use collection::{Collection, GroupedData, VolumeRef, Warning};
pub use config::{default_config, parse_config, BidsConfig, SetDef};
pub use fs::{DatasetFs, Entry};
pub use protocol::{protocol_for, resolve_protocol};
pub use resolve::{collections_for, resolve_set};
pub use scan::scan_dataset;
pub use sidecar::{sidecar_for, Sidecar};
pub use table::{parse_to_table, BidsRow};
