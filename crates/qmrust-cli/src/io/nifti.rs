//! NIfTI I/O helpers for reading 4D IR data / 3D masks and writing 3D output maps.

use anyhow::{bail, Context, Result};
use ndarray::{Array3, Array4};
use nifti::writer::WriterOptions;
use nifti::{IntoNdArray, NiftiHeader, NiftiObject, ReaderOptions};
use std::path::{Path, PathBuf};

/// Read a NIfTI file and return the raw dynamic-dimension array + header.
fn read_nifti_raw(path: &Path) -> Result<(ndarray::ArrayD<f64>, NiftiHeader)> {
    let obj = ReaderOptions::new()
        .read_file(path)
        .with_context(|| format!("Failed to read NIfTI file {:?}", path))?;
    let header = obj.header().clone();
    let volume = obj.into_volume();
    let data = volume
        .into_ndarray::<f64>()
        .with_context(|| format!("Failed to convert NIfTI volume to ndarray from {:?}", path))?;
    Ok((data, header))
}

/// Read a measurement NIfTI as `(x, y, z, volume)`, given how many volumes the
/// model expects (`Model::n_volumes()`).
///
/// A 4D file is unambiguous and is used as-is. A 3D file is not: NIfTI's
/// `dim[0] = 3` declares three *spatial* axes, but the convention this tool
/// grew up with also stores a single-slice series that way, with the series
/// axis third. The file alone cannot say which it is, so the model decides —
/// it is the only party that knows whether it fits one volume or many:
///
/// - expecting one volume  → `(x, y, z, 1)`, a genuine 3D acquisition;
/// - expecting `n` volumes → `(x, y, 1, n)`, a single slice sampled `n` times,
///   and the third extent must actually be `n`.
pub fn read_4d_nifti(path: &Path, expected_volumes: usize) -> Result<(Array4<f64>, NiftiHeader)> {
    let (data, header) = read_nifti_raw(path)?;
    let shape = data.shape().to_vec();
    match shape.len() {
        4 => {
            let arr = data
                .into_dimensionality::<ndarray::Ix4>()
                .map_err(|e| anyhow::anyhow!("Failed to reshape to 4D: {}", e))?;
            Ok((arr, header))
        }
        3 => {
            // `into_ndarray` returns Fortran-ordered memory (see the `nifti`
            // crate's doc note); pulling the raw buffer and re-wrapping it
            // with `from_shape_vec` (which assumes C order) would silently
            // transpose non-square data. `insert_axis` adds the singleton
            // dimension by logical index instead, so it's layout-agnostic
            // wherever it goes.
            let axis = if expected_volumes == 1 {
                // Three spatial axes and one volume: append the volume axis.
                3
            } else {
                // Single-slice series: the third extent is the series, so the
                // singleton z goes before it.
                if shape[2] != expected_volumes {
                    bail!(
                        "{:?} is a 3D NIfTI of {}x{}x{}, but the model expects {} volumes. \
                         Read as a single-slice series its third dimension would have to be \
                         {}; read as one 3D volume the model would have to expect 1. Store a \
                         single-slice series as 4D ({}x{}x1x{}) to say which is meant.",
                        path,
                        shape[0],
                        shape[1],
                        shape[2],
                        expected_volumes,
                        expected_volumes,
                        shape[0],
                        shape[1],
                        expected_volumes
                    );
                }
                2
            };
            let arr = data
                .insert_axis(ndarray::Axis(axis))
                .into_dimensionality::<ndarray::Ix4>()
                .map_err(|e| anyhow::anyhow!("Failed to reshape 3D NIfTI to 4D: {}", e))?;
            Ok((arr, header))
        }
        _ => bail!(
            "Expected 3D or 4D NIfTI file, got {}D from {:?}",
            shape.len(),
            path
        ),
    }
}

/// Read a 3D NIfTI mask file. Voxels > 0 are true.
pub fn read_mask_nifti(path: &Path) -> Result<Array3<bool>> {
    let (data, _header) = read_nifti_raw(path)?;
    let shape = data.shape();
    match shape.len() {
        3 => {
            let arr = data
                .into_dimensionality::<ndarray::Ix3>()
                .map_err(|e| anyhow::anyhow!("Failed to reshape mask to 3D: {}", e))?;
            Ok(arr.mapv(|v| v > 0.0))
        }
        2 => {
            // 2D mask → (x, y, 1). See the 3D branch of `read_4d_nifti` for
            // why `insert_axis` (not a raw-buffer `from_shape_vec`) is required.
            let arr = data
                .insert_axis(ndarray::Axis(2))
                .into_dimensionality::<ndarray::Ix3>()
                .map_err(|e| anyhow::anyhow!("Failed to reshape 2D mask to 3D: {}", e))?;
            Ok(arr.mapv(|v| v > 0.0))
        }
        _ => bail!(
            "Expected 2D or 3D mask NIfTI, got {}D from {:?}",
            shape.len(),
            path
        ),
    }
}

