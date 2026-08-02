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

/// A volume's identity, mirroring `qmrust_core::VolumeId`: exactly one field
/// is populated — `role` for a `Named` measurement (MTw, PDw, T1w), `params`
/// for a `Series` one ({"InversionTime": 0.65}).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct VolumeIdentity {
    pub role: Option<String>,
    pub params: std::collections::BTreeMap<String, f64>,
}

impl From<&qmrust_core::core::model::VolumeId> for VolumeIdentity {
    fn from(id: &qmrust_core::core::model::VolumeId) -> Self {
        use qmrust_core::core::model::VolumeId;
        match id {
            VolumeId::Role(r) => Self {
                role: Some(r.to_string()),
                params: Default::default(),
            },
            VolumeId::Params(p) => Self {
                role: None,
                params: p.clone(),
            },
        }
    }
}

/// What resolution made of one file. Every file the caller supplies gets
/// exactly one verdict, so a reader sees the whole dataset — including what
/// was *not* selected, which is half of what makes resolution comprehensible.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileRole {
    /// Measurement volume `index` of `total`, with the identity it resolved to.
    Volume {
        index: usize,
        total: usize,
        identity: VolumeIdentity,
    },
    /// Metadata that merged into these volumes' protocol. Several volumes when
    /// BIDS inheritance applies it to a whole directory level.
    Sidecar {
        applies_to: Vec<String>,
    },
    Mask,
    /// A declared model input, by its own name ("B1map", "R1map").
    Aux {
        input: String,
    },
    /// Read, but not a fitting input.
    DatasetMetadata,
    /// Not selected, and why.
    Unused {
        reason: String,
    },
}

/// One resolved collection: which files, what they mean, and the acquisition
/// read from their sidecars.
#[derive(Debug, Clone, serde::Serialize)]
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
    /// One verdict per file the caller supplied, in path order — the whole
    /// dataset as resolution saw it.
    pub files: Vec<(String, FileRole)>,
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
        let files = annotate_files(
            &fs,
            &table,
            &data_files,
            &volume_ids,
            &paths,
            entry.bids_suffix,
            &c.subject,
        )?;

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
            // Grouping warnings and input-resolution warnings reach the reader
            // through the same channel: both say the dataset did not hold
            // something the recipe asked for.
            warnings: c
                .warnings
                .iter()
                .map(|w| w.message.clone())
                .chain(paths.warnings.iter().cloned())
                .collect(),
            files,
        });
    }
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

/// Files a dataset carries that resolution reads but never fits.
fn is_dataset_metadata(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    matches!(
        name,
        "dataset_description.json" | "participants.tsv" | ".bidsignore" | "README" | "CHANGES"
    )
}

/// Why a file was not selected, in terms a reader can act on. Driven by the
/// file table's own parse of the path, so it never hardcodes a model or suffix.
///
/// `loaded_subject` is the collection being displayed (e.g. `"sub-01"`), or
/// `None` when no collection resolved at all — there is then no subject to
/// contrast against, and a suffix mismatch is the real answer.
fn unused_reason(
    table: &[rust_bids::BidsRow],
    path: &str,
    wanted_suffix: &str,
    loaded_subject: Option<&str>,
) -> String {
    let Some(row) = table.iter().find(|r| r.path == path) else {
        return "not a recognized BIDS file".to_string();
    };
    if row.derivatives.as_deref() == Some("qmrust") {
        return "a reference fit output, not a fitting input".to_string();
    }
    if row.suffix != wanted_suffix {
        return format!("suffix {}, not this model's {wanted_suffix}", row.suffix);
    }
    if let (Some(loaded), Some(s)) = (loaded_subject, row.entities.get("subject")) {
        if s != loaded.trim_start_matches("sub-") {
            return format!("subject sub-{s}, not the loaded {loaded}");
        }
    }
    "not selected for this collection".to_string()
}

