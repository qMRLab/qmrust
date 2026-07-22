//! `qmrust bidsify`: convert a qMRLab `.mat` dataset into a BIDS layout whose
//! voxel data is byte-identical to the source `.mat` (no rescale, no dtype
//! narrowing — every volume is written as `f64`/datatype 64).
//!
//! Supports `inversion_recovery` (IRT1) and `qmt_spgr` (QMTSPGR, a
//! non-official/`.bidsignore`'d suffix).

use anyhow::{bail, Result};
use ndarray::{Array3, Array4, Axis};
use nifti::NiftiHeader;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::commands::make_minimal_header;
use crate::io;

/// IRT1 per-volume sidecar fields (BIDS-qMRI convention). `repetition_time`
/// comes from the IR config's `repetition_time` field (in seconds); it's
/// `None`, and skipped rather than serialized as `null`, only when the config
/// doesn't supply one.
#[derive(Serialize)]
struct IrSidecar {
    #[serde(rename = "InversionTime")]
    inversion_time: f64,
    #[serde(rename = "RepetitionTime", skip_serializing_if = "Option::is_none")]
    repetition_time: Option<f64>,
}

/// qMT-SPGR per-volume sidecar fields. `Angle`/`Offset` match the protocol
/// keys `qmt_spgr`'s `protocol_schema()` reads back out of BIDS sidecars, so
/// a `QMTSPGR` fit is metadata-driven and order-free. `RepetitionTime` and
/// `MTPulseDuration` come from the config's `protocol.timing` (already
/// BIDS-native seconds — qMT configs carry no ms convention to convert).
#[derive(Serialize)]
struct QmtSidecar {
    #[serde(rename = "Angle")]
    angle: f64,
    #[serde(rename = "Offset")]
    offset: f64,
    #[serde(rename = "RepetitionTime")]
    repetition_time: f64,
    #[serde(rename = "MTPulseDuration")]
    mt_pulse_duration: f64,
}

/// Parsed CLI arguments for `qmrust bidsify`.
pub struct BidsifyArgs {
    pub model: String,
    pub mat_data: Option<PathBuf>,
    pub mat_dir: Option<PathBuf>,
    pub mask: Option<PathBuf>,
    pub config: PathBuf,
    pub subject: String,
    pub out: PathBuf,
}

/// Entry point for the `bidsify` subcommand: reads the `.mat` dataset (+
/// optional mask + config) and writes the BIDS tree.
pub fn run_bidsify(args: BidsifyArgs) -> Result<()> {
    match args.model.as_str() {
        "inversion_recovery" => run_bidsify_ir(args),
        "qmt_spgr" => run_bidsify_qmt(args),
        other => bail!(
            "bidsify only supports --model inversion_recovery|qmt_spgr, got '{}'",
            other
        ),
    }
}

fn run_bidsify_ir(args: BidsifyArgs) -> Result<()> {
    let mat_data = args
        .mat_data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("--mat-data is required for --model inversion_recovery"))?;

    let contents = std::fs::read_to_string(&args.config)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", args.config, e))?;
    let (cfg, _raw) = qmrust_core::config::parse_config(&contents)?;

    let mat = io::mat::read_mat_file(mat_data)?;

    // The .mat's own TI vector (if present) is authoritative and, crucially,
    // in the same order as the data's 4th axis — never re-sort it. Falling
    // back to the config's `inversion_times` assumes that list is already in
    // the data's volume order (true for qMRLab's IR_demo protocol configs).
    //
    // qMRLab `.mat` TI vectors are in milliseconds, while the parsed config
    // (and thus core) is BIDS-native seconds — convert ms -> s here, at the
    // shell boundary, so the sidecar and the config fallback always agree on
    // units (see CLAUDE.md "Units — BIDS-native").
    let ti = crate::commands::mat_ti_to_seconds(mat.ti.clone())
        .unwrap_or_else(|| cfg.inversion_times.clone());
    if ti.is_empty() {
        bail!("no inversion times: absent from both the .mat file and the config");
    }

    // A separately-supplied --mask file takes precedence over one embedded in
    // the IR .mat (matches `qmrust fit`'s existing --mask-overrides-embedded
    // convention).
    let mask = match &args.mask {
        Some(p) => Some(io::mat::read_mask_mat(p)?),
        None => mat.mask,
    };

    // IR config fields are top-level in the YAML, so `_raw` (the raw parsed
    // tree) can be re-parsed directly into `IrConfig` to pick up TR.
    let ir_cfg: qmrust_core::models::inversion_recovery::config::IrConfig =
        serde_yaml::from_value(_raw.clone())?;
    let tr = ir_cfg.repetition_time;

    bidsify_ir(
        &mat.ir_data,
        &ti,
        mask.as_ref(),
        &args.subject,
        &args.out,
        tr,
    )
}