/// Read a 2D/3D NIfTI scalar map as a 3D f64 array (z=1 for 2D).
pub fn read_map_nifti(path: &Path) -> Result<Array3<f64>> {
    let (arr, _header) = read_map_nifti_with_header(path)?;
    Ok(arr)
}

/// Read a 2D/3D NIfTI scalar map as a 3D f64 array (z=1 for 2D), also
/// returning the header — used when a single volume's spatial geometry must
/// be threaded through (e.g. one timepoint of a BIDS `Sequential` series).
pub fn read_map_nifti_with_header(path: &Path) -> Result<(Array3<f64>, NiftiHeader)> {
    let (data, header) = read_nifti_raw(path)?;
    let shape = data.shape();
    let arr = match shape.len() {
        3 => data
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| anyhow::anyhow!("Failed to reshape map to 3D: {}", e))?,
        2 => data
            .insert_axis(ndarray::Axis(2))
            .into_dimensionality::<ndarray::Ix3>()
            .map_err(|e| anyhow::anyhow!("Failed to reshape 2D map to 3D: {}", e))?,
        _ => bail!(
            "Expected 2D or 3D NIfTI map, got {}D from {:?}",
            shape.len(),
            path
        ),
    };
    Ok((arr, header))
}

/// Read a `Named` model's role volumes from a directory: one `<role>.nii.gz`
/// per role (each a 3D scalar map), stacked into a 4D array in the given role
/// order — column `i` is `roles[i]`. The BIDS-input counterpart to
/// [`crate::io::mat::read_named_mat_volumes`]. The first role's spatial header
/// is preserved for the output geometry. Every role file must exist and share
/// the same spatial dims.
pub fn read_named_nii_volumes(dir: &Path, roles: &[&str]) -> Result<(Array4<f64>, NiftiHeader)> {
    let paths: Vec<PathBuf> = roles
        .iter()
        .map(|r| dir.join(format!("{r}.nii.gz")))
        .collect();
    stack_nii_volumes(&paths)
}

/// Read one 3D scalar NIfTI per path and stack them into a 4D array in the
/// given order — column `i` is `paths[i]`. Used wherever a measurement ships
/// as one file per volume rather than one 4D file: a `Named` model's role
/// files, or a `Series` model's per-volume files in acquisition order. The
/// first file's spatial header is preserved for the output geometry. Every
/// file must exist and share the same spatial dims.
pub fn stack_nii_volumes(paths: &[PathBuf]) -> Result<(Array4<f64>, NiftiHeader)> {
    let mut vols: Vec<ndarray::Array3<f64>> = Vec::with_capacity(paths.len());
    let mut header: Option<NiftiHeader> = None;
    let mut dims: Option<(usize, usize, usize)> = None;
    for path in paths {
        let (v, h) = read_map_nifti_with_header(path)
            .with_context(|| format!("reading volume from {path:?}"))?;
        let d = v.dim();
        match dims {
            None => dims = Some(d),
            Some(expected) if expected != d => bail!(
                "{:?} has spatial dims {:?}, expected {:?} (from the first volume)",
                path,
                d,
                expected
            ),
            _ => {}
        }
        if header.is_none() {
            header = Some(h);
        }
        vols.push(v);
    }
    let (nx, ny, nz) = dims.with_context(|| "at least one volume is required")?;
    let mut out = Array4::<f64>::zeros((nx, ny, nz, paths.len()));
    for (t, v) in vols.iter().enumerate() {
        out.index_axis_mut(ndarray::Axis(3), t).assign(v);
    }
    Ok((out, header.expect("checked non-empty above")))
}

/// Create a 3D header from a 4D reference header. Test-only: production output
/// goes through [`write_map_nifti`] (3D for z > 1, 2D for a single slice).
#[cfg(test)]
fn make_3d_header(header_4d: &NiftiHeader) -> NiftiHeader {
    let mut h = header_4d.clone();
    h.dim[0] = 3;
    h.dim[4] = 1;
    h.pixdim[4] = 0.0;
    // Set datatype to FLOAT64 (code 64, bitpix 64)
    h.datatype = 64;
    h.bitpix = 64;
    h
}

