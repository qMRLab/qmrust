//! NIfTI I/O helpers for reading 4D IR data / 3D masks and writing 3D output maps.

use anyhow::{bail, Context, Result};
use ndarray::{Array3, Array4};
use nifti::writer::WriterOptions;
use nifti::{IntoNdArray, NiftiHeader, NiftiObject, ReaderOptions};
use std::path::Path;

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

/// Read a 4D NIfTI file (IR data). Returns (data, header).
/// The 4th dimension corresponds to different TI volumes.
pub fn read_4d_nifti(path: &Path) -> Result<(Array4<f64>, NiftiHeader)> {
    let (data, header) = read_nifti_raw(path)?;
    let shape = data.shape();
    match shape.len() {
        4 => {
            let arr = data
                .into_dimensionality::<ndarray::Ix4>()
                .map_err(|e| anyhow::anyhow!("Failed to reshape to 4D: {}", e))?;
            Ok((arr, header))
        }
        3 => {
            // Treat 3D as 4D with z=1: (x, y, n_ti) with no z slice -> (x, y, 1, n_ti).
            // `into_ndarray` returns Fortran-ordered memory (see the `nifti`
            // crate's doc note); pulling the raw buffer and re-wrapping it
            // with `from_shape_vec` (which assumes C order) would silently
            // transpose non-square data. `insert_axis` adds the singleton
            // dimension by logical index instead, so it's layout-agnostic.
            let arr = data
                .insert_axis(ndarray::Axis(2))
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