fn run_bidsify_qmt(args: BidsifyArgs) -> Result<()> {
    // --mat-data wins; otherwise fall back to <mat-dir>/MTdata.mat (mirrors
    // `qmrust fit --mat-dir`'s convenience-path convention).
    let mat_data_path = args
        .mat_data
        .clone()
        .or_else(|| args.mat_dir.as_ref().map(|d| d.join("MTdata.mat")))
        .ok_or_else(|| anyhow::anyhow!("--model qmt_spgr requires --mat-data or --mat-dir"))?;

    let contents = std::fs::read_to_string(&args.config)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", args.config, e))?;
    let (_cfg, raw) = qmrust_core::config::parse_config(&contents)?;
    let mut q: qmrust_core::models::qmt_spgr::config::QmtSpgrConfig = match raw.get("qmt_spgr") {
        Some(sub) => serde_yaml::from_value(sub.clone())?,
        None => Default::default(),
    };
    q.validate_options()?;
    q.validate_protocol()?;

    let mat = io::mat::read_mat_file(&mat_data_path)?;

    // --mask, then a Mask.mat found via --mat-dir, then one embedded in the
    // MTdata .mat itself, in that precedence order (same as the IR path).
    let mat_dir_mask = args
        .mat_dir
        .as_ref()
        .map(|d| d.join("Mask.mat"))
        .filter(|p| p.exists());
    let mask = match args.mask.as_ref().or(mat_dir_mask.as_ref()) {
        Some(p) => Some(io::mat::read_mask_mat(p)?),
        None => mat.mask,
    };

    // Aux maps are only available via --mat-dir (OSF's qMT dataset ships them
    // as separate single-variable .mat files alongside MTdata.mat).
    let load_aux = |name: &str| -> Result<Option<Array3<f64>>> {
        match args
            .mat_dir
            .as_ref()
            .map(|d| d.join(name))
            .filter(|p| p.exists())
        {
            Some(p) => Ok(Some(io::mat::read_map_mat(&p)?)),
            None => Ok(None),
        }
    };
    let r1map = load_aux("R1map.mat")?;
    let b1map = load_aux("B1map.mat")?;
    let b0map = load_aux("B0map.mat")?;

    bidsify_qmt(
        &mat.ir_data,
        &q.protocol.mtdata,
        q.protocol.timing.trep,
        q.protocol.timing.tmt,
        mask.as_ref(),
        r1map.as_ref(),
        b1map.as_ref(),
        b0map.as_ref(),
        &args.subject,
        &args.out,
    )
}