/// Write a 3D f64 array as a NIfTI file, using a reference header for spatial
/// metadata. Test-only helper for building 3D fixtures; production map output
/// uses [`write_map_nifti`].
#[cfg(test)]
pub fn write_3d_nifti(
    data: &Array3<f64>,
    reference_header: &NiftiHeader,
    output_path: &Path,
) -> Result<()> {
    let header = make_3d_header(reference_header);
    let dyn_data = data.clone().into_dyn();
    WriterOptions::new(output_path)
        .reference_header(&header)
        .write_nifti(&dyn_data)
        .with_context(|| format!("Failed to write NIfTI to {:?}", output_path))?;
    Ok(())
}

/// Write an output map, collapsing a singleton z dimension to a genuine 2D
/// image so the file matches qMRLab's `make_nii` output (`dim[0] == 2`).
///
/// The `nifti` writer derives `dim` from the array shape, so a 2D array yields
/// a 2D NIfTI while all affine fields (qform/sform/srow/pixdim) come from
/// `reference_header`. Use this for `.mat`-sourced inputs (which carry no real
/// spatial header) so Rust maps overlay and subtract cleanly against qMRLab
/// maps. For z > 1 this writes a normal 3D volume.
pub fn write_map_nifti(
    data: &Array3<f64>,
    reference_header: &NiftiHeader,
    output_path: &Path,
) -> Result<()> {
    let (_nx, _ny, nz) = data.dim();
    let mut header = reference_header.clone();
    header.datatype = 64;
    header.bitpix = 64;
    if nz == 1 {
        // A 2D image has no temporal axis, so clear its spacing (qMRLab's
        // make_nii leaves pixdim[4] = 0), matching reference maps field-for-field.
        header.pixdim[4] = 0.0;
        // Drop the singleton z axis → 2D (nx, ny), preserving (i, j) order.
        let slice2d = data.index_axis(ndarray::Axis(2), 0).to_owned();
        WriterOptions::new(output_path)
            .reference_header(&header)
            .write_nifti(&slice2d)
            .with_context(|| format!("Failed to write NIfTI to {:?}", output_path))?;
    } else {
        let dyn_data = data.clone().into_dyn();
        WriterOptions::new(output_path)
            .reference_header(&header)
            .write_nifti(&dyn_data)
            .with_context(|| format!("Failed to write NIfTI to {:?}", output_path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array;
    use std::path::PathBuf;

    /// A unique tempdir, removed on drop.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "qmrust-nifti-test-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The value `write_nifti` stores at a voxel: its C-order flat index.
    fn c_index(coords: &[usize], shape: &[usize]) -> f64 {
        let mut idx = 0usize;
        for (c, s) in coords.iter().zip(shape) {
            idx = idx * s + c;
        }
        idx as f64
    }

    /// Write a NIfTI of the given shape; values are the C-order index, so a
    /// transposed read is detectable and not just a shape mismatch.
    fn write_nifti(dir: &std::path::Path, name: &str, shape: &[usize]) -> PathBuf {
        let n: usize = shape.iter().product();
        let data = Array::from_iter((0..n).map(|i| i as f64))
            .into_shape_with_order(shape.to_vec())
            .unwrap();
        let path = dir.join(name);
        WriterOptions::new(&path).write_nifti(&data).unwrap();
        path
    }

    /// Per-volume files carry the acquisition axis *between* them, so the
    /// stack's 4th axis has to follow `paths` exactly. A silent reorder here
    /// would pair every volume with the wrong sidecar.
    #[test]
    fn stacking_follows_the_order_the_paths_were_given() {
        let dir = TempDir::new("stack-order");
        let a = write_nifti(&dir.0, "a.nii", &[2, 3, 1]);
        let b = write_nifti(&dir.0, "b.nii", &[2, 3, 1]);
        let (fwd, _) = stack_nii_volumes(&[a.clone(), b.clone()]).unwrap();
        let (rev, _) = stack_nii_volumes(&[b, a]).unwrap();
        assert_eq!(fwd.dim(), (2, 3, 1, 2));
        // Same two files, opposite order: every voxel swaps between volumes.
        for x in 0..2 {
            for y in 0..3 {
                assert_eq!(fwd[[x, y, 0, 0]], rev[[x, y, 0, 1]]);
                assert_eq!(fwd[[x, y, 0, 1]], rev[[x, y, 0, 0]]);
            }
        }
    }

    /// Volumes of different sizes cannot be one series, and stacking them would
    /// otherwise write whichever geometry came first over all of them.
    #[test]
    fn stacking_rejects_volumes_of_different_dimensions() {
        let dir = TempDir::new("stack-dims");
        let a = write_nifti(&dir.0, "a.nii", &[2, 3, 1]);
        let b = write_nifti(&dir.0, "b.nii", &[2, 4, 1]);
        let err = stack_nii_volumes(&[a, b]).unwrap_err().to_string();
        assert!(err.contains("expected"), "unhelpful dims error: {err}");
    }

    /// The output geometry is the first volume's, matching what the 4D reader
    /// returns for a single file.
    #[test]
    fn stacking_keeps_the_first_volumes_header() {
        let dir = TempDir::new("stack-header");
        let a = write_nifti(&dir.0, "a.nii", &[2, 3, 1]);
        let b = write_nifti(&dir.0, "b.nii", &[2, 3, 1]);
        let (_, first) = read_map_nifti_with_header(&a).unwrap();
        let (_, stacked) = stack_nii_volumes(&[a, b]).unwrap();
        assert_eq!(stacked.srow_x, first.srow_x);
        assert_eq!(stacked.pixdim, first.pixdim);
    }

    /// NIfTI's `dim[0]` counts dimensions, and a value of 3 declares three
    /// *spatial* axes — there is no temporal axis in the file at all. Reading
    /// the third one as time turns a 143x218x220 brain into one slice imaged
    /// 220 times, which is what a single-volume model (mp2rage, mt_ratio's
    /// individual inputs) is handed.
    #[test]
    fn a_3d_volume_keeps_its_third_axis_spatial() {
        let tmp = TempDir::new("3d-spatial");
        let path = write_nifti(&tmp.0, "vol.nii", &[4, 5, 6]);
        let (arr, _) = read_4d_nifti(&path, 1).unwrap();
        assert_eq!(
            arr.dim(),
            (4, 5, 6, 1),
            "a 3D NIfTI is one volume of 3D data, not 6 timepoints of a single slice"
        );
    }

    /// The voxel at a known spatial position must survive the read. If the
    /// third axis is silently reinterpreted as time, this lands somewhere
    /// else entirely.
    #[test]
    fn a_3d_volume_keeps_its_voxels_where_they_were() {
        let tmp = TempDir::new("3d-voxels");
        let path = write_nifti(&tmp.0, "vol.nii", &[4, 5, 6]);
        let (arr, _) = read_4d_nifti(&path, 1).unwrap();
        assert_eq!(arr[[1, 2, 3, 0]], c_index(&[1, 2, 3], &[4, 5, 6]));
    }

    /// The case the 3D branch was written for is properly a *4D* file with a
    /// singleton z — which is exactly how every dataset qmrust ships is
    /// stored (e.g. inversion_recovery.nii.gz is 128x128x1x9). This must keep
    /// working unchanged.
    #[test]
    fn a_single_slice_series_is_stored_4d_and_still_loads() {
        let tmp = TempDir::new("4d-series");
        let path = write_nifti(&tmp.0, "series.nii", &[4, 5, 1, 9]);
        let (arr, _) = read_4d_nifti(&path, 9).unwrap();
        assert_eq!(arr.dim(), (4, 5, 1, 9));
    }

    /// A genuine multi-slice, multi-volume acquisition is unaffected either
    /// way; this pins the common case against regression.
    #[test]
    fn a_4d_volume_series_is_unchanged() {
        let tmp = TempDir::new("4d-full");
        let path = write_nifti(&tmp.0, "full.nii", &[4, 5, 6, 3]);
        let (arr, _) = read_4d_nifti(&path, 3).unwrap();
        assert_eq!(arr.dim(), (4, 5, 6, 3));
        assert_eq!(arr[[1, 2, 3, 2]], c_index(&[1, 2, 3, 2], &[4, 5, 6, 3]));
    }

    /// The historical reading stays available for a model that wants it: a 3D
    /// file whose third extent matches the expected volume count is a
    /// single-slice series.
    #[test]
    fn a_3d_file_is_a_single_slice_series_when_the_model_expects_several() {
        let tmp = TempDir::new("3d-series");
        let path = write_nifti(&tmp.0, "series.nii", &[4, 5, 6]);
        let (arr, _) = read_4d_nifti(&path, 6).unwrap();
        assert_eq!(arr.dim(), (4, 5, 1, 6));
    }

    /// A 3D file that matches neither reading is rejected where the shape is
    /// known, naming both the file and the count the model wanted — rather
    /// than being reshaped into something the engine rejects later with a
    /// volume count that is itself an artefact of the read.
    #[test]
    fn a_3d_file_matching_neither_reading_is_rejected_at_load() {
        let tmp = TempDir::new("3d-mismatch");
        let path = write_nifti(&tmp.0, "vol.nii", &[4, 5, 6]);
        let err = read_4d_nifti(&path, 3).unwrap_err().to_string();
        assert!(err.contains("4x5x6"), "error should name the shape: {err}");
        assert!(
            err.contains("expects 3 volumes"),
            "error should name the count: {err}"
        );
    }
}
