//! `qmrust bidsify`: convert a qMRLab dataset — a `.mat` or a 4D NIfTI — into a
//! BIDS layout whose voxel data is byte-identical to the source (no rescale, no
//! dtype narrowing — every volume is written as `f64`/datatype 64). A NIfTI
//! source's spatial header (affine/pixdim/qform/sform) is preserved; a `.mat`
//! source, which carries no header, gets a minimal one.
//!
//! Model-agnostic: every write decision — the BIDS suffix, per-volume
//! filename entities, sidecar metadata, and which auxiliary maps to look for
//! — comes from the registry-resolved `Model` (`Model::bids`,
//! `Model::bids_volume`, `Model::required_inputs`). Adding a model here needs
//! no change to this file.

use anyhow::Result;
use ndarray::{Array3, Array4, Axis};
use nifti::NiftiHeader;
use std::path::{Path, PathBuf};

use crate::commands::make_minimal_header;
use crate::io;
use qmrust_core::core::model::{MeasurementKind, Model};

/// Parsed CLI arguments for `qmrust bidsify`.
pub struct BidsifyArgs {
    pub model: String,
    pub mat_data: Option<PathBuf>,
    pub mat_dir: Option<PathBuf>,
    pub nii_data: Option<PathBuf>,
    pub nii_mask: Option<PathBuf>,
    pub mask: Option<PathBuf>,
    pub config: PathBuf,
    pub subject: String,
    pub out: PathBuf,
}

/// Entry point for the `bidsify` subcommand: resolves the model from the
/// registry, reads the source dataset (`.mat` or 4D NIfTI, + optional mask +
/// aux maps), and writes the BIDS tree the model's own `bids()`/
/// `bids_volume()`/`required_inputs()` describe.
pub fn run_bidsify(args: BidsifyArgs) -> Result<()> {
    let contents = std::fs::read_to_string(&args.config)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", args.config, e))?;
    let (_cfg, raw) = qmrust_core::config::parse_config(&contents)?;
    let entry = qmrust_core::registry::by_name(&args.model)
        .ok_or_else(|| anyhow::anyhow!("Unknown model: '{}'", args.model))?;
    // The recipe supplies the acquisition: bidsify writes the BIDS sidecars
    // from it, so there is no BIDS protocol to read here.
    let model = (entry.describe)(&raw)?;

    // A NIfTI source carries a real spatial header (preserved); a `.mat` source
    // carries none (a minimal header is synthesized in `write_bids_tree`). Each
    // source reads its mask from its own flag, so reject the other source's
    // mask flag rather than silently ignoring it.
    let (data, mask, aux, source_header) = if let Some(nii) = args.nii_data.as_ref() {
        anyhow::ensure!(
            args.mat_data.is_none() && args.mat_dir.is_none(),
            "--nii-data is mutually exclusive with --mat-data/--mat-dir"
        );
        anyhow::ensure!(
            args.mask.is_none(),
            "--mask is for a .mat source; pass --nii-mask with --nii-data"
        );
        read_nifti_source(nii, args.nii_mask.as_deref(), model.as_ref())?
    } else {
        anyhow::ensure!(
            args.nii_mask.is_none(),
            "--nii-mask is for a NIfTI source; pass --mask with --mat-data/--mat-dir"
        );
        read_mat_source(&args, model.as_ref())?
    };

    write_bids_tree(
        model.as_ref(),
        &data,
        mask.as_ref(),
        &aux,
        &args.subject,
        &args.out,
        source_header.as_ref(),
    )
}

/// A read measurement: 4D data, optional mask, declared aux maps, and the
/// source spatial header (`Some` for a NIfTI source, `None` for a `.mat`).
type Source = (
    Array4<f64>,
    Option<Array3<bool>>,
    Vec<(String, Array3<f64>)>,
    Option<NiftiHeader>,
);