/// Write one inversion-recovery `.mat` dataset as a BIDS tree rooted at `out`:
/// `dataset_description.json`, `participants.tsv`, one
/// `sub-<subject>/anat/sub-<subject>_inv-<i>_IRT1.nii.gz` (+ JSON sidecar) per
/// volume in `ir_data`'s 4th axis (1-based `<i>`, matching `ti`'s order), and
/// — if a mask is given — a brain-mask derivative in the `preprocessed`
/// pipeline (`derivatives/preprocessed/sub-<subject>/anat/`).
pub fn bidsify_ir(
    ir_data: &Array4<f64>,
    ti: &[f64],
    mask: Option<&Array3<bool>>,
    subject: &str,
    out: &Path,
    tr: Option<f64>,
) -> Result<()> {
    let (nx, ny, nz, n_ti) = ir_data.dim();
    if ti.len() != n_ti {
        bail!(
            "IR data has {} volumes but {} inversion times were supplied",
            n_ti,
            ti.len()
        );
    }

    std::fs::create_dir_all(out)?;
    write_dataset_description(out)?;
    write_participants_row(out, subject)?;

    let anat_dir = out.join(format!("sub-{subject}")).join("anat");
    std::fs::create_dir_all(&anat_dir)?;
    // .mat inputs carry no spatial header — emit qMRLab's make_nii-compatible
    // minimal header (matches how `qmrust fit --mat-data` treats .mat input).
    let header = make_minimal_header(nx, ny, nz);

    for (i, &inversion_time) in ti.iter().enumerate() {
        let vol = ir_data.index_axis(Axis(3), i).to_owned();
        let base = format!("sub-{subject}_inv-{}_IRT1", i + 1);
        let nii_path = anat_dir.join(format!("{base}.nii.gz"));
        write_inv_volume(&vol, &header, &nii_path)?;

        let json_path = anat_dir.join(format!("{base}.json"));
        // RepetitionTime comes from the config (qMRLab's IR_demo TR is 2.5 s
        // — 2500 ms in qMRLab's ms convention, converted at the config
        // boundary). It's recorded here per BIDS convention even though the
        // RD-NLS fitter itself doesn't consume TR; it's omitted (not
        // serialized as `null`) if the config doesn't supply one.
        let sidecar = IrSidecar {
            inversion_time,
            repetition_time: tr,
        };
        std::fs::write(&json_path, serde_json::to_string_pretty(&sidecar)?)?;
    }

    if let Some(mask) = mask {
        let mask_f64 = mask.mapv(|b| if b { 1.0 } else { 0.0 });
        write_preprocessed(
            out,
            subject,
            "anat",
            &format!("sub-{subject}_desc-brain_mask.nii.gz"),
            &mask_f64,
            &header,
        )?;
    }

    Ok(())
}

