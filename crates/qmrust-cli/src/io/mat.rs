//! MATLAB .mat file reading support.
//!
//! Reads qMRLab-style .mat files containing a 4D measurement array and an
//! optional mask. The acquisition protocol (TIs, flip angles, …) is not read
//! from the `.mat` here — it comes from the model's own `--config` (the
//! non-BIDS contract; see CLAUDE.md "Units — BIDS-native").

use anyhow::{bail, Context, Result};
use matfile::MatFile;
use ndarray::{Array3, Array4};
use std::path::Path;

/// A model-agnostic `.mat` source: a stacked 4D measurement and optional mask.
pub struct MatDataset {
    /// 4D measurement array (x, y, z, n_volumes). For 2D data, z=1.
    pub data: Array4<f64>,
    /// Optional binary mask (x, y, z).
    pub mask: Option<Array3<bool>>,
}

/// Extract f64 values from a matfile NumericData.
pub(crate) fn numeric_to_f64(data: &matfile::NumericData) -> Vec<f64> {
    use matfile::NumericData::*;
    match data {
        Double { real, .. } => real.clone(),
        Single { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int8 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt8 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int16 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt16 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int32 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt32 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int64 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt64 { real, .. } => real.iter().map(|&v| v as f64).collect(),
    }
}

/// Read a MATLAB .mat file containing a stacked measurement.
///
/// Expected variables:
///   - `IRdata`/`IRData`/`MTdata`/`MTData`: 2D (x*y, n_vol) or 3D (x, y, n_vol)
///     or 4D (x, y, z, n_vol)
///   - `Mask` (optional): 2D (x, y) or 3D (x, y, z)
pub fn read_mat_file(path: &Path) -> Result<MatDataset> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open .mat file {:?}", path))?;
    let mat =
        MatFile::parse(file).with_context(|| format!("Failed to parse .mat file {:?}", path))?;

    // Find the measurement array (try all known naming conventions).
    let data_array = mat
        .find_by_name("IRdata")
        .or_else(|| mat.find_by_name("IRData"))
        .or_else(|| mat.find_by_name("MTdata"))
        .or_else(|| mat.find_by_name("MTData"))
        .with_context(|| {
            let names: Vec<_> = mat.arrays().iter().map(|a| a.name()).collect();
            format!(
                "No 'IRdata'/'IRData'/'MTdata'/'MTData' variable found in {:?}. Available: {:?}",
                path, names
            )
        })?;

    let data_size = data_array.size().to_vec();
    let data_raw = numeric_to_f64(data_array.data());

    // MATLAB stores data in column-major order. ndarray default is row-major.
    // We need to reshape accounting for this.
    let data = match data_size.len() {
        2 => {
            let (nrows, ncols) = (data_size[0], data_size[1]);
            // Treat as (nrows, 1, 1, ncols): each row is a voxel, cols are volumes.
            // MATLAB stores column-major, so column j occupies indices
            // j*nrows..(j+1)*nrows.
            let mut arr = Array4::<f64>::zeros((nrows, 1, 1, ncols));
            for j in 0..ncols {
                for i in 0..nrows {
                    arr[[i, 0, 0, j]] = data_raw[j * nrows + i];
                }
            }
            arr
        }
        3 => {
            // (x, y, n_vol) — add z=1 → (x, y, 1, n_vol)
            let (nx, ny, n_vol) = (data_size[0], data_size[1], data_size[2]);
            // MATLAB column-major: index = i + j*nx + k*nx*ny
            let mut arr = Array4::<f64>::zeros((nx, ny, 1, n_vol));
            for k in 0..n_vol {
                for j in 0..ny {
                    for i in 0..nx {
                        arr[[i, j, 0, k]] = data_raw[k * nx * ny + j * nx + i];
                    }
                }
            }
            arr
        }
        4 => {
            let (nx, ny, nz, n_vol) = (data_size[0], data_size[1], data_size[2], data_size[3]);
            let mut arr = Array4::<f64>::zeros((nx, ny, nz, n_vol));
            for t in 0..n_vol {
                for k in 0..nz {
                    for j in 0..ny {
                        for i in 0..nx {
                            arr[[i, j, k, t]] =
                                data_raw[t * nx * ny * nz + k * nx * ny + j * nx + i];
                        }
                    }
                }
            }
            arr
        }
        _ => bail!(
            "measurement array has unsupported dimensionality: {:?} (expected 2D, 3D, or 4D)",
            data_size
        ),
    };

    // Read optional Mask
    let mask = if let Some(mask_array) = mat.find_by_name("Mask") {
        let mask_size = mask_array.size().to_vec();
        let mask_raw = numeric_to_f64(mask_array.data());

        let mask_arr = match mask_size.len() {
            2 => {
                let (nx, ny) = (mask_size[0], mask_size[1]);
                let mut arr = Array3::<bool>::from_elem((nx, ny, 1), false);
                for j in 0..ny {
                    for i in 0..nx {
                        arr[[i, j, 0]] = mask_raw[j * nx + i] > 0.0;
                    }
                }
                arr
            }
            3 => {
                let (nx, ny, nz) = (mask_size[0], mask_size[1], mask_size[2]);
                let mut arr = Array3::<bool>::from_elem((nx, ny, nz), false);
                for k in 0..nz {
                    for j in 0..ny {
                        for i in 0..nx {
                            arr[[i, j, k]] = mask_raw[k * nx * ny + j * nx + i] > 0.0;
                        }
                    }
                }
                arr
            }
            _ => bail!("Mask has unsupported dimensionality: {:?}", mask_size),
        };
        Some(mask_arr)
    } else {
        None
    };

    let (nx, ny, nz, n_vol) = data.dim();
    eprintln!(
        "  Loaded .mat: data {}x{}x{}x{}, mask={}",
        nx,
        ny,
        nz,
        n_vol,
        if mask.is_some() { "yes" } else { "no" },
    );

    Ok(MatDataset { data, mask })
}

