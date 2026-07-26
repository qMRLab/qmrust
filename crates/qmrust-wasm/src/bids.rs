//! Resolve a BIDS dataset held in memory, with no filesystem.
//!
//! This is the resolution half of the CLI's `--bids-dir` path against a
//! [`MemFs`] instead of a real directory. Every step — the file table, the
//! collection grouping, the protocol composed from sidecars, the aux/mask
//! selection, the volume ordering — is the *same* `rust-bids` code the CLI runs,
//! so a dataset resolves identically whether it arrived as an unzipped archive,
//! a directory a browser handed over, or a path on disk.
//!
//! Nothing here reads pixel data. It answers only "which files matter and what
//! do they mean", handing back dataset-relative paths; the caller already holds
//! the bytes and loads the ones it is told to. That is what keeps this binding
//! ignorant of where its dataset came from — the property the whole design rests
//! on, since a "demo data" shortcut anywhere here would fork the code path a
//! reader's own data takes.

use crate::api::emit_protocol;
use qmrust_core::core::model::MeasurementKind;
use rust_bids::MemFs;

/// One resolved collection: which files, what they mean, and the acquisition
/// read from their sidecars.
pub struct ResolvedCollection {
    pub subject: String,
    pub session: Option<String>,
    pub suffix: String,
    /// Measurement volumes in the order the model consumes them — stack voxel
    /// data in exactly this order so column `i` matches `volume_ids_json[i]`.
    pub data_files: Vec<String>,
    pub mask_file: Option<String>,
    /// `(model input name, path)` for each declared aux input that was found.
    pub aux_files: Vec<(String, String)>,
    /// Ready to hand to `fit_volume`.
    pub volume_ids_json: String,
    /// The acquisition resolved from the sidecars, ready to hand to
    /// `fit_volume`. This is why a recipe for BIDS data carries options only.
    pub protocol_json: String,
    /// Resolution warnings, verbatim — the caller should surface these.
    pub warnings: Vec<String>,
}

/// Resolve every collection in `files` matching the model named by `cfg_yaml`.
///
/// `files` is the whole dataset as dataset-relative path → bytes; paths must
/// already be relative to the dataset root (the directory holding
/// `dataset_description.json`). `cfg_yaml` is the recipe — it names the model and
/// may carry a `mask:` block selecting which mask to apply. `grouping_yaml`
/// overrides the default grouping manifest.
///
/// An empty result is not an error: a dataset legitimately may hold nothing this
/// model can fit, and the caller reports that rather than treating it as a
/// failure.
pub fn resolve_bids(
    files: Vec<(String, Vec<u8>)>,
    cfg_yaml: &str,
    grouping_yaml: Option<&str>,
) -> Result<Vec<ResolvedCollection>, String> {
    let (cfg, raw) = qmrust_core::config::parse_config(cfg_yaml).map_err(|e| e.to_string())?;
    let entry = qmrust_core::registry::by_name(&cfg.model)
        .ok_or_else(|| format!("Unknown model: '{}'", cfg.model))?;

    // The model's structural declarations (protocol schema, declared aux,
    // measurement kind) are read before any collection's protocol is composed.
    let probe = (entry.describe)(&raw).map_err(|e| e.to_string())?;
    let schema = probe.protocol_schema();
    // The BIDS path resolves protocol from sidecars, so no `Source::Option`
    // fallbacks are supplied.
    let options = std::collections::BTreeMap::new();
    let named_roles: Option<&'static [&'static str]> = match probe.measurement() {
        MeasurementKind::Named { roles } => Some(roles),
        MeasurementKind::Series { .. } => None,
    };

    let fs = MemFs::from_files(files);
    let bids_cfg = match grouping_yaml {
        Some(yaml) => {
            rust_bids::parse_config(yaml).map_err(|e| format!("parsing grouping manifest: {e}"))?
        }
        None => rust_bids::default_config(),
    };
    let vocab = rust_bids::Vocabulary::from_config(&bids_cfg);
    let mask_spec = rust_bids::MaskSpec::from_recipe(&raw, &vocab).map_err(|e| e.to_string())?;

    // The table spans the whole dataset — raw tree and every `derivatives/`
    // pipeline — and is the single source aux inputs and masks are located from.
    let table = rust_bids::parse_to_table(&fs, &vocab).map_err(|e| e.to_string())?;
    let collections =
        rust_bids::collections_for(&fs, &bids_cfg, entry.bids_suffix).map_err(|e| e.to_string())?;

    let mut out = Vec::with_capacity(collections.len());
    for c in &collections {
        let data_files: Vec<String> = rust_bids::ordered_volume_paths(c, named_roles)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(str::to_string)
            .collect();
        let proto = rust_bids::compose_protocol(&fs, c, &schema, &options, named_roles)
            .map_err(|e| e.to_string())?;
        let volume_ids =
            qmrust_core::engine::build_volume_ids(probe.measurement(), &proto, data_files.len())?;
        let paths =
            rust_bids::resolve_input_paths(&table, probe.as_ref(), &c.entities, mask_spec.as_ref())
                .map_err(|e| e.to_string())?;

        out.push(ResolvedCollection {
            subject: c.subject.clone(),
            session: c.session.clone(),
            suffix: c.suffix.clone(),
            data_files,
            mask_file: paths.mask.clone(),
            aux_files: paths
                .aux
                .iter()
                .filter_map(|(name, p)| p.clone().map(|p| (name.clone(), p)))
                .collect(),
            volume_ids_json: emit_volume_ids(&volume_ids)?,
            protocol_json: emit_protocol(&proto)?,
            warnings: c.warnings.iter().map(|w| w.message.clone()).collect(),
        });
    }
    Ok(out)
}

