//! qMRI-BIDS layout resolver: a wasm-clean Rust port of the bids2nf grammar.
//! Layer 1 (`table`) parses a dataset into flat rows via the `fs::DatasetFs`
//! trait; Layer 2 (`resolve`) groups those rows into `Collection`s per a
//! `bids2nf.yaml` config. No `std::fs` — the shell supplies I/O.

pub mod collection;
pub mod config;
pub mod entities;
pub mod fs;
pub mod protocol;
pub mod resolve;
pub mod table;

pub use collection::{Collection, GroupedData, VolumeRef, Warning};
pub use config::{default_config, parse_config, Bids2nfConfig, SetDef};
pub use fs::{DatasetFs, Entry};
pub use resolve::{collections_for, resolve_set};
pub use table::{parse_to_table, BidsRow};
