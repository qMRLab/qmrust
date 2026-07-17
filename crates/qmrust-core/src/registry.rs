//! Model registry: the one place that maps a config `model:` name (and a BIDS
//! grouping suffix) to the builder that constructs it. Adding a model means a
//! new module implementing `Model` plus one entry here.

use crate::core::model::{Model, Protocol};
use crate::models;
use anyhow::Result;

pub type Builder = fn(&serde_yaml::Value, &Protocol) -> Result<Box<dyn Model>>;

pub struct ModelEntry {
    pub name: &'static str,
    pub bids_suffix: &'static str,
    pub build: Builder,
}

pub fn all() -> &'static [ModelEntry] {
    &[
        ModelEntry {
            name: "inversion_recovery",
            bids_suffix: "IRT1",
            build: models::inversion_recovery::build,
        },
        ModelEntry {
            name: "qmt_spgr",
            bids_suffix: "MTS",
            build: models::qmt_spgr::build,
        },
    ]
}

pub fn by_name(name: &str) -> Option<&'static ModelEntry> {
    all().iter().find(|e| e.name == name)
}

pub fn by_bids_suffix(suffix: &str) -> Option<&'static ModelEntry> {
    all().iter().find(|e| e.bids_suffix == suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_name() {
        assert!(by_name("inversion_recovery").is_some());
        assert!(by_name("qmt_spgr").is_some());
        assert!(by_name("nope").is_none());
    }

    #[test]
    fn lookup_by_bids_suffix() {
        assert_eq!(by_bids_suffix("IRT1").unwrap().name, "inversion_recovery");
        assert_eq!(by_bids_suffix("MTS").unwrap().name, "qmt_spgr");
    }

    #[test]
    fn builds_via_registry() {
        let v: serde_yaml::Value = serde_yaml::from_str("model: qmt_spgr\n").unwrap();
        let entry = by_name("qmt_spgr").unwrap();
        let m = (entry.build)(&v, &crate::core::model::Protocol::default()).unwrap();
        assert_eq!(m.output_names().len(), 8);
    }
}
