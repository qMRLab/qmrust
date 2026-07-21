//! The known-terms vocabulary a dataset is read against: canonical BIDS
//! suffixes/datatypes (from the spec) plus configurable extensions — a
//! dataset's own custom entities/suffixes, and every registered model's BIDS
//! suffix (so a model is discoverable the moment it's added to the registry,
//! with no config change). Threaded into `parse_to_table` so grouping and
//! `.bidsignore` exemption are never hardcoded to a fixed model list.

use crate::config::{BidsConfig, CustomEntity};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Canonical BIDS suffixes, transcribed verbatim from the BIDS specification.
const CANONICAL_SUFFIXES: &[&str] = &[
    "2PE",
    "ADC",
    "BF",
    "Chimap",
    "CARS",
    "CONF",
    "DIC",
    "DF",
    "FA",
    "FLAIR",
    "FLASH",
    "FLUO",
    "IRT1",
    "M0map",
    "MEGRE",
    "MESE",
    "MP2RAGE",
    "MPE",
    "MPM",
    "MTR",
    "MTRmap",
    "MTS",
    "MTVmap",
    "MTsat",
    "MWFmap",
    "NLO",
    "OCT",
    "PC",
    "PD",
    "PDT2",
    "PDmap",
    "PDw",
    "PLI",
    "R1map",
    "R2map",
    "R2starmap",
    "RB1COR",
    "RB1map",
    "S0map",
    "SEM",
    "SPIM",
    "SR",
    "T1map",
    "T1rho",
    "T1w",
    "T2map",
    "T2star",
    "T2starmap",
    "T2starw",
    "T2w",
    "TB1AFI",
    "TB1DAM",
    "TB1EPI",
    "TB1RFM",
    "TB1SRGE",
    "TB1TFL",
    "TB1map",
    "TEM",
    "UNIT1",
    "VFA",
    "angio",
    "asl",
    "aslcontext",
    "asllabeling",
    "beh",
    "blood",
    "bold",
    "cbv",
    "channels",
    "colFA",
    "coordsystem",
    "defacemask",
    "description",
    "descriptions",
    "dseg",
    "dwi",
    "eeg",
    "electrodes",
    "emg",
    "epi",
    "events",
    "expADC",
    "fieldmap",
    "headshape",
    "XPCT",
    "ieeg",
    "inplaneT1",
    "inplaneT2",
    "m0scan",
    "magnitude",
    "magnitude1",
    "magnitude2",
    "markers",
    "mask",
    "meg",
    "motion",
    "mrsi",
    "mrsref",
    "nirs",
    "noRF",
    "optodes",
    "pet",
    "phase",
    "phase1",
    "phase2",
    "phasediff",
    "photo",
    "physio",
    "physioevents",
    "probseg",
    "sbref",
    "scans",
    "sessions",
    "stim",
    "svs",
    "trace",
    "uCT",
    "unloc",
];

/// Canonical BIDS datatype directory names, transcribed verbatim from the spec.
const CANONICAL_DATATYPES: &[&str] = &[
    "anat",
    "beh",
    "dwi",
    "eeg",
    "emg",
    "fmap",
    "func",
    "ieeg",
    "meg",
    "micr",
    "motion",
    "mrs",
    "nirs",
    "perf",
    "pet",
    "phenotype",
];

/// The known-terms vocabulary: canonical BIDS built-ins plus configurable
/// extensions (a dataset's custom entities/suffixes and every registered
/// model's BIDS suffix).
pub struct Vocabulary {
    suffixes: BTreeSet<&'static str>,
    datatypes: BTreeSet<&'static str>,
    custom_entities: BTreeMap<String, String>,
    custom_suffixes: BTreeSet<String>,
}