/// One verdict per supplied file, for the collection being displayed.
///
/// Sidecar attribution comes from `sidecar_sources_for` — what actually merged
/// — because under BIDS inheritance a volume's protocol often comes from a file
/// in a different directory, which filename pairing would call unused.
fn annotate_files(
    fs: &MemFs,
    table: &[rust_bids::BidsRow],
    data_files: &[String],
    volume_ids: &[qmrust_core::core::model::VolumeId],
    inputs: &rust_bids::InputPaths,
    wanted_suffix: &str,
    loaded_subject: &str,
) -> Result<Vec<(String, FileRole)>, String> {
    let loaded_subject = Some(loaded_subject);
    let total = data_files.len();
    let mut roles: std::collections::BTreeMap<String, FileRole> = Default::default();

    for (index, (path, id)) in data_files.iter().zip(volume_ids).enumerate() {
        roles.insert(
            path.clone(),
            FileRole::Volume {
                index,
                total,
                identity: VolumeIdentity::from(id),
            },
        );
        for src in rust_bids::sidecar_sources_for(fs, path).map_err(|e| e.to_string())? {
            // A file that is both a volume and a sidecar cannot happen
            // (different extensions); leave the volume verdict alone.
            if let FileRole::Sidecar { applies_to } =
                roles.entry(src).or_insert_with(|| FileRole::Sidecar {
                    applies_to: Vec::new(),
                })
            {
                applies_to.push(path.clone());
            }
        }
    }
    if let Some(mask) = &inputs.mask {
        roles.insert(mask.clone(), FileRole::Mask);
    }
    for (input, path) in &inputs.aux {
        if let Some(path) = path {
            roles.insert(
                path.clone(),
                FileRole::Aux {
                    input: input.clone(),
                },
            );
        }
    }

    Ok(fs
        .paths()
        .map(|path| {
            let role = roles.remove(path).unwrap_or_else(|| {
                if is_dataset_metadata(path) {
                    FileRole::DatasetMetadata
                } else {
                    FileRole::Unused {
                        reason: unused_reason(table, path, wanted_suffix, loaded_subject),
                    }
                }
            });
            (path.to_string(), role)
        })
        .collect())
}