/// Walk a real dataset directory into the path→bytes shape [`resolve_bids`]
/// takes. Native-only convenience for tests and desktop callers; a browser
/// builds the same map from an unzipped archive or a dropped directory.
#[cfg(not(target_arch = "wasm32"))]
pub fn read_dataset_dir(root: &std::path::Path) -> std::io::Result<Vec<(String, Vec<u8>)>> {
    fn walk(
        dir: &std::path::Path,
        root: &std::path::Path,
        out: &mut Vec<(String, Vec<u8>)>,
    ) -> std::io::Result<()> {
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, root, out)?;
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("walked under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&path)?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

/// Emit per-volume identities in the encoding `fit_volume`'s `volume_ids_json`
/// reads: role names for a `Named` measurement, param-row objects for a
/// `Series` one.
fn emit_volume_ids(ids: &[qmrust_core::core::model::VolumeId]) -> Result<String, String> {
    use qmrust_core::core::model::VolumeId;
    let json = match ids.first() {
        Some(VolumeId::Role(_)) => {
            let roles: Vec<&str> = ids
                .iter()
                .map(|id| match id {
                    VolumeId::Role(r) => Ok(*r),
                    VolumeId::Params(_) => Err("mixed Role and Params volume ids".to_string()),
                })
                .collect::<Result<_, String>>()?;
            serde_json::to_string(&roles)
        }
        Some(VolumeId::Params(_)) => {
            let rows: Vec<&std::collections::BTreeMap<String, f64>> = ids
                .iter()
                .map(|id| match id {
                    VolumeId::Params(p) => Ok(p),
                    VolumeId::Role(_) => Err("mixed Params and Role volume ids".to_string()),
                })
                .collect::<Result<_, String>>()?;
            serde_json::to_string(&rows)
        }
        None => serde_json::to_string(&Vec::<&str>::new()),
    };
    json.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but complete single-subject IRT1 dataset: three inversion times
    /// whose sidecars carry the acquisition, plus a brain mask in a
    /// `preprocessed` derivatives pipeline. The `.nii.gz` bytes are irrelevant —
    /// resolution never reads pixel data.
    fn irt1_dataset(tis: &[f64]) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![(
            "dataset_description.json".to_string(),
            br#"{"Name":"t","BIDSVersion":"1.8.0"}"#.to_vec(),
        )];
        for (i, ti) in tis.iter().enumerate() {
            let base = format!("sub-01/anat/sub-01_inv-{}_IRT1", i + 1);
            files.push((format!("{base}.nii.gz"), vec![0u8; 4]));
            files.push((
                format!("{base}.json"),
                format!(r#"{{"InversionTime": {ti}, "RepetitionTime": 2.5}}"#).into_bytes(),
            ));
        }
        files.push((
            "derivatives/preprocessed/sub-01/anat/sub-01_desc-brain_mask.nii.gz".to_string(),
            vec![0u8; 4],
        ));
        files
    }

    const IRT1_RECIPE: &str =
        "model: inversion_recovery\nmethod: magnitude\nmask:\n  desc: brain\n";

    /// The load-bearing contract: resolving a dataset held in memory yields the
    /// acquisition read from *its* sidecars — not from the recipe, which for BIDS
    /// input carries options only. The resolved protocol and volume identities
    /// must reproduce the dataset's own inversion times exactly, in the order the
    /// data files are given, so a caller can stack voxels and fit directly.
    #[test]
    fn resolves_protocol_from_the_datasets_own_sidecars() {
        let tis = [0.35, 0.65, 0.95];
        let got = resolve_bids(irt1_dataset(&tis), IRT1_RECIPE, None).unwrap();
        assert_eq!(got.len(), 1, "one subject → one collection");
        let c = &got[0];

        assert_eq!(c.subject, "sub-01");
        assert_eq!(c.suffix, "IRT1");
        assert_eq!(
            c.data_files,
            vec![
                "sub-01/anat/sub-01_inv-1_IRT1.nii.gz",
                "sub-01/anat/sub-01_inv-2_IRT1.nii.gz",
                "sub-01/anat/sub-01_inv-3_IRT1.nii.gz",
            ]
        );
        assert_eq!(
            c.mask_file.as_deref(),
            Some("derivatives/preprocessed/sub-01/anat/sub-01_desc-brain_mask.nii.gz")
        );

        // The sidecars' TIs, in data_files order — both in the protocol and in
        // the per-volume identities the model assembles signal by.
        let proto = crate::api::parse_protocol(&c.protocol_json).unwrap();
        let resolved: Vec<f64> = proto.volumes.iter().map(|v| v["InversionTime"]).collect();
        assert_eq!(resolved, tis);
        let ids: Vec<std::collections::BTreeMap<String, f64>> =
            serde_json::from_str(&c.volume_ids_json).unwrap();
        let id_tis: Vec<f64> = ids.iter().map(|m| m["InversionTime"]).collect();
        assert_eq!(id_tis, tis);
    }

    /// The protocol must track the data, never a remembered acquisition: the
    /// same recipe over a dataset with different inversion times must resolve to
    /// *that* dataset's values. This is what makes fitting a reader's own data
    /// correct rather than silently describing the demo dataset.
    #[test]
    fn protocol_follows_the_data_not_the_recipe() {
        let a = resolve_bids(irt1_dataset(&[0.35, 0.65, 0.95]), IRT1_RECIPE, None).unwrap();
        let b = resolve_bids(irt1_dataset(&[0.10, 0.20, 0.30]), IRT1_RECIPE, None).unwrap();
        assert_ne!(
            a[0].protocol_json, b[0].protocol_json,
            "the same recipe over different acquisitions must not resolve alike"
        );
        let tis = |c: &ResolvedCollection| -> Vec<f64> {
            crate::api::parse_protocol(&c.protocol_json)
                .unwrap()
                .volumes
                .iter()
                .map(|v| v["InversionTime"])
                .collect()
        };
        assert_eq!(tis(&b[0]), vec![0.10, 0.20, 0.30]);
    }

    /// A dataset holding nothing this model can fit resolves to zero
    /// collections, not an error — an expected outcome a caller reports, since a
    /// reader's own dataset may simply not match.
    #[test]
    fn a_dataset_without_this_models_data_resolves_to_nothing() {
        let files = vec![(
            "dataset_description.json".to_string(),
            br#"{"Name":"t","BIDSVersion":"1.8.0"}"#.to_vec(),
        )];
        assert!(resolve_bids(files, IRT1_RECIPE, None).unwrap().is_empty());
    }
}
