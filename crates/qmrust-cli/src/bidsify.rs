//! `qmrust bidsify`: convert a qMRLab `.mat` dataset into a BIDS layout whose
//! voxel data is byte-identical to the source `.mat` (no rescale, no dtype
//! narrowing — every inversion volume is written as `f64`/datatype 64).
//!
//! IRT1 (`inversion_recovery`) is the only model supported so far; QMTSPGR is
//! a tracked follow-up (see the BIDS-examples plan).

use anyhow::{bail, Result};
use ndarray::{Array3, Array4, Axis};
use nifti::NiftiHeader;
use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::commands::make_minimal_header;
use crate::io;

/// IRT1 per-volume sidecar fields (BIDS-qMRI convention). `repetition_time`
/// comes from the IR config's `repetition_time` field (qMRLab's IR_demo TR
/// is 2.5 s — 2500 ms in qMRLab's ms convention, converted at the config
/// boundary); it's `None`, and skipped rather than serialized as `null`,
/// only when the config doesn't supply one. Kept as a typed struct (not a
/// hand-formatted string) so it stays correct as fields grow (this shape,
/// and later QMTSPGR's much larger sidecar).
#[derive(Serialize)]
struct IrSidecar {
    #[serde(rename = "InversionTime")]
    inversion_time: f64,
    #[serde(rename = "RepetitionTime", skip_serializing_if = "Option::is_none")]
    repetition_time: Option<f64>,
}

/// Parsed CLI arguments for `qmrust bidsify`.
pub struct BidsifyArgs {
    pub model: String,
    pub mat_data: PathBuf,
    pub mask: Option<PathBuf>,
    pub config: PathBuf,
    pub subject: String,
    pub out: PathBuf,
}

/// Entry point for the `bidsify` subcommand: reads the `.mat` dataset (+
/// optional mask + config) and writes the BIDS tree.
pub fn run_bidsify(args: BidsifyArgs) -> Result<()> {
    if args.model != "inversion_recovery" {
        bail!(
            "bidsify only supports --model inversion_recovery so far, got '{}'",
            args.model
        );
    }

    let contents = std::fs::read_to_string(&args.config)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", args.config, e))?;
    let (cfg, _raw) = qmrust_core::config::parse_config(&contents)?;

    let mat = io::mat::read_mat_file(&args.mat_data)?;

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

/// Write one inversion-recovery `.mat` dataset as a BIDS tree rooted at `out`:
/// `dataset_description.json`, `participants.tsv`, one
/// `sub-<subject>/anat/sub-<subject>_inv-<i>_IRT1.nii.gz` (+ JSON sidecar) per
/// volume in `ir_data`'s 4th axis (1-based `<i>`, matching `ti`'s order), and
/// — if a mask is given — a brain-mask derivative under
/// `derivatives/qmrust/sub-<subject>/anat/`.
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
        let deriv_anat = out
            .join("derivatives")
            .join("qmrust")
            .join(format!("sub-{subject}"))
            .join("anat");
        std::fs::create_dir_all(&deriv_anat)?;
        let mask_f64 = mask.mapv(|b| if b { 1.0 } else { 0.0 });
        let mask_path = deriv_anat.join(format!("sub-{subject}_desc-brain_mask.nii.gz"));
        write_inv_volume(&mask_f64, &header, &mask_path)?;
    }

    Ok(())
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

/// Create `participants.tsv` (with header) if missing, and append a
/// `sub-<subject>` row if it isn't already present.
fn write_participants_row(out: &Path, subject: &str) -> Result<()> {
    let path = out.join("participants.tsv");
    let row = format!("sub-{subject}");
    if !path.exists() {
        std::fs::write(&path, format!("participant_id\n{row}\n"))?;
        return Ok(());
    }
    let mut contents = std::fs::read_to_string(&path)?;
    if contents.lines().any(|l| l.trim() == row) {
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

    /// Step 1 (TDD): byte-identical round-trip. Every voxel written by the
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

    /// Step 2/3: structure test — the IRT1 tree is produced from an
    /// in-memory Array4 + TI fixture (no .mat writer exists, per the plan).
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
            .join("qmrust")
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
}