/// Verdicts for a dataset that resolved to no collections: every file gets an
/// `Unused` reason naming the mismatch. Without this, a reader whose data does
/// not match sees an empty panel and no explanation.
pub fn annotate_non_matching(
    files: Vec<(String, Vec<u8>)>,
    cfg_yaml: &str,
) -> Result<Vec<(String, FileRole)>, String> {
    // Only the model's declared suffix is needed here: with no collections
    // there is no protocol to compose, so the recipe's options go unread.
    let (cfg, _) = qmrust_core::config::parse_config(cfg_yaml).map_err(|e| e.to_string())?;
    let entry = qmrust_core::registry::by_name(&cfg.model)
        .ok_or_else(|| format!("Unknown model: '{}'", cfg.model))?;
    let fs = MemFs::from_files(files);
    let vocab = rust_bids::Vocabulary::from_config(&rust_bids::default_config());
    let table = rust_bids::parse_to_table(&fs, &vocab).map_err(|e| e.to_string())?;
    Ok(fs
        .paths()
        .map(|path| {
            let role = if is_dataset_metadata(path) {
                FileRole::DatasetMetadata
            } else {
                FileRole::Unused {
                    reason: unused_reason(&table, path, entry.bids_suffix, None),
                }
            };
            (path.to_string(), role)
        })
        .collect())
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

    /// Extend the IRT1 fixture with files resolution must account for but not
    /// select: another model's acquisition, a second subject, and a reference fit
    /// output.
    fn irt1_dataset_with_distractors(tis: &[f64]) -> Vec<(String, Vec<u8>)> {
        let mut files = irt1_dataset(tis);
        files.push((
            "sub-01/anat/sub-01_flip-1_mt-off_MTS.nii.gz".to_string(),
            vec![0u8; 4],
        ));
        files.push((
            "sub-02/anat/sub-02_inv-1_IRT1.nii.gz".to_string(),
            vec![0u8; 4],
        ));
        files.push((
            "sub-02/anat/sub-02_inv-1_IRT1.json".to_string(),
            br#"{"InversionTime": 0.5, "RepetitionTime": 2.5}"#.to_vec(),
        ));
        files.push((
            "derivatives/qmrust/sub-01/anat/sub-01_T1map.nii.gz".to_string(),
            vec![0u8; 4],
        ));
        files.push(("participants.tsv".to_string(), b"participant_id\n".to_vec()));
        files
    }

    /// Totality: every file handed in gets exactly one verdict. A file silently
    /// absent is the bug this panel exists to expose, so it is asserted, and a
    /// duplicate would double-render a row.
    #[test]
    fn every_supplied_file_gets_exactly_one_verdict() {
        let files = irt1_dataset_with_distractors(&[0.35, 0.65, 0.95]);
        let supplied: std::collections::BTreeSet<String> =
            files.iter().map(|(p, _)| p.clone()).collect();

        let got = resolve_bids(files, IRT1_RECIPE, None).unwrap();
        let sub01 = got
            .iter()
            .find(|c| c.subject == "sub-01")
            .expect("sub-01 must resolve");
        let verdicts = &sub01.files;

        let annotated: std::collections::BTreeSet<String> =
            verdicts.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(annotated, supplied, "every supplied file, and only those");
        assert_eq!(
            verdicts.len(),
            supplied.len(),
            "exactly one verdict per file (no duplicates)"
        );
    }

    /// An inherited sidecar contributes to the protocol, so it must be attributed
    /// to the volumes it fed — never marked unused. Computing verdicts in Rust
    /// exists for this case; filename pairing in JS would get it wrong.
    #[test]
    fn an_inherited_sidecar_is_attributed_not_orphaned() {
        let mut files = irt1_dataset(&[0.35, 0.65, 0.95]);
        // A subject-level sidecar applying to all three volumes.
        files.push((
            "sub-01/sub-01_IRT1.json".to_string(),
            br#"{"RepetitionTime": 2.5}"#.to_vec(),
        ));

        let got = resolve_bids(files, IRT1_RECIPE, None).unwrap();
        let role = got[0]
            .files
            .iter()
            .find(|(p, _)| p == "sub-01/sub-01_IRT1.json")
            .map(|(_, r)| r)
            .expect("the inherited sidecar must be present");

        match role {
            FileRole::Sidecar { applies_to } => assert_eq!(
                applies_to.len(),
                3,
                "an inherited sidecar feeds every volume it applies to: {applies_to:?}"
            ),
            other => panic!("expected Sidecar, got {other:?}"),
        }
    }

    /// A Named model's volumes are identified by role, not by numeric params, so
    /// the identity must carry the role name — otherwise MTS rows would show no
    /// identity at all.
    #[test]
    fn a_named_models_volume_identity_carries_its_role() {
        let files = vec![
            (
                "dataset_description.json".to_string(),
                br#"{"Name":"t","BIDSVersion":"1.8.0"}"#.to_vec(),
            ),
            (
                "sub-01/anat/sub-01_mt-on_MTR.nii.gz".to_string(),
                vec![0u8; 4],
            ),
            (
                "sub-01/anat/sub-01_mt-on_MTR.json".to_string(),
                b"{}".to_vec(),
            ),
            (
                "sub-01/anat/sub-01_mt-off_MTR.nii.gz".to_string(),
                vec![0u8; 4],
            ),
            (
                "sub-01/anat/sub-01_mt-off_MTR.json".to_string(),
                b"{}".to_vec(),
            ),
        ];
        let got = resolve_bids(files, "model: mt_ratio\n", None).unwrap();

        let roles: Vec<String> = got[0]
            .files
            .iter()
            .filter_map(|(_, r)| match r {
                FileRole::Volume { identity, .. } => identity.role.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(roles.len(), 2, "both MTR volumes carry a role: {roles:?}");
        assert!(roles.contains(&"MTon".to_string()), "{roles:?}");
        assert!(roles.contains(&"MToff".to_string()), "{roles:?}");
    }

    /// A dataset holding nothing this model can fit must explain itself per file,
    /// not present an empty panel. This is the diagnostic case a reader hits when
    /// dropping their own data.
    #[test]
    fn a_non_matching_dataset_explains_every_file() {
        let files = vec![
            (
                "dataset_description.json".to_string(),
                br#"{"Name":"t","BIDSVersion":"1.8.0"}"#.to_vec(),
            ),
            (
                "sub-01/anat/sub-01_flip-1_mt-off_MTS.nii.gz".to_string(),
                vec![0u8; 4],
            ),
        ];
        let verdicts = annotate_non_matching(files.clone(), IRT1_RECIPE).unwrap();

        assert_eq!(verdicts.len(), files.len(), "every file accounted for");
        let mts = verdicts
            .iter()
            .find(|(p, _)| p.ends_with("_MTS.nii.gz"))
            .map(|(_, r)| r)
            .unwrap();
        match mts {
            FileRole::Unused { reason } => {
                assert!(reason.contains("MTS"), "names what it is: {reason}");
                assert!(reason.contains("IRT1"), "names what was wanted: {reason}");
            }
            other => panic!("expected Unused, got {other:?}"),
        }
    }
}