/// Read a `.mat` measurement (+ optional mask + declared aux maps).
///
/// A `Series` model reads one stacked measurement array (`--mat-data`, or the
/// lone measurement `.mat` in `--mat-dir`). A `Named` model instead reads one
/// single-variable `<role>.mat` per role from `--mat-dir` (e.g. MTR's
/// `MTon.mat`/`MToff.mat`), stacked in the model's declared role order.
fn read_mat_source(args: &BidsifyArgs, model: &dyn Model) -> Result<Source> {
    // --mask wins; then a Mask.mat found via --mat-dir; then, for a stacked
    // measurement, one embedded in the source .mat itself.
    let mat_dir_mask = args
        .mat_dir
        .as_ref()
        .map(|d| d.join("Mask.mat"))
        .filter(|p| p.exists());
    let explicit_mask = args.mask.as_ref().or(mat_dir_mask.as_ref());

    let (data, embedded_mask) = match model.measurement() {
        MeasurementKind::Named { roles } => {
            let dir = args.mat_dir.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "a named model reads one <role>.mat per role, so bidsify needs --mat-dir \
                     (got --mat-data or nothing)"
                )
            })?;
            (io::mat::read_named_mat_volumes(dir, roles)?, None)
        }
        MeasurementKind::Series { .. } => {
            let mat_data_path = match args.mat_data.clone() {
                Some(p) => p,
                None => {
                    let dir = args.mat_dir.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("bidsify needs --mat-data, --mat-dir, or --nii-data")
                    })?;
                    measurement_in_dir(dir, model)?
                }
            };
            let mat = io::mat::read_mat_file(&mat_data_path)?;
            (mat.data, mat.mask)
        }
    };

    let mask = match explicit_mask {
        Some(p) => Some(io::mat::read_mask_mat(p)?),
        None => embedded_mask,
    };

    // Every auxiliary input the model declares (by logical name) is looked
    // up as `<name>.mat` under --mat-dir, if supplied.
    let aux: Vec<(String, Array3<f64>)> = model
        .required_inputs()
        .into_iter()
        .filter_map(|spec| {
            let path = args
                .mat_dir
                .as_ref()
                .map(|d| d.join(format!("{}.mat", spec.name)))
                .filter(|p| p.exists())?;
            Some(io::mat::read_map_mat(&path).map(|m| (spec.name.to_string(), m)))
        })
        .collect::<Result<Vec<_>>>()?;

    Ok((data, mask, aux, None))
}

/// Read a 4D NIfTI measurement (+ optional NIfTI mask), preserving its spatial
/// header. Auxiliary NIfTI inputs are not resolved here — no NIfTI-shipped
/// model currently declares any.
fn read_nifti_source(nii: &Path, nii_mask: Option<&Path>, _model: &dyn Model) -> Result<Source> {
    let (data, header) = io::nifti::read_4d_nifti(nii)?;
    let mask = match nii_mask {
        Some(p) => Some(io::nifti::read_mask_nifti(p)?),
        None => None,
    };
    Ok((data, mask, Vec::new(), Some(header)))
}

/// Locate the measurement `.mat` in `dir`: the single top-level `.mat` file
/// that is neither a declared auxiliary input (`<name>.mat` for each
/// `required_inputs()`) nor the mask (`Mask.mat`). Model-agnostic — the
/// measurement filename is never hardcoded; it is whatever remains after the
/// model's own declared inputs are excluded. Zero or several candidates is an
/// error asking for an explicit `--mat-data`.
fn measurement_in_dir(dir: &Path, model: &dyn Model) -> Result<PathBuf> {
    let mut excluded: std::collections::BTreeSet<String> = model
        .required_inputs()
        .iter()
        .map(|s| format!("{}.mat", s.name))
        .collect();
    excluded.insert("Mask.mat".to_string());

    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("reading --mat-dir {:?}: {}", dir, e))?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "mat"))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !excluded.contains(n))
        })
        .collect();
    candidates.sort();

    match candidates.as_slice() {
        [one] => Ok(one.clone()),
        [] => anyhow::bail!(
            "no measurement .mat found in {:?} (every .mat matched a declared aux or Mask.mat); pass --mat-data explicitly",
            dir
        ),
        many => {
            let names: Vec<_> = many.iter().filter_map(|p| p.file_name()).collect();
            anyhow::bail!(
                "multiple candidate measurement .mat files in {:?}: {:?}; pass --mat-data to disambiguate",
                dir,
                names
            )
        }
    }
}

