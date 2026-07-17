//! MATLAB .mat file reading support.
//!
//! Reads IR_demo-style .mat files containing IRdata, Mask, and TI arrays.

use anyhow::{bail, Context, Result};
use matfile::MatFile;
use ndarray::{Array3, Array4};
use std::path::Path;

/// Data loaded from a .mat file.
pub struct MatData {
    /// 4D IR data array (x, y, z, n_ti). For 2D data, z=1.
    pub ir_data: Array4<f64>,
    /// Optional binary mask (x, y, z).
    pub mask: Option<Array3<bool>>,
    /// Inversion times in ms (overrides config if present).
    pub ti: Option<Vec<f64>>,
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

/// Read a MATLAB .mat file containing IR data.
///
/// Expected variables:
///   - `IRdata` or `IRData`: 2D (x*y, n_ti) or 3D (x, y, n_ti) or 4D (x, y, z, n_ti)
///   - `Mask` (optional): 2D (x, y) or 3D (x, y, z)
///   - `TI` (optional): 1D vector of inversion times in ms
pub fn read_mat_file(path: &Path) -> Result<MatData> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open .mat file {:?}", path))?;
    let mat =
        MatFile::parse(file).with_context(|| format!("Failed to parse .mat file {:?}", path))?;

    // Find IR data array (try both naming conventions)
    let ir_array = mat
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

    let ir_size = ir_array.size().to_vec();
    let ir_data_raw = numeric_to_f64(ir_array.data());

    // MATLAB stores data in column-major order. ndarray default is row-major.
    // We need to reshape accounting for this.
    let ir_data = match ir_size.len() {
        2 => {
            // (rows, cols) in MATLAB = (spatial, n_ti) — treat as (rows, 1, 1, cols)
            // But more commonly IR_demo has shape (x, y, n_ti) stored as 2D
            // Actually MATLAB 2D means (x*y, n_ti) or (x, n_ti)
            let (nrows, ncols) = (ir_size[0], ir_size[1]);
            // Treat as (nrows, 1, 1, ncols) — each row is a voxel, cols are TI points
            // MATLAB column-major: data is stored column by column
            // Column j values are at indices j*nrows..(j+1)*nrows
            let mut arr = Array4::<f64>::zeros((nrows, 1, 1, ncols));
            for j in 0..ncols {
                for i in 0..nrows {
                    arr[[i, 0, 0, j]] = ir_data_raw[j * nrows + i];
                }
            }
            arr
        }
        3 => {
            // (x, y, n_ti) — add z=1 → (x, y, 1, n_ti)
            let (nx, ny, n_ti) = (ir_size[0], ir_size[1], ir_size[2]);
            // MATLAB column-major: index = i + j*nx + k*nx*ny
            let mut arr = Array4::<f64>::zeros((nx, ny, 1, n_ti));
            for k in 0..n_ti {
                for j in 0..ny {
                    for i in 0..nx {
                        arr[[i, j, 0, k]] = ir_data_raw[k * nx * ny + j * nx + i];
                    }
                }
            }
            arr
        }
        4 => {
            let (nx, ny, nz, n_ti) = (ir_size[0], ir_size[1], ir_size[2], ir_size[3]);
            let mut arr = Array4::<f64>::zeros((nx, ny, nz, n_ti));
            for t in 0..n_ti {
                for k in 0..nz {
                    for j in 0..ny {
                        for i in 0..nx {
                            arr[[i, j, k, t]] =
                                ir_data_raw[t * nx * ny * nz + k * nx * ny + j * nx + i];
                        }
                    }
                }
            }
            arr
        }
        _ => bail!(
            "IRdata has unsupported dimensionality: {:?} (expected 2D, 3D, or 4D)",
            ir_size
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

    // Read optional TI vector
    let ti = if let Some(ti_array) = mat.find_by_name("TI") {
        let ti_raw = numeric_to_f64(ti_array.data());
        Some(ti_raw)
    } else {
        None
    };

    let (nx, ny, nz, n_ti) = ir_data.dim();
    eprintln!(
        "  Loaded .mat: IRdata {}x{}x{}x{}, mask={}, TI={}",
        nx,
        ny,
        nz,
        n_ti,
        if mask.is_some() { "yes" } else { "no" },
        if let Some(ti) = ti.as_ref() {
            format!("{} values", ti.len())
        } else {
            "not in file".to_string()
        }
    );

    Ok(MatData { ir_data, mask, ti })
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