impl Vocabulary {
    /// The built-in vocabulary qmrust knows with no dataset config: canonical
    /// BIDS terms plus every registered model's BIDS suffix. Registered models
    /// are a compile-time fact (not configuration), so their suffixes — e.g.
    /// the custom `QMTSPGR` — are known and `.bidsignore`-exempt by default;
    /// `from_config` only layers a *dataset's* own declared customs on top.
    pub fn bids() -> Self {
        let mut vocab = Vocabulary {
            suffixes: CANONICAL_SUFFIXES.iter().copied().collect(),
            datatypes: CANONICAL_DATATYPES.iter().copied().collect(),
            custom_entities: BTreeMap::new(),
            custom_suffixes: BTreeSet::new(),
        };
        for entry in qmrust_core::registry::all() {
            vocab.custom_suffixes.insert(entry.bids_suffix.to_string());
        }
        vocab
    }

    /// The built-in vocabulary extended with this dataset's declared
    /// `custom_entities`/`custom_suffixes`.
    pub fn from_config(cfg: &BidsConfig) -> Self {
        let mut vocab = Self::bids();
        for CustomEntity { key, name } in &cfg.custom_entities {
            vocab.custom_entities.insert(key.clone(), name.clone());
        }
        for suffix in &cfg.custom_suffixes {
            vocab.custom_suffixes.insert(suffix.clone());
        }
        vocab
    }

    /// The full name a custom entity key normalizes to, or the key itself if
    /// it isn't declared as a custom entity (canonical keys are already
    /// normalized upstream by `parse_filename`).
    pub fn normalize_entity_key(&self, key: &str) -> String {
        self.custom_entities
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_string())
    }

    /// Whether `s` is known to this vocabulary at all: canonical, or a
    /// registered/config custom suffix.
    pub fn is_known_suffix(&self, s: &str) -> bool {
        self.suffixes.contains(s) || self.custom_suffixes.contains(s)
    }

    /// Whether `s` is a *custom* (non-canonical) suffix: registered model or
    /// config-declared, but NOT a canonical BIDS suffix.
    pub fn is_custom_suffix(&self, s: &str) -> bool {
        self.custom_suffixes.contains(s)
    }

    /// Whether `s` is one of the 16 canonical BIDS datatype directory names.
    pub fn is_datatype(&self, s: &str) -> bool {
        self.datatypes.contains(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;

    #[test]
    fn bids_vocabulary_knows_canonical_and_registered_terms() {
        let v = Vocabulary::bids();
        assert!(v.is_known_suffix("T1map"));
        assert!(v.is_known_suffix("IRT1"));
        // A canonical suffix is known but not "custom".
        assert!(!v.is_custom_suffix("T1map"));
        // Registered model suffixes are built in (compile-time), so known and
        // custom (hence `.bidsignore`-exempt) with no config.
        assert!(v.is_known_suffix("QMTSPGR"));
        assert!(v.is_custom_suffix("QMTSPGR"));
        assert!(v.is_datatype("anat"));
        assert!(v.is_datatype("fmap"));
        assert!(!v.is_datatype("notadatatype"));
    }

    #[test]
    fn from_config_folds_registered_and_declared_customs() {
        let v = Vocabulary::from_config(&default_config());
        assert!(v.is_known_suffix("QMTSPGR"));
        assert!(v.is_custom_suffix("QMTSPGR"));
        assert!(!v.is_custom_suffix("T1map"));
    }

    #[test]
    fn from_config_folds_in_declared_customs() {
        let cfg = crate::config::parse_config(
            r#"
loop_over: [subject, session, run, task]
custom_entities:
  - key: myent
    name: myentity
custom_suffixes: [MYSUFFIX]
"#,
        )
        .unwrap();
        let v = Vocabulary::from_config(&cfg);
        assert!(v.is_known_suffix("MYSUFFIX"));
        assert!(v.is_custom_suffix("MYSUFFIX"));
        assert_eq!(v.normalize_entity_key("myent"), "myentity");
        assert_eq!(v.normalize_entity_key("unrelated"), "unrelated");
    }
}