/// Write one dataset as a BIDS tree rooted at `out`, driven by
/// `model`'s declared BIDS identity: `dataset_description.json`,
/// `participants.tsv`, a `.bidsignore` entry if `model`'s suffix is
/// non-canonical, one `sub-<subject>/anat/sub-<subject>[_<entity>-<val>...]_<suffix>.nii.gz`
/// (+ JSON sidecar, from `model.bids_volume(i)`) per volume in `data`'s 4th
/// axis, a brain-mask derivative if `mask` is given, and each `aux` map
/// placed under its declared BIDS suffix in the `preprocessed` derivatives
/// pipeline (`anat/` or `fmap/`, by BIDS field-map convention).
pub fn write_bids_tree(
    model: &dyn Model,
    data: &Array4<f64>,
    mask: Option<&Array3<bool>>,
    aux: &[(String, Array3<f64>)],
    subject: &str,
    out: &Path,
    source_header: Option<&NiftiHeader>,
) -> Result<()> {
    let spec = model
        .bids()
        .ok_or_else(|| anyhow::anyhow!("model declares no BIDS suffix"))?;

    let (nx, ny, nz, n) = data.dim();
    anyhow::ensure!(
        n == model.n_volumes(),
        "data has {n} volumes but the model's protocol describes {}",
        model.n_volumes()
    );

    std::fs::create_dir_all(out)?;
    write_dataset_description(out)?;
    write_participants_row(out, subject)?;
    write_bidsignore_if_custom(out, spec.suffix)?;

    let anat_dir = out.join(format!("sub-{subject}")).join("anat");
    std::fs::create_dir_all(&anat_dir)?;
    // A NIfTI source's spatial header is preserved; a `.mat` source carries
    // none, so emit qMRLab's make_nii-compatible minimal header (matches how
    // `qmrust fit --mat-data` treats .mat input). The volume writers force
    // datatype 64 regardless, so voxel data is always written as f64.
    let header = match source_header {
        Some(h) => h.clone(),
        None => make_minimal_header(nx, ny, nz),
    };

    for i in 0..n {
        let vol = data.index_axis(Axis(3), i).to_owned();
        let bv = model.bids_volume(i);
        let base = filename_stem(subject, &bv.entities, spec.suffix);
        write_inv_volume(&vol, &header, &anat_dir.join(format!("{base}.nii.gz")))?;
        std::fs::write(
            anat_dir.join(format!("{base}.json")),
            serde_json::to_string_pretty(&bv.sidecar)?,
        )?;
    }

    if let Some(mask) = mask {
        write_mask(out, subject, mask, &header)?;
    }

    for spec_in in model.required_inputs() {
        let Some(bmap) = spec_in.bids.as_ref() else {
            continue;
        };
        if let Some((_, vol)) = aux.iter().find(|(name, _)| name == spec_in.name) {
            write_aux_map(out, subject, bmap.suffix, vol, &header)?;
        }
    }

    Ok(())
}

/// Build a filename stem `sub-<subject>[_<entity>-<value>...]_<suffix>` from
/// a model's per-volume filename entities and BIDS suffix.
fn filename_stem(subject: &str, entities: &[(&'static str, String)], suffix: &str) -> String {
    let mut stem = format!("sub-{subject}");
    for (key, value) in entities {
        stem.push_str(&format!("_{key}-{value}"));
    }
    stem.push('_');
    stem.push_str(suffix);
    stem
}

/// The datatype directory (`fmap` or `anat`) an auxiliary BIDS suffix belongs
/// in, per BIDS field-map convention: `*B1map`/`*B0map` suffixes are
/// estimated field maps (`fmap`); everything else (R1 maps, brain masks, …)
/// is an anatomical derivative (`anat`).
fn aux_datatype(suffix: &str) -> &'static str {
    if suffix.ends_with("B1map") || suffix.ends_with("B0map") {
        "fmap"
    } else {
        "anat"
    }
}

/// The physical unit a known field-map BIDS suffix is conventionally
/// recorded in, for the `Units` sidecar `write_aux_map` writes alongside a
/// `fmap`-datatype map. `None` for suffixes that get no unit sidecar (e.g.
/// `anat`-datatype maps).
fn aux_units(suffix: &str) -> Option<&'static str> {
    if suffix.ends_with("B1map") {
        Some("1")
    } else if suffix.ends_with("B0map") {
        Some("Hz")
    } else {
        None
    }
}

/// Write one auxiliary map into the `preprocessed` derivatives pipeline,
/// under the datatype directory its BIDS suffix implies (see
/// `aux_datatype`), with a `Units` sidecar for field-map-datatype suffixes.
fn write_aux_map(
    out: &Path,
    subject: &str,
    suffix: &str,
    vol: &Array3<f64>,
    header: &NiftiHeader,
) -> Result<()> {
    let datatype = aux_datatype(suffix);
    let dir = preprocessed_datatype_dir(out, subject, datatype)?;
    let base = format!("sub-{subject}_{suffix}");
    write_inv_volume(vol, header, &dir.join(format!("{base}.nii.gz")))?;
    if let Some(units) = aux_units(suffix) {
        let sidecar = serde_json::json!({ "Units": units });
        std::fs::write(
            dir.join(format!("{base}.json")),
            serde_json::to_string_pretty(&sidecar)?,
        )?;
    }
    Ok(())
}