/// Read a mask from a separate .mat file.
/// Looks for a variable named 'Mask', 'mask', or the first numeric array.
pub fn read_mask_mat(path: &Path) -> Result<Array3<bool>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open mask .mat file {:?}", path))?;
    let mat = MatFile::parse(file)
        .with_context(|| format!("Failed to parse mask .mat file {:?}", path))?;

    let mask_array = mat
        .find_by_name("Mask")
        .or_else(|| mat.find_by_name("mask"))
        .or_else(|| mat.arrays().first())
        .with_context(|| format!("No arrays found in mask file {:?}", path))?;

    let mask_size = mask_array.size().to_vec();
    let mask_raw = numeric_to_f64(mask_array.data());

    let mask_arr = match mask_size.len() {
        2 => {
            let (nx, ny) = (mask_size[0], mask_size[1]);
            let mut arr = Array3::<bool>::from_elem((nx, ny, 1), false);
            for j in 0..ny {
                for i in 0..nx {
                    arr[[i, j, 0]] = mask_raw[j * nx + i] > 0.0;
                }
            }
            arr
        }
        3 => {
            let (nx, ny, nz) = (mask_size[0], mask_size[1], mask_size[2]);
            let mut arr = Array3::<bool>::from_elem((nx, ny, nz), false);
            for k in 0..nz {
                for j in 0..ny {
                    for i in 0..nx {
                        arr[[i, j, k]] = mask_raw[k * nx * ny + j * nx + i] > 0.0;
                    }
                }
            }
            arr
        }
        _ => bail!("Mask has unsupported dimensionality: {:?}", mask_size),
    };

    eprintln!("  Loaded mask from .mat: {:?}", mask_arr.dim());
    Ok(mask_arr)
}

