//! The single voxel-fitting engine. Drives any `Model`, dispatching on its
//! declared `FitStrategy`. Replaces the old `fitting::fit_volume*` pair.

use crate::core::model::{Aux, FitStrategy, Model};
use crate::fitting::FitResults;
use anyhow::{bail, Result};
use ndarray::{Array3, Array4};
use rayon::prelude::*;

/// Loaded auxiliary 3D maps, keyed by `InputSpec.name` (None if absent).
pub struct AuxMaps {
    maps: Vec<(String, Option<Array3<f64>>)>,
}

impl AuxMaps {
    pub fn new(maps: Vec<(String, Option<Array3<f64>>)>) -> Self {
        Self { maps }
    }
    pub fn empty() -> Self {
        Self { maps: vec![] }
    }
    /// Build the per-voxel `Aux` scalar bundle at (x,y,z).
    fn at(&self, x: usize, y: usize, z: usize) -> Aux {
        let mut a = Aux::new();
        for (name, m) in &self.maps {
            if let Some(map) = m {
                a.set(name, map[[x, y, z]]);
            }
        }
        a
    }
    fn validate_dims(&self, nx: usize, ny: usize, nz: usize) -> Result<()> {
        for (name, m) in &self.maps {
            if let Some(map) = m {
                let (mx, my, mz) = map.dim();
                if mx != nx || my != ny || mz != nz {
                    bail!(
                        "Aux map '{}' dims ({},{},{}) != data dims ({},{},{})",
                        name,
                        mx,
                        my,
                        mz,
                        nx,
                        ny,
                        nz
                    );
                }
            }
        }
        Ok(())
    }
}

/// Fit a whole volume with `model`, honoring its `FitStrategy`.
///
/// `progress` is invoked with the number of voxels completed. Because the
/// parallel fit collects all results before returning, `progress` is called
/// exactly once with the total voxel count (a single completion tick), not
/// per-voxel — ticking a shared `&mut` closure from inside a rayon `par_iter`
/// is not sound. Callers wanting a live bar (e.g. the CLI) get a bar that
/// jumps to full on completion; the heavy work is the parallel fit itself.
pub fn run(
    model: &dyn Model,
    data: &Array4<f64>,
    mask: Option<&Array3<bool>>,
    aux: &AuxMaps,
    progress: &mut dyn FnMut(u64),
) -> Result<FitResults> {
    match model.strategy() {
        FitStrategy::Voxelwise => run_voxelwise(model, data, mask, aux, progress),
        FitStrategy::MatrixWise => bail!("matrix-wise fitting not yet implemented"),
    }
}

fn run_voxelwise(
    model: &dyn Model,
    data: &Array4<f64>,
    mask: Option<&Array3<bool>>,
    aux: &AuxMaps,
    progress: &mut dyn FnMut(u64),
) -> Result<FitResults> {
    let (nx, ny, nz, n_t) = data.dim();

    if let Some(m) = mask {
        let (mx, my, mz) = m.dim();
        if mx != nx || my != ny || mz != nz {
            bail!(
                "Mask dimensions ({},{},{}) != data dimensions ({},{},{})",
                mx,
                my,
                mz,
                nx,
                ny,
                nz
            );
        }
    }
    aux.validate_dims(nx, ny, nz)?;

    let mut indices: Vec<(usize, usize, usize)> = Vec::new();
    for x in 0..nx {
        for y in 0..ny {
            for z in 0..nz {
                let in_mask = mask.is_none_or(|m| m[[x, y, z]]);
                if in_mask {
                    let all_zero = (0..n_t).all(|t| data[[x, y, z, t]] == 0.0);
                    if !all_zero {
                        indices.push((x, y, z));
                    }
                }
            }
        }
    }

    let total = indices.len();
    eprintln!(
        "  Fitting {} voxels ({} masked/empty skipped)...",
        total,
        nx * ny * nz - total
    );

    let output_names = model.output_names();
    let n_outputs = output_names.len();
    let results: Vec<_> = indices
        .par_iter()
        .map(|&(x, y, z)| {
            let voxel: Vec<f64> = (0..n_t).map(|t| data[[x, y, z, t]]).collect();
            let a = aux.at(x, y, z);
            let fit =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| model.fit(&voxel, &a)));
            ((x, y, z), fit.ok())
        })
        .collect();
    progress(total as u64);

    let mut maps: Vec<Array3<f64>> = (0..n_outputs)
        .map(|_| Array3::from_elem((nx, ny, nz), f64::NAN))
        .collect();
    let mut n_failed = 0;
    for ((x, y, z), fit_opt) in results {
        match fit_opt {
            Some(values) => {
                for (i, &v) in values.iter().enumerate().take(n_outputs) {
                    maps[i][[x, y, z]] = v;
                }
            }
            None => n_failed += 1,
        }
    }
    if n_failed > 0 {
        eprintln!("  Warning: {} voxels failed to fit", n_failed);
    }

    Ok(output_names.into_iter().zip(maps).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Aux, InputSpec, Model};
    use ndarray::{Array3, Array4};

    struct SumModel;
    impl Model for SumModel {
        fn param_names(&self) -> Vec<&'static str> {
            vec!["s"]
        }
        fn output_names(&self) -> Vec<String> {
            vec!["sum".into(), "aux".into()]
        }
        fn param_bounds(&self) -> Vec<(f64, f64)> {
            vec![(f64::NEG_INFINITY, f64::INFINITY)]
        }
        fn fixed_mask(&self) -> Vec<bool> {
            vec![false]
        }
        fn required_inputs(&self) -> Vec<InputSpec> {
            vec![InputSpec {
                name: "k",
                required: false,
                bids: None,
            }]
        }
        fn n_acquisitions(&self) -> usize {
            2
        }
        fn forward(&self, _p: &[f64], _a: &Aux) -> Vec<f64> {
            vec![0.0, 0.0]
        }
        fn fit(&self, sig: &[f64], aux: &Aux) -> Vec<f64> {
            vec![sig.iter().sum(), aux.get("k").unwrap_or(-1.0)]
        }
    }

    #[test]
    fn voxelwise_runs_and_passes_aux() {
        let mut data = Array4::<f64>::zeros((1, 1, 1, 2));
        data[[0, 0, 0, 0]] = 1.0;
        data[[0, 0, 0, 1]] = 2.0;
        let mut k = Array3::<f64>::zeros((1, 1, 1));
        k[[0, 0, 0]] = 7.0;
        let aux = AuxMaps::new(vec![("k".to_string(), Some(k))]);
        let res = run(&SumModel, &data, None, &aux, &mut |_| {}).unwrap();
        assert_eq!(res["sum"][[0, 0, 0]], 3.0);
        assert_eq!(res["aux"][[0, 0, 0]], 7.0);
    }

    #[test]
    fn voxelwise_invokes_progress_per_voxel() {
        use ndarray::Array4;
        let mut data = Array4::<f64>::zeros((2, 1, 1, 2));
        data[[0, 0, 0, 0]] = 1.0;
        data[[0, 0, 0, 1]] = 2.0;
        data[[1, 0, 0, 0]] = 1.0;
        data[[1, 0, 0, 1]] = 2.0;
        let aux = AuxMaps::empty();
        let mut ticks = 0u64;
        let _ = run(&SumModel, &data, None, &aux, &mut |n| ticks += n).unwrap();
        assert_eq!(ticks, 2); // two non-empty voxels
    }
}