/// Write one qMT-SPGR `.mat` dataset as a BIDS tree rooted at `out`: one
/// `sub-<subject>/anat/sub-<subject>_flip-<f>_mt-<m>_QMTSPGR.nii.gz` (+ JSON
/// sidecar) per volume in `mt_data`'s 4th axis, plus `dataset_description.json`,
/// `participants.tsv`, and a root `.bidsignore` (QMTSPGR is a custom,
/// non-official suffix). The raw tree holds only the QMTSPGR acquisitions;
/// every supplied auxiliary map is a computed input and lands in the
/// `preprocessed` derivatives pipeline under its datatype — B1/B0 field maps
/// in `derivatives/preprocessed/sub-<subject>/fmap/` (`TB1map`, `B0map`), the
/// R1 map and brain mask in `derivatives/preprocessed/sub-<subject>/anat/`.
///
/// `protocol[i]` is `[Angle (deg), Offset (Hz)]` for volume `i`, in the same
/// order as `mt_data`'s 4th axis (never re-sorted — qMRLab's `.mat` volume
/// order is authoritative, matching `bidsify_ir`'s TI-order convention).
/// `flip-<f>` indexes the unique Angles in first-seen order; `mt-<m>` indexes
/// the unique Offsets in first-seen order (both 1-based) — this reproduces
/// qMRLab's `qmt_spgr_batch` file-naming convention. Correctness of any
/// downstream fit does not depend on this ordering: `qmt_spgr` reads
/// `Angle`/`Offset` back out of each sidecar via `protocol_schema()`.
#[allow(clippy::too_many_arguments)]
pub fn bidsify_qmt(
    mt_data: &Array4<f64>,
    protocol: &[[f64; 2]],
    repetition_time: f64,
    mt_pulse_duration: f64,
    mask: Option<&Array3<bool>>,
    r1map: Option<&Array3<f64>>,
    b1map: Option<&Array3<f64>>,
    b0map: Option<&Array3<f64>>,
    subject: &str,
    out: &Path,
) -> Result<()> {
    let (nx, ny, nz, n_vol) = mt_data.dim();
    if protocol.len() != n_vol {
        bail!(
            "MT data has {} volumes but the protocol has {} rows",
            n_vol,
            protocol.len()
        );
    }

    std::fs::create_dir_all(out)?;
    write_dataset_description(out)?;
    write_participants_row(out, subject)?;
    write_bidsignore(out)?;

    let anat_dir = out.join(format!("sub-{subject}")).join("anat");
    std::fs::create_dir_all(&anat_dir)?;
    let header = make_minimal_header(nx, ny, nz);

    // First-seen-order indices, 1-based, matching qMRLab's flip/mt file
    // naming (see the doc comment above).
    let mut angles_seen: Vec<f64> = Vec::new();
    let mut offsets_seen: Vec<f64> = Vec::new();

    for (i, row) in protocol.iter().enumerate() {
        let [angle, offset] = *row;
        let flip_idx = first_seen_index(&mut angles_seen, angle);
        let mt_idx = first_seen_index(&mut offsets_seen, offset);

        let vol = mt_data.index_axis(Axis(3), i).to_owned();
        let base = format!("sub-{subject}_flip-{flip_idx}_mt-{mt_idx}_QMTSPGR");
        let nii_path = anat_dir.join(format!("{base}.nii.gz"));
        write_inv_volume(&vol, &header, &nii_path)?;

        let json_path = anat_dir.join(format!("{base}.json"));
        let sidecar = QmtSidecar {
            angle,
            offset,
            repetition_time,
            mt_pulse_duration,
        };
        std::fs::write(&json_path, serde_json::to_string_pretty(&sidecar)?)?;
    }

    if let Some(b1) = b1map {
        write_fmap(out, subject, "TB1map", "1", b1, &header)?;
    }
    if let Some(b0) = b0map {
        write_fmap(out, subject, "B0map", "Hz", b0, &header)?;
    }
    if let Some(r1) = r1map {
        write_preprocessed(
            out,
            subject,
            "anat",
            &format!("sub-{subject}_R1map.nii.gz"),
            r1,
            &header,
        )?;
    }
    if let Some(mask) = mask {
        let mask_f64 = mask.mapv(|b| if b { 1.0 } else { 0.0 });
        write_preprocessed(
            out,
            subject,
            "anat",
            &format!("sub-{subject}_desc-brain_mask.nii.gz"),
            &mask_f64,
            &header,
        )?;
    }

    Ok(())
}