/// Reshape a column-major 2D MATLAB buffer into an (nx, ny, 1) f64 array.
pub(crate) fn reshape_map_2d(raw: &[f64], nx: usize, ny: usize) -> Array3<f64> {
    let mut arr = Array3::<f64>::zeros((nx, ny, 1));
    for j in 0..ny {
        for i in 0..nx {
            arr[[i, j, 0]] = raw[j * nx + i];
        }
    }
    arr
}

/// Reshape a column-major 3D MATLAB buffer into an (nx, ny, nz) f64 array.
pub(crate) fn reshape_map_3d(raw: &[f64], nx: usize, ny: usize, nz: usize) -> Array3<f64> {
    let mut arr = Array3::<f64>::zeros((nx, ny, nz));
    for k in 0..nz {
        for j in 0..ny {
            for i in 0..nx {
                arr[[i, j, k]] = raw[k * nx * ny + j * nx + i];
            }
        }
    }
    arr
}

/// Read a scalar map (R1map/B1map/B0map) from a single-variable .mat file.
/// Uses the first array in the file (these files hold exactly one variable).
pub fn read_map_mat(path: &Path) -> Result<Array3<f64>> {
    let file =
        std::fs::File::open(path).with_context(|| format!("Failed to open .mat map {:?}", path))?;
    let mat =
        MatFile::parse(file).with_context(|| format!("Failed to parse .mat map {:?}", path))?;
    let arr = mat
        .arrays()
        .first()
        .with_context(|| format!("No arrays found in {:?}", path))?;
    let size = arr.size().to_vec();
    let raw = numeric_to_f64(arr.data());
    let out = match size.len() {
        2 => reshape_map_2d(&raw, size[0], size[1]),
        3 => reshape_map_3d(&raw, size[0], size[1], size[2]),
        _ => bail!("Map {:?} has unsupported dims {:?}", path, size),
    };
    Ok(out)
}

/// Read a `Named` model's role volumes from a `--mat-dir`: one single-variable
/// `<role>.mat` per role (each read like an auxiliary map), stacked into a 4D
/// array in the given role order — column `i` is `roles[i]`. This is the
/// `.mat` counterpart to a BIDS named set, where each role is a separate file
/// rather than one stacked measurement array. Every role file must exist and
/// share the same spatial dims.
pub fn read_named_mat_volumes(dir: &Path, roles: &[&str]) -> Result<Array4<f64>> {
    let mut vols: Vec<Array3<f64>> = Vec::with_capacity(roles.len());
    let mut dims: Option<(usize, usize, usize)> = None;
    for &role in roles {
        let path = dir.join(format!("{role}.mat"));
        let v = read_map_mat(&path)
            .with_context(|| format!("reading named role '{role}' from {:?}", path))?;
        let d = v.dim();
        match dims {
            None => dims = Some(d),
            Some(expected) if expected != d => bail!(
                "role '{}' has spatial dims {:?}, expected {:?} (from the first role)",
                role,
                d,
                expected
            ),
            _ => {}
        }
        vols.push(v);
    }
    let (nx, ny, nz) = dims.with_context(|| "a named model must declare at least one role")?;
    let mut out = Array4::<f64>::zeros((nx, ny, nz, roles.len()));
    for (t, v) in vols.iter().enumerate() {
        out.index_axis_mut(ndarray::Axis(3), t).assign(v);
    }
    Ok(out)
}

#[cfg(test)]
mod map_tests {
    use super::*;

    #[test]
    fn reshape_2d_column_major_to_3d() {
        // MATLAB column-major (nx=2, ny=3): stored 0,1,2,3,4,5 -> col-major
        let raw = vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0];
        let arr = reshape_map_2d(&raw, 2, 3);
        assert_eq!(arr.dim(), (2, 3, 1));
        assert_eq!(arr[[0, 0, 0]], 0.0);
        assert_eq!(arr[[1, 0, 0]], 1.0);
        assert_eq!(arr[[0, 1, 0]], 2.0);
    }
}
