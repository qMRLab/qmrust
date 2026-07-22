use crate::{collections_for, BidsConfig, Collection, DatasetFs};
use std::collections::BTreeMap;

/// Every registered model's fittable collections in `fs`, keyed by BIDS suffix.
/// Registry-driven — a new model appears here with no change to this function.
///
/// This is the multi-model entry point: unlike `run_fit_bids` (which resolves
/// a single suffix for one already-chosen model), `scan_dataset` answers "what
/// can I fit in this dataset" across all registered models before a model is
/// picked.
pub fn scan_dataset<F: DatasetFs>(fs: &F, cfg: &BidsConfig) -> BTreeMap<String, Vec<Collection>> {
    let mut out = BTreeMap::new();
    for entry in qmrust_core::registry::all() {
        if let Ok(cols) = collections_for(fs, cfg, entry.bids_suffix) {
            if !cols.is_empty() {
                out.insert(entry.bids_suffix.to_string(), cols);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_config, fs::MemFs};

    #[test]
    fn scans_irt1_collections_keyed_by_suffix() {
        let mut fs = MemFs::new();
        for i in 1..=4 {
            fs = fs
                .touch(&format!("sub-01/anat/sub-01_inv-0{i}_IRT1.nii.gz"))
                .with(
                    &format!("sub-01/anat/sub-01_inv-0{i}_IRT1.json"),
                    b"{}".to_vec(),
                );
        }
        let map = scan_dataset(&fs, &default_config());
        assert!(map.contains_key("IRT1"));
        assert_eq!(map["IRT1"].len(), 1);
    }
}