/// Return `val`'s 1-based index in first-seen order within `seen`, appending
/// it if not already present. Used to derive qMRLab's `flip-<f>`/`mt-<m>`
/// BIDS entities from the protocol's raw Angle/Offset columns.
fn first_seen_index(seen: &mut Vec<f64>, val: f64) -> usize {
    match seen.iter().position(|&v| v == val) {
        Some(idx) => idx + 1,
        None => {
            seen.push(val);
            seen.len()
        }
    }
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

/// Write an estimated field map into the `preprocessed` pipeline's `fmap/`
/// datatype with a minimal sidecar. `suffix` is the BIDS field-map suffix
/// (`TB1map` for a transmit B1+ map, `B0map` for a B0 map); `units` is that
/// map's unit (`"1"` for a dimensionless B1 ratio, `"Hz"` for B0). These maps
/// are computed field estimates, not raw acquisitions, so they are derivatives.
fn write_fmap(
    out: &Path,
    subject: &str,
    suffix: &str,
    units: &str,
    vol: &Array3<f64>,
    header: &NiftiHeader,
) -> Result<()> {
    let fmap_dir = preprocessed_datatype_dir(out, subject, "fmap")?;
    let base = format!("sub-{subject}_{suffix}");
    write_inv_volume(vol, header, &fmap_dir.join(format!("{base}.nii.gz")))?;
    let sidecar = serde_json::json!({ "Units": units });
    std::fs::write(
        fmap_dir.join(format!("{base}.json")),
        serde_json::to_string_pretty(&sidecar)?,
    )?;
    Ok(())
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

/// Ensure the dataset root's `.bidsignore` contains a `*QMTSPGR*` line:
/// `QMTSPGR` is a non-official BIDS suffix, so the raw dataset ignores it for
/// generic validators (rust-bids's own discovery exempts registered model
/// suffixes regardless of `.bidsignore`). Creates the file if missing; never
/// duplicates the line.
fn write_bidsignore(out: &Path) -> Result<()> {
    let path = out.join(".bidsignore");
    let line = "*QMTSPGR*";
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
    contents.push_str(line);
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
    /// AND every voxel must match by (i,j,k), guarding the fix in
    /// `io::nifti`'s 2D->3D reshape (see the commit that fixed it).
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

    /// Structure test — the IRT1 tree is produced from an
    /// in-memory Array4 + TI fixture (no .mat writer exists).
    /// `bidsify_ir` itself is unit-agnostic (it writes whatever `ti`/`tr` it's
    /// given verbatim); the fixture uses BIDS-native seconds — matching what
    /// `run_bidsify` now feeds it after converting a `.mat`'s ms TI vector —
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

        bidsify_ir(&ir_data, &ti, Some(&mask), "01", &dir, Some(2.5)).unwrap();

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

    /// Re-running bidsify for the same subject must not duplicate the
    /// participants.tsv row.
    #[test]
    fn bidsify_ir_participants_row_not_duplicated() {
        let dir = tmp_dir("dedup");
        let ir_data = Array4::from_shape_fn((1, 1, 1, 3), |(_, _, _, t)| t as f64);
        let ti = vec![0.350, 0.650, 0.950];

        bidsify_ir(&ir_data, &ti, None, "01", &dir, None).unwrap();
        bidsify_ir(&ir_data, &ti, None, "01", &dir, None).unwrap();

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
        bidsify_ir(&ir_data, &ti, None, "01", &dir, None).unwrap();

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

        bidsify_ir(&ir_data, &ti, None, "01", &dir, None).unwrap();

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
    /// The fixture's row order mirrors the real qMRLab qMT protocol quoted in
    /// the task brief: [142,443],[426,443],[142,1088],[426,1088] = flip-1_mt-1,
    /// flip-2_mt-1, flip-1_mt-2, flip-2_mt-2 (Angle varies fastest -> flip
    /// index; Offset next -> mt index).
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

        bidsify_qmt(
            &mt_data, &protocol, tr, tmt, None, None, None, None, "02", &dir,
        )
        .unwrap();

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
        let mask = Array3::from_shape_vec((2, 1, 1), vec![true, false]).unwrap();
        let r1map = Array3::from_shape_vec((2, 1, 1), vec![1.1, 2.2]).unwrap();
        let b1map = Array3::from_shape_vec((2, 1, 1), vec![0.9, 1.05]).unwrap();
        let b0map = Array3::from_shape_vec((2, 1, 1), vec![-12.0, 7.5]).unwrap();

        bidsify_qmt(
            &mt_data,
            &protocol,
            0.025,
            0.0102,
            Some(&mask),
            Some(&r1map),
            Some(&b1map),
            Some(&b0map),
            "02",
            &dir,
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

    /// Re-running `bidsify_qmt` must not duplicate the `.bidsignore` line.
    #[test]
    fn bidsify_qmt_bidsignore_not_duplicated() {
        let dir = tmp_dir("qmt-bidsignore-dedup");
        let mt_data = Array4::from_shape_fn((1, 1, 1, 1), |_| 0.0);
        let protocol = vec![[142.0, 443.0]];

        bidsify_qmt(
            &mt_data, &protocol, 0.025, 0.0102, None, None, None, None, "02", &dir,
        )
        .unwrap();
        bidsify_qmt(
            &mt_data, &protocol, 0.025, 0.0102, None, None, None, None, "02", &dir,
        )
        .unwrap();

        let bidsignore = std::fs::read_to_string(dir.join(".bidsignore")).unwrap();
        assert_eq!(bidsignore.matches("*QMTSPGR*").count(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