/// Write a brain mask into the `preprocessed` pipeline's `anat/` datatype — a
/// mask is an anatomical derivative regardless of which model requested it.
fn write_mask(out: &Path, subject: &str, mask: &Array3<bool>, header: &NiftiHeader) -> Result<()> {
    let mask_f64 = mask.mapv(|b| if b { 1.0 } else { 0.0 });
    write_preprocessed(
        out,
        subject,
        "anat",
        &format!("sub-{subject}_desc-brain_mask.nii.gz"),
        &mask_f64,
        header,
    )
}

/// The byte-identical volume writer: a 3D `f64` array in, an `f64`/datatype
/// 64 NIfTI out, no rescale — this is what makes `bidsify`'s output
/// byte-identical to the source `.mat` array.
fn write_inv_volume(vol: &Array3<f64>, header: &NiftiHeader, path: &Path) -> Result<()> {
    io::nifti::write_map_nifti(vol, header, path)
}

/// Create `dataset_description.json` if it doesn't already exist.
fn write_dataset_description(out: &Path) -> Result<()> {
    let path = out.join("dataset_description.json");
    if !path.exists() {
        std::fs::write(
            &path,
            r#"{"Name":"qmrust BIDS example","BIDSVersion":"1.8.0"}"#,
        )?;
    }
    Ok(())
}

/// Resolve (creating as needed) a datatype directory under the `preprocessed`
/// derivatives pipeline: `derivatives/preprocessed/sub-<subject>/<datatype>/`.
/// Ensures the pipeline's `dataset_description.json` exists first.
fn preprocessed_datatype_dir(out: &Path, subject: &str, datatype: &str) -> Result<PathBuf> {
    let pipeline = out.join("derivatives").join("preprocessed");
    write_derivative_dataset_description(&pipeline, "preprocessed")?;
    let dir = pipeline.join(format!("sub-{subject}")).join(datatype);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Write `vol` as `file` into datatype `datatype` of the `preprocessed`
/// derivatives pipeline (`derivatives/preprocessed/sub-<subject>/<datatype>/`).
/// Used for computed inputs — R1 maps, brain masks — which are derivatives of
/// prior processing, not raw acquisitions.
fn write_preprocessed(
    out: &Path,
    subject: &str,
    datatype: &str,
    file: &str,
    vol: &Array3<f64>,
    header: &NiftiHeader,
) -> Result<()> {
    let dir = preprocessed_datatype_dir(out, subject, datatype)?;
    write_inv_volume(vol, header, &dir.join(file))?;
    Ok(())
}

/// Create a derivatives pipeline's `dataset_description.json` (marking it a
/// standalone BIDS derivative dataset) if it doesn't already exist.
fn write_derivative_dataset_description(pipeline: &Path, name: &str) -> Result<()> {
    std::fs::create_dir_all(pipeline)?;
    let path = pipeline.join("dataset_description.json");
    if !path.exists() {
        let dd = serde_json::json!({
            "Name": name,
            "BIDSVersion": "1.8.0",
            "DatasetType": "derivative",
            "GeneratedBy": [{ "Name": "qmrust bidsify" }],
        });
        std::fs::write(&path, serde_json::to_string_pretty(&dd)?)?;
    }
    Ok(())
}

/// Ensure the dataset root's `.bidsignore` contains a `*<suffix>*` line when
/// `suffix` is not a canonical BIDS suffix (a registered model's own
/// suffix is "custom" by construction — see `rust_bids::Vocabulary`).
/// Canonical suffixes need no entry. Creates the file if missing; never
/// duplicates the line.
fn write_bidsignore_if_custom(out: &Path, suffix: &str) -> Result<()> {
    if !rust_bids::Vocabulary::bids().is_custom_suffix(suffix) {
        return Ok(());
    }
    let path = out.join(".bidsignore");
    let line = format!("*{suffix}*");
    if !path.exists() {
        std::fs::write(&path, format!("{line}\n"))?;
        return Ok(());
    }
    let mut contents = std::fs::read_to_string(&path)?;
    if contents.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if !contents.is_empty() && !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&line);
    contents.push('\n');
    std::fs::write(&path, contents)?;
    Ok(())
}

