//! `qmrust mtsat-b1`: build a TardifLab MTsat B1-correction artifact by
//! simulating the sequence surface and self-calibrating on a reference MTS
//! dataset.

use anyhow::{Context, Result};
use rayon::prelude::*;
use serde::Deserialize;
use std::path::PathBuf;

use qmrust_core::mtsat_b1::calibrate::{fit_m0b, regress_m0b_vs_r1};
use qmrust_core::mtsat_b1::fitvalues::FitValues;
use qmrust_core::mtsat_b1::sim::{mtsat_sim, SeqParams, VfaParams};
use qmrust_core::mtsat_b1::surface::{self, SsSurface};

#[derive(Debug, Deserialize)]
pub struct SeqConfig {
    pub seq: SeqParams,
    pub vfa: VfaParams,
    pub grid: GridSpec,
    pub b1_ref: f64,
}

#[derive(Debug, Deserialize)]
pub struct GridSpec {
    pub m0b: Range,
    pub t1obs: Vec<f64>,
    pub b1_steps: usize,
    pub b1_max_factor: f64,
}

#[derive(Debug, Deserialize)]
pub struct Range {
    pub start: f64,
    pub step: f64,
    pub stop: f64,
}

impl SeqConfig {
    /// Cartesian product of `(M0b, b1, Raobs)`, matching `simSeq_M0b_R1obs.m`.
    /// This point order is fixed by `surface::fit`'s basis (`M0b` outer, `b1`
    /// middle, `Raobs` inner).
    pub fn build_grid(&self) -> Vec<[f64; 3]> {
        let m0bs: Vec<f64> = {
            let mut v = Vec::new();
            let mut x = self.grid.m0b.start;
            while x <= self.grid.m0b.stop + 1e-12 {
                v.push(x);
                x += self.grid.m0b.step;
            }
            v
        };
        let b1max = self.b1_ref * self.grid.b1_max_factor;
        let b1s: Vec<f64> = (0..self.grid.b1_steps)
            .map(|i| b1max * i as f64 / (self.grid.b1_steps as f64 - 1.0))
            .collect();
        let raobs: Vec<f64> = self.grid.t1obs.iter().map(|t| 1.0 / t).collect();
        let mut g = Vec::new();
        for &m in &m0bs {
            for &b in &b1s {
                for &r in &raobs {
                    g.push([m, b, r]);
                }
            }
        }
        g
    }
}

pub struct MtSatB1Args {
    pub seq: PathBuf,
    pub bids_dir: PathBuf,
    pub out: PathBuf,
}

/// Minimum number of masked, valid (`MTsat > 0`) reference voxels required to
/// trust the M0b-vs-R1 regression.
const MIN_CALIBRATION_VOXELS: usize = 100;

pub fn run_mtsat_b1(args: MtSatB1Args) -> Result<()> {
    let text =
        std::fs::read_to_string(&args.seq).with_context(|| format!("reading {:?}", args.seq))?;
    let cfg: SeqConfig = serde_yaml::from_str(&text)?;

    // 1-2. Simulate the surface (parallel over the grid).
    let grid = cfg.build_grid();
    eprintln!("Simulating {} grid points...", grid.len());
    let samples: Vec<([f64; 3], f64)> = grid
        .par_iter()
        .map(|&pt| {
            let (m0b, b1, raobs) = (pt[0], pt[1], pt[2]);
            (pt, mtsat_sim(&cfg.seq, &cfg.vfa, m0b, raobs, b1))
        })
        .collect();
    let ss_surface: SsSurface = surface::fit(&samples);

    // 3. Reference dataset: R1 + MTSAT + B1 per masked/valid voxel (reuses the
    //    mt_sat BIDS fit machinery).
    let voxels = crate::commands::collect_mtsat_r1_b1(&args.bids_dir)?;

    // 4. Per-voxel M0b, then regress on R1.
    let m0b_samples: Vec<(f64, f64)> = voxels
        .par_iter()
        .filter(|(_, mtsat, _)| *mtsat > 0.0)
        .map(|&(r1, mtsat, b1)| (r1, fit_m0b(&ss_surface, cfg.b1_ref * b1, r1, mtsat)))
        .collect();
    anyhow::ensure!(
        m0b_samples.len() > MIN_CALIBRATION_VOXELS,
        "too few valid calibration voxels ({}, need > {})",
        m0b_samples.len(),
        MIN_CALIBRATION_VOXELS
    );
    let m0b_vs_r1 = regress_m0b_vs_r1(&m0b_samples);

    // 5. Write the artifact.
    let fv = FitValues {
        ss_surface,
        m0b_vs_r1,
        seq: cfg.seq,
        vfa: cfg.vfa,
        b1_ref: cfg.b1_ref,
    };
    std::fs::write(&args.out, serde_yaml::to_string(&fv)?)?;
    eprintln!("Wrote {:?}", args.out);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seq_yaml_parses_into_params_and_grid() {
        let yaml = r#"
seq:
  num_sat_pulse: 2
  pulse_dur: 0.000768
  pulse_gap_dur: 0.0006
  tr: 0.028
  w_exc_dur: 0.003
  num_excitation: 1
  freq_pattern: DualAlternate
  delta: 7000
  flip_angle: 9
  sat_shape: Hanning
  r: 26
  t2a: 0.07
  t1d: 0.006
  m0a: 1
  rb: 1
  t2b: 0.000012
vfa: { fa1_deg: 5, fa2_deg: 20, tr1: 0.03, tr2: 0.03 }
grid:
  m0b: { start: 0.0, step: 0.025, stop: 0.20 }
  t1obs: [0.6, 0.8, 1.0, 1.4, 2.0, 3.0, 4.5]
  b1_steps: 15
  b1_max_factor: 1.3
b1_ref: 9.0
"#;
        let cfg: SeqConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.seq.num_sat_pulse, 2);
        assert_eq!(cfg.b1_ref, 9.0);
        let grid = cfg.build_grid();
        assert!(!grid.is_empty());
    }
}
