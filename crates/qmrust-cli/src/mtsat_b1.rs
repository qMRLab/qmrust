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
  freq_pattern: DualAlternate
  delta: 7000
  flip_angle: 9
  r: 26
  t2a: 0.07
  t1d: 0.006
  m0a: 1
  r1b: 1
  t2b: 0.000012
  bw: 390.625
  mt_grad_time: 0.0
  n_avg: 20
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

    /// Guards the shipped recipe against `SeqParams` field drift: a stale
    /// recipe fails this test at build time instead of the CLI at runtime.
    #[test]
    fn shipped_recipe_parses() {
        let yaml = include_str!("../../../recipes/mtsat_b1_seq.yaml");
        let cfg: SeqConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.seq.n_avg, 20);
        assert_eq!(
            cfg.seq.freq_pattern,
            qmrust_core::mtsat_b1::sim::FreqPattern::DualAlternate
        );
    }

    /// External validation: `flash_signal` must reproduce TardifLab's
    /// `BlochSimFlashSequence_v2` (single-echo FLASH) reference signals. Needs
    /// the reference CSV (columns `M0b,b1,R1,satFlipAngle,sig_MTw,sig_PDw,
    /// sig_T1w`) produced by `matlab/mtsat_b1_reference.m`, supplied via env var
    /// — no MATLAB here, so `#[ignore]`d by default. Run with:
    ///
    /// ```text
    /// QMRUST_MTSAT_B1_REF_CSV=<path>/mtsat_b1_reference.csv \
    ///   cargo test -p qmrust-cli --release mtsat_b1_flash_matches_matlab_reference -- --ignored --nocapture
    /// ```
    ///
    /// The sequence/VFA parameters come from the shipped recipe, so the CSV must
    /// have been generated with the same SHARED-PARAMS values. Signals are
    /// absolute transverse magnitudes (M0a = 1) on both sides — compared
    /// directly. Targets: median relative error < 1%, max < 2% (the residual is
    /// step-size, `expm`, and the `satFlipAngle`↔`b1rms` / lineshape-bandwidth
    /// conventions, largest in the strong-saturation corner).
    #[test]
    #[ignore]
    fn mtsat_b1_flash_matches_matlab_reference() {
        use qmrust_core::mtsat_b1::sim::flash_signal;
        let csv = std::env::var("QMRUST_MTSAT_B1_REF_CSV")
            .expect("set QMRUST_MTSAT_B1_REF_CSV=<path>/mtsat_b1_reference.csv");
        let text = std::fs::read_to_string(&csv).unwrap();
        let cfg: SeqConfig =
            serde_yaml::from_str(include_str!("../../../recipes/mtsat_b1_seq.yaml")).unwrap();
        let seq = cfg.seq;
        let mut pdw_seq = seq;
        pdw_seq.tr = cfg.vfa.tr1;
        let mut t1w_seq = seq;
        t1w_seq.tr = cfg.vfa.tr2;

        let mut errs = Vec::new();
        for line in text.lines().skip(1) {
            let f: Vec<f64> = line
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            if f.len() < 7 {
                continue;
            }
            let (m0b, b1, r1) = (f[0], f[1], f[2]);
            let (mtw_ref, pdw_ref, t1w_ref) = (f[4], f[5], f[6]);
            let mtw = flash_signal(&seq, m0b, r1, seq.flip_angle, b1, true);
            let pdw = flash_signal(&pdw_seq, m0b, r1, cfg.vfa.fa1_deg, 0.0, false);
            let t1w = flash_signal(&t1w_seq, m0b, r1, cfg.vfa.fa2_deg, 0.0, false);
            for (got, want) in [(mtw, mtw_ref), (pdw, pdw_ref), (t1w, t1w_ref)] {
                errs.push(100.0 * (got - want).abs() / want.abs());
            }
        }
        assert!(!errs.is_empty(), "no rows parsed from {csv}");
        let max = errs.iter().cloned().fold(0.0_f64, f64::max);
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = errs[errs.len() / 2];
        eprintln!(
            "mtsat_b1 vs MATLAB reference: {} comparisons, median {median:.3}%, max {max:.3}%",
            errs.len()
        );
        assert!(
            median < 1.0,
            "median relative error {median:.3}% exceeds 1%"
        );
        assert!(max < 2.0, "max relative error {max:.3}% exceeds 2%");
    }
}