/// Create `participants.tsv` (with header) if missing, and append a
/// `sub-<subject>` row if that participant isn't already present. Presence is
/// keyed on the first (`participant_id`) column, so a row stays deduplicated
/// even after extra columns (e.g. `description`) are added to the table.
fn write_participants_row(out: &Path, subject: &str) -> Result<()> {
    let path = out.join("participants.tsv");
    let row = format!("sub-{subject}");
    if !path.exists() {
        std::fs::write(&path, format!("participant_id\n{row}\n"))?;
        return Ok(());
    }
    let mut contents = std::fs::read_to_string(&path)?;
    if contents
        .lines()
        .any(|l| l.split('\t').next().map(str::trim) == Some(row.as_str()))
    {
        return Ok(());
    }
    if !contents.ends_with('\n') {
        contents.push('\n');
    }
    contents.push_str(&row);
    contents.push('\n');
    std::fs::write(&path, contents)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("qmrust-bidsify-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Resolve `name` via the registry's `describe` (structural, no protocol
    /// needed) from an in-memory YAML config — the same seam `run_bidsify`
    /// itself uses, so these tests exercise the generic path exactly.
    fn model_from_yaml(name: &str, yaml: &str) -> Box<dyn Model> {
        let v: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let entry = qmrust_core::registry::by_name(name).unwrap();
        (entry.describe)(&v).unwrap()
    }

    fn ir_model(ti: &[f64], tr: Option<f64>) -> Box<dyn Model> {
        let mut yaml = format!(
            "model: inversion_recovery\nmethod: magnitude\ninversion_times: {:?}\n",
            ti
        );
        if let Some(tr) = tr {
            yaml.push_str(&format!("repetition_time: {tr}\n"));
        }
        model_from_yaml("inversion_recovery", &yaml)
    }

    fn qmt_model(mtdata: &[[f64; 2]], tr: f64, tmt: f64) -> Box<dyn Model> {
        let rows: Vec<String> = mtdata
            .iter()
            .map(|r| format!("[{}, {}]", r[0], r[1]))
            .collect();
        let yaml = format!(
            "model: qmt_spgr\nqmt_spgr:\n  protocol:\n    mtdata: [{}]\n    timing:\n      TR: {tr}\n      tmt: {tmt}\n",
            rows.join(", ")
        );
        model_from_yaml("qmt_spgr", &yaml)
    }

    /// Byte-identical round-trip. Every voxel written by the
    /// volume-writer must read back exactly equal to the source array — no
    /// rescale, no precision loss.
    #[test]
    fn write_inv_volume_is_byte_identical() {
        let dir = tmp_dir("roundtrip");
        let header = make_minimal_header(2, 2, 1);

        // Distinct values per voxel, including ones that would expose any
        // rescale/quantization (irrational-looking f64s).
        let vol = Array3::from_shape_vec((2, 2, 1), vec![0.0, 1.234_567_89, -42.5, 1e10]).unwrap();
        let path = dir.join("vol.nii.gz");
        write_inv_volume(&vol, &header, &path).unwrap();

        let read_back = io::nifti::read_map_nifti(&path).unwrap();
        for ((i, j, k), &expected) in vol.indexed_iter() {
            assert_eq!(
                read_back[[i, j, k]],
                expected,
                "voxel ({i},{j},{k}) mismatch"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Non-square byte-identical round-trip: a 2x2x1 fixture can't
    /// distinguish a value-transpose from an nx<->ny shape-swap. A 3x2x1
    /// fixture pins both: read-back shape must stay (3,2,1) (not (2,3,1))
    /// AND every voxel must match by (i,j,k), guarding `io::nifti`'s 2D->3D
    /// reshape against an nx<->ny swap.
    #[test]
    fn write_inv_volume_is_byte_identical_non_square() {
        let dir = tmp_dir("roundtrip-nonsquare");
        let header = make_minimal_header(3, 2, 1);

        let vol = Array3::from_shape_fn((3, 2, 1), |(i, j, _k)| (i * 10 + j) as f64 + 0.25);
        let path = dir.join("vol.nii.gz");
        write_inv_volume(&vol, &header, &path).unwrap();

        let read_back = io::nifti::read_map_nifti(&path).unwrap();
        assert_eq!(
            read_back.dim(),
            (3, 2, 1),
            "read-back shape must match the source (nx, ny, nz), not a swapped (ny, nx, nz)"
        );
        for ((i, j, k), &expected) in vol.indexed_iter() {
            assert_eq!(
                read_back[[i, j, k]],
                expected,
                "voxel ({i},{j},{k}) mismatch"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Structure test — the IRT1 tree is produced from an in-memory Array4 +
    /// a registry-resolved IR model (no .mat writer exists). The fixture uses
    /// BIDS-native seconds, matching what `run_bidsify` feeds the writer —
    /// so the sidecar this test asserts on reads as real InversionTime/
    /// RepetitionTime values in seconds (0.35 s / 2.5 s TR), not ms.
    #[test]
    fn bidsify_ir_writes_expected_tree() {
        let dir = tmp_dir("structure");
        let ir_data = Array4::from_shape_fn((2, 2, 1, 3), |(i, j, _k, t)| {
            (i * 10 + j) as f64 + t as f64 * 0.5
        });
        let ti = vec![0.350, 0.650, 0.950];
        let mask = Array3::from_shape_vec((2, 2, 1), vec![true, false, true, true]).unwrap();
        let model = ir_model(&ti, Some(2.5));

        write_bids_tree(model.as_ref(), &ir_data, Some(&mask), &[], "01", &dir, None).unwrap();

        assert!(dir.join("dataset_description.json").exists());
        let participants = std::fs::read_to_string(dir.join("participants.tsv")).unwrap();
        assert!(participants.contains("participant_id"));
        assert!(participants.contains("sub-01"));

        let anat = dir.join("sub-01").join("anat");
        for (i, &t) in ti.iter().enumerate() {
            let base = format!("sub-01_inv-{}_IRT1", i + 1);
            let nii = anat.join(format!("{base}.nii.gz"));
            assert!(nii.exists(), "missing {:?}", nii);
            let json = std::fs::read_to_string(anat.join(format!("{base}.json"))).unwrap();
            let sidecar: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(sidecar["InversionTime"], t);
            assert_eq!(sidecar["RepetitionTime"], 2.5);

            let read_back = io::nifti::read_map_nifti(&nii).unwrap();
            let expected = ir_data.index_axis(Axis(3), i);
            for ((x, y, z), &v) in expected.indexed_iter() {
                assert_eq!(read_back[[x, y, z]], v);
            }
        }

        let mask_path = dir
            .join("derivatives")
            .join("preprocessed")
            .join("sub-01")
            .join("anat")
            .join("sub-01_desc-brain_mask.nii.gz");
        assert!(mask_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A NIfTI source's spatial header must be carried into the written volumes
    /// (the orientation-preservation contract). With `Some(source_header)`, the
    /// distinctive affine survives to the output; the `None` path (a synthesized
    /// minimal header) is covered by the tests above.
    #[test]
    fn bidsify_preserves_source_header_affine() {
        let dir = tmp_dir("source-header");
        let ir_data = Array4::from_shape_fn((2, 2, 1, 3), |(i, j, _k, t)| {
            (i * 10 + j) as f64 + t as f64 * 0.5
        });
        let model = ir_model(&[0.350, 0.650, 0.950], None);

        // Distinctive affine, unlike anything make_minimal_header would produce.
        let mut src = make_minimal_header(2, 2, 1);
        src.srow_x = [2.5, 0.0, 0.0, -17.5];
        src.srow_y = [0.0, 3.0, 0.0, -21.0];
        src.srow_z = [0.0, 0.0, 4.0, 8.0];
        src.pixdim = [1.0, 2.5, 3.0, 4.0, 0.0, 0.0, 0.0, 0.0];
        src.sform_code = 2;

        write_bids_tree(model.as_ref(), &ir_data, None, &[], "01", &dir, Some(&src)).unwrap();

        let nii = dir
            .join("sub-01")
            .join("anat")
            .join("sub-01_inv-1_IRT1.nii.gz");
        let (_data, out) = io::nifti::read_map_nifti_with_header(&nii).unwrap();
        assert_eq!(out.srow_x, src.srow_x);
        assert_eq!(out.srow_y, src.srow_y);
        assert_eq!(out.srow_z, src.srow_z);
        assert_eq!(out.pixdim[1], src.pixdim[1]);
        assert_eq!(out.pixdim[2], src.pixdim[2]);
        assert_eq!(out.sform_code, src.sform_code);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Re-running bidsify for the same subject must not duplicate the
    /// participants.tsv row.
    #[test]
    fn bidsify_ir_participants_row_not_duplicated() {
        let dir = tmp_dir("dedup");
        let ir_data = Array4::from_shape_fn((1, 1, 1, 3), |(_, _, _, t)| t as f64);
        let ti = vec![0.350, 0.650, 0.950];
        let model = ir_model(&ti, None);

        write_bids_tree(model.as_ref(), &ir_data, None, &[], "01", &dir, None).unwrap();
        write_bids_tree(model.as_ref(), &ir_data, None, &[], "01", &dir, None).unwrap();

        let participants = std::fs::read_to_string(dir.join("participants.tsv")).unwrap();
        assert_eq!(participants.matches("sub-01").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A participant already listed with extra columns (e.g. a `description`)
    /// must not be re-appended: dedup keys on the `participant_id` column, not
    /// the whole row.
    #[test]
    fn bidsify_participants_row_deduped_with_extra_columns() {
        let dir = tmp_dir("dedup-extra-cols");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("participants.tsv"),
            "participant_id\tdescription\nsub-01\tInversion Recovery T1 mapping (IRT1)\n",
        )
        .unwrap();

        let ir_data = Array4::from_shape_fn((1, 1, 1, 3), |(_, _, _, t)| t as f64);
        let ti = vec![0.350, 0.650, 0.950];
        let model = ir_model(&ti, None);
        write_bids_tree(model.as_ref(), &ir_data, None, &[], "01", &dir, None).unwrap();

        let participants = std::fs::read_to_string(dir.join("participants.tsv")).unwrap();
        assert_eq!(participants.matches("sub-01").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// When the config supplies no TR, RepetitionTime must be omitted from
    /// the sidecar rather than serialized as `null`.
    #[test]
    fn bidsify_ir_omits_repetition_time_when_absent() {
        let dir = tmp_dir("no-tr");
        let ir_data = Array4::from_shape_fn((1, 1, 1, 3), |(_, _, _, t)| t as f64);
        let ti = vec![0.350, 0.650, 0.950];
        let model = ir_model(&ti, None);

        write_bids_tree(model.as_ref(), &ir_data, None, &[], "01", &dir, None).unwrap();

        let anat = dir.join("sub-01").join("anat");
        let json = std::fs::read_to_string(anat.join("sub-01_inv-1_IRT1.json")).unwrap();
        let sidecar: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(sidecar.get("RepetitionTime").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Byte-identical + structure test for QMTSPGR. Uses the same
    /// `write_inv_volume` writer as IR (already proven byte-identical above,
    /// including the non-square/shape-swap guard), so this test's job is to
    /// pin the QMTSPGR-specific pieces: flip/mt filename derivation, the
    /// Angle/Offset/RepetitionTime sidecar, and the `.bidsignore` line —
    /// while still asserting every voxel round-trips exactly.
    ///
    /// The fixture's row order mirrors the qMRLab qMT protocol:
    /// [142,443],[426,443],[142,1088],[426,1088] = flip-1_mt-1, flip-2_mt-1,
    /// flip-1_mt-2, flip-2_mt-2 (Angle varies fastest -> flip index; Offset
    /// next -> mt index).
    #[test]
    fn bidsify_qmt_writes_expected_tree() {
        let dir = tmp_dir("qmt-structure");
        let mt_data = Array4::from_shape_fn((2, 2, 1, 4), |(i, j, _k, t)| {
            (i * 10 + j) as f64 + t as f64 * 0.5
        });
        let protocol: Vec<[f64; 2]> = vec![
            [142.0, 443.0],
            [426.0, 443.0],
            [142.0, 1088.0],
            [426.0, 1088.0],
        ];
        let tr = 0.025;
        let tmt = 0.0102;
        let model = qmt_model(&protocol, tr, tmt);

        write_bids_tree(model.as_ref(), &mt_data, None, &[], "02", &dir, None).unwrap();

        assert!(dir.join("dataset_description.json").exists());
        let participants = std::fs::read_to_string(dir.join("participants.tsv")).unwrap();
        assert!(participants.contains("sub-02"));
        let bidsignore = std::fs::read_to_string(dir.join(".bidsignore")).unwrap();
        assert!(bidsignore.contains("*QMTSPGR*"));

        let anat = dir.join("sub-02").join("anat");
        let expected = [
            ("sub-02_flip-1_mt-1_QMTSPGR", 142.0, 443.0),
            ("sub-02_flip-2_mt-1_QMTSPGR", 426.0, 443.0),
            ("sub-02_flip-1_mt-2_QMTSPGR", 142.0, 1088.0),
            ("sub-02_flip-2_mt-2_QMTSPGR", 426.0, 1088.0),
        ];
        for (i, (base, angle, offset)) in expected.iter().enumerate() {
            let nii = anat.join(format!("{base}.nii.gz"));
            assert!(nii.exists(), "missing {:?}", nii);
            let json = std::fs::read_to_string(anat.join(format!("{base}.json"))).unwrap();
            let sidecar: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert_eq!(sidecar["Angle"], *angle);
            assert_eq!(sidecar["Offset"], *offset);
            assert_eq!(sidecar["RepetitionTime"], tr);
            assert_eq!(sidecar["MTPulseDuration"], tmt);

            let read_back = io::nifti::read_map_nifti(&nii).unwrap();
            let expected_vol = mt_data.index_axis(Axis(3), i);
            for ((x, y, z), &v) in expected_vol.indexed_iter() {
                assert_eq!(read_back[[x, y, z]], v, "voxel ({x},{y},{z}) mismatch");
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Aux maps are only written when supplied, and each lands in its
    /// datatype within the `preprocessed` derivatives pipeline: B1/B0 field
    /// maps in `fmap/` (with a `Units` sidecar), the R1 map and brain mask in
    /// `anat/`. The raw tree holds no aux. Each present map round-trips
    /// byte-identically.
    #[test]
    fn bidsify_qmt_places_aux_by_bids_nature() {
        let dir = tmp_dir("qmt-aux");
        let mt_data = Array4::from_shape_fn((2, 1, 1, 1), |(i, _j, _k, _t)| i as f64);
        let protocol = vec![[142.0, 443.0]];
        let model = qmt_model(&protocol, 0.025, 0.0102);
        let mask = Array3::from_shape_vec((2, 1, 1), vec![true, false]).unwrap();
        let r1map = Array3::from_shape_vec((2, 1, 1), vec![1.1, 2.2]).unwrap();
        let b1map = Array3::from_shape_vec((2, 1, 1), vec![0.9, 1.05]).unwrap();
        let b0map = Array3::from_shape_vec((2, 1, 1), vec![-12.0, 7.5]).unwrap();
        let aux = vec![
            ("R1map".to_string(), r1map.clone()),
            ("B1map".to_string(), b1map.clone()),
            ("B0map".to_string(), b0map.clone()),
        ];

        write_bids_tree(
            model.as_ref(),
            &mt_data,
            Some(&mask),
            &aux,
            "02",
            &dir,
            None,
        )
        .unwrap();

        let preproc = dir.join("derivatives").join("preprocessed");
        assert!(preproc.join("dataset_description.json").exists());

        // Field maps: the preprocessed pipeline's fmap/ datatype, with a Units
        // sidecar.
        let fmap = preproc.join("sub-02").join("fmap");
        for (suffix, map, units) in [("TB1map", &b1map, "1"), ("B0map", &b0map, "Hz")] {
            let nii = fmap.join(format!("sub-02_{suffix}.nii.gz"));
            assert!(nii.exists(), "missing preprocessed fmap {suffix}");
            let sidecar: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(fmap.join(format!("sub-02_{suffix}.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(sidecar["Units"], units);
            let read_back = io::nifti::read_map_nifti(&nii).unwrap();
            for ((x, y, z), &v) in map.indexed_iter() {
                assert_eq!(read_back[[x, y, z]], v);
            }
        }

        // R1 map and brain mask: the preprocessed pipeline's anat/ datatype.
        let preproc_anat = preproc.join("sub-02").join("anat");
        assert!(preproc_anat.join("sub-02_desc-brain_mask.nii.gz").exists());
        let r1_path = preproc_anat.join("sub-02_R1map.nii.gz");
        assert!(r1_path.exists());
        let read_back = io::nifti::read_map_nifti(&r1_path).unwrap();
        for ((x, y, z), &v) in r1map.indexed_iter() {
            assert_eq!(read_back[[x, y, z]], v);
        }

        // The raw tree holds no aux — only the QMTSPGR acquisitions.
        assert!(!dir.join("sub-02").join("fmap").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Re-running `write_bids_tree` must not duplicate the `.bidsignore`
    /// line.
    #[test]
    fn bidsify_qmt_bidsignore_not_duplicated() {
        let dir = tmp_dir("qmt-bidsignore-dedup");
        let mt_data = Array4::from_shape_fn((1, 1, 1, 1), |_| 0.0);
        let protocol = vec![[142.0, 443.0]];
        let model = qmt_model(&protocol, 0.025, 0.0102);

        write_bids_tree(model.as_ref(), &mt_data, None, &[], "02", &dir, None).unwrap();
        write_bids_tree(model.as_ref(), &mt_data, None, &[], "02", &dir, None).unwrap();

        let bidsignore = std::fs::read_to_string(dir.join(".bidsignore")).unwrap();
        assert_eq!(bidsignore.matches("*QMTSPGR*").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
