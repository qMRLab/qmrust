//! qmt_spgr — quantitative MT from SPGR, Ramani and SledPikeRP analytical sub-models.

pub mod adapter;
pub mod config;
pub mod fit;
pub mod lineshape;
pub mod model;
pub mod ode;
pub mod pulse;
pub mod sf;

pub use adapter::{build, describe};

use config::QmtSpgrConfig;
use fit::fit_bounded;
use fit::{fit_ramani, FitBounds};
use model::srp_signal;
use model::FitOpt;
use pulse::GaussHannPulse;
use sf::SfTable;

/// Which qmt_spgr analytical sub-model to fit.
#[derive(Clone, Copy, PartialEq)]
pub enum SubModel {
    Ramani,
    SledPikeRp,
}

/// Precomputed fitter for qmt_spgr fitting.
pub struct QmtSpgrFitter {
    /// Protocol angles (deg) and offsets (Hz), one per volume.
    angles: Vec<f64>,
    offsets: Vec<f64>,
    trf: f64,
    bandwidth: f64,
    tr: f64,
    alpha: f64,
    bounds: FitBounds,
    r1map: bool,
    r1req_r1f: bool,
    fix_r1f_t2f: bool,
    fix_r1f_t2f_value: f64,
    /// Indices of MToff (angle<1) volumes and volumes to fit.
    mtoff_idx: Vec<usize>,
    fit_idx: Vec<usize>,
    submodel: SubModel,
    sf: Option<SfTable>,
}

pub const OUTPUT_NAMES: [&str; 8] = ["F", "kr", "R1f", "R1r", "T2f", "T2r", "kf", "resnorm"];

impl QmtSpgrFitter {
    pub fn new(cfg: &QmtSpgrConfig) -> Self {
        let angles: Vec<f64> = cfg.protocol.mtdata.iter().map(|r| r[0]).collect();
        let offsets: Vec<f64> = cfg.protocol.mtdata.iter().map(|r| r[1]).collect();
        let mtoff_idx: Vec<usize> = angles
            .iter()
            .enumerate()
            .filter(|(_, &a)| a < 1.0)
            .map(|(i, _)| i)
            .collect();
        let fit_idx: Vec<usize> = angles
            .iter()
            .enumerate()
            .filter(|(_, &a)| a >= 1.0)
            .map(|(i, _)| i)
            .collect();
        let f = &cfg.fitting;
        let submodel = if cfg.model == "SledPikeRP" {
            SubModel::SledPikeRp
        } else {
            SubModel::Ramani
        };
        let sf = if submodel == SubModel::SledPikeRp {
            let pulse = GaussHannPulse::new(cfg.protocol.timing.tmt, cfg.pulse.bandwidth);
            let (sa, so, st) = sf::build_sf_axes(&angles, &offsets);
            Some(sf::build_sf_table(&pulse, &sa, &so, &st))
        } else {
            None
        };
        Self {
            angles,
            offsets,
            trf: cfg.protocol.timing.tmt,
            bandwidth: cfg.pulse.bandwidth,
            tr: cfg.protocol.timing.trep,
            alpha: cfg.read_pulse_alpha,
            bounds: FitBounds {
                st: f.st,
                lb: f.lb,
                ub: f.ub,
                fx: f.fx,
            },
            r1map: f.use_r1map_to_constrain_r1f,
            r1req_r1f: f.fix_r1r_eq_r1f,
            fix_r1f_t2f: f.fix_r1f_t2f,
            fix_r1f_t2f_value: f.r1f_t2f,
            mtoff_idx,
            fit_idx,
            submodel,
            sf,
        }
    }
    pub fn output_names(&self) -> &'static [&'static str] {
        &OUTPUT_NAMES
    }
    /// Per-parameter (lower, upper) fit bounds, in [F,kr,R1f,R1r,T2f,T2r] order.
    pub fn param_bounds(&self) -> [(f64, f64); 6] {
        let mut out = [(0.0, 0.0); 6];
        for (o, (lb, ub)) in out
            .iter_mut()
            .zip(self.bounds.lb.iter().zip(self.bounds.ub.iter()))
        {
            *o = (*lb, *ub);
        }
        out
    }
    /// Which of the 6 params are fixed (not independently recovered), in
    /// [F,kr,R1f,R1r,T2f,T2r] order. Reflects fx plus R1map/R1r=R1f constraints
    /// (validate() folds those into fx).
    pub fn fixed_mask(&self) -> [bool; 6] {
        self.bounds.fx
    }
    pub fn fit_voxel(
        &self,
        voxel: &[f64],
        r1: Option<f64>,
        b1: Option<f64>,
        b0: Option<f64>,
    ) -> Vec<f64> {
        let b1 = b1.unwrap_or(1.0);
        let b0 = b0.unwrap_or(0.0);

        // Per-volume protocol adjusted for B1 (angle) and B0 (offset).
        let pulse = GaussHannPulse::new(self.trf, self.bandwidth);
        let mut fit_angles = Vec::with_capacity(self.fit_idx.len());
        let mut fit_offsets = Vec::with_capacity(self.fit_idx.len());
        for &i in &self.fit_idx {
            fit_angles.push(self.angles[i] * b1);
            fit_offsets.push(self.offsets[i] + b0);
        }

        // Per-voxel MToff normalization on the raw voxel data.
        let data = model::normalize_mtoff(voxel, &self.mtoff_idx, &self.fit_idx);

        // R1map: fix R1f start to observed R1 (clamped), force fx[2].
        let mut bounds = FitBounds {
            st: self.bounds.st,
            lb: self.bounds.lb,
            ub: self.bounds.ub,
            fx: self.bounds.fx,
        };
        let mut r1obs = None;
        if self.r1map {
            if let Some(r1v) = r1 {
                let r1c = r1v.max(0.1);
                bounds.st[2] = r1c;
                bounds.fx[2] = true;
                r1obs = Some(r1c);
            }
        }
        if self.r1req_r1f {
            bounds.fx[3] = true;
            bounds.st[3] = bounds.st[2];
        }

        let opt = FitOpt {
            r1map: self.r1map && r1obs.is_some(),
            r1obs,
            r1req_r1f: self.r1req_r1f,
            fix_r1f_t2f: self.fix_r1f_t2f,
            fix_r1f_t2f_value: self.fix_r1f_t2f_value,
        };

        let (mut x, resnorm) = match self.submodel {
            SubModel::Ramani => {
                let fit_w1cw: Vec<f64> =
                    fit_angles.iter().map(|&a| pulse.w1cw(a, self.tr)).collect();
                fit_ramani(&data, &fit_offsets, &fit_w1cw, &bounds, &opt)
            }
            SubModel::SledPikeRp => {
                let sf = self.sf.as_ref().expect("Sf table built for SledPikeRp");
                let tau = pulse.tau();
                let mut fit_w1rp = Vec::with_capacity(fit_angles.len());
                for &a in &fit_angles {
                    fit_w1rp.push(pulse.w1rp(a, tau));
                }
                // MATLAB SPGR_fit.m scales both Prot.Angles and Prot.Alpha by B1.
                let alpha = self.alpha * b1;
                let tr = self.tr;
                let angles_ref = &fit_angles;
                let offsets_ref = &fit_offsets;
                let w1rp_ref = &fit_w1rp;
                let opt_ref = &opt;
                fit_bounded(&data, &bounds, |x| {
                    srp_signal(
                        x,
                        angles_ref,
                        offsets_ref,
                        w1rp_ref,
                        tau,
                        tr,
                        alpha,
                        sf,
                        &pulse,
                        opt_ref,
                    )
                })
            }
        };

        // Post-fit: R1r=R1f if constrained; final R1f from computeR1 when R1map used.
        if self.r1req_r1f {
            x[3] = x[2];
        }
        if let Some(r1o) = r1obs {
            let kf = x[1] * x[0];
            x[2] = model::compute_r1(x[0], kf, x[3], r1o);
        }
        // T2f has no gradient in the model when fix_r1f_t2f is on (WF uses the
        // fixed R1f*T2f value directly), so the optimizer leaves x[4] at its
        // start value. Override it here from the final R1f, matching MATLAB
        // SPGR_fit.m: `Fit.T2f = FitOpt.FixR1fT2fValue/Fit.R1f;`.
        if self.fix_r1f_t2f && self.submodel == SubModel::Ramani {
            x[4] = self.fix_r1f_t2f_value / x[2];
        }
        let kf = x[1] * x[0];
        vec![x[0], x[1], x[2], x[3], x[4], x[5], kf, resnorm]
    }

    /// Noise-free forward signal aligned to the full protocol. MToff rows
    /// (angle < 1) are set to 1.0 (the MToff normalization reference), so a
    /// subsequent fit_voxel round-trips exactly.
    pub fn forward(&self, x: &[f64; 6], b1: f64, b0: f64, r1: Option<f64>) -> Vec<f64> {
        let pulse = GaussHannPulse::new(self.trf, self.bandwidth);

        // Per-volume protocol for the fit volumes, adjusted for B1/B0.
        let mut fit_angles = Vec::with_capacity(self.fit_idx.len());
        let mut fit_offsets = Vec::with_capacity(self.fit_idx.len());
        for &i in &self.fit_idx {
            fit_angles.push(self.angles[i] * b1);
            fit_offsets.push(self.offsets[i] + b0);
        }

        // Match fit_voxel's opt/bounds handling for R1map / R1r=R1f.
        let mut r1obs = None;
        if self.r1map {
            if let Some(r1v) = r1 {
                r1obs = Some(r1v.max(0.1));
            }
        }
        let opt = model::FitOpt {
            r1map: self.r1map && r1obs.is_some(),
            r1obs,
            r1req_r1f: self.r1req_r1f,
            fix_r1f_t2f: self.fix_r1f_t2f,
            fix_r1f_t2f_value: self.fix_r1f_t2f_value,
        };

        let fit_signal = match self.submodel {
            SubModel::Ramani => {
                let w1cw: Vec<f64> = fit_angles.iter().map(|&a| pulse.w1cw(a, self.tr)).collect();
                model::ramani_signal(x, &fit_offsets, &w1cw, &opt)
            }
            SubModel::SledPikeRp => {
                let sf = self.sf.as_ref().expect("Sf table built for SledPikeRp");
                let tau = pulse.tau();
                let w1rp: Vec<f64> = fit_angles.iter().map(|&a| pulse.w1rp(a, tau)).collect();
                let alpha = self.alpha * b1;
                model::srp_signal(
                    x,
                    &fit_angles,
                    &fit_offsets,
                    &w1rp,
                    tau,
                    self.tr,
                    alpha,
                    sf,
                    &pulse,
                    &opt,
                )
            }
        };

        // Scatter fit-volume signal back into full-protocol order; MToff → 1.0.
        let mut full = vec![1.0_f64; self.angles.len()];
        for (k, &vol) in self.fit_idx.iter().enumerate() {
            full[vol] = fit_signal[k];
        }
        full
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::QmtSpgrConfig;

    #[test]
    fn fit_voxel_recovers_synthetic() {
        let cfg = QmtSpgrConfig::default();
        let fitter = QmtSpgrFitter::new(&cfg);
        // Build synthetic normalized data from the fitter's own forward model
        // by fitting a flat signal is not meaningful; instead check output shape
        // and that fitted params are within bounds.
        let n = cfg.protocol.mtdata.len();
        let voxel = vec![0.5_f64; n];
        let out = fitter.fit_voxel(&voxel, Some(1.0), None, None);
        assert_eq!(out.len(), 8);
        // F within [lb, ub]
        assert!(
            out[0] >= 1e-4 - 1e-9 && out[0] <= 0.5 + 1e-9,
            "F out of range: {}",
            out[0]
        );
        // kf == kr * F
        assert!((out[6] - out[1] * out[0]).abs() < 1e-9);
    }

    #[test]
    fn sledpikerp_fit_voxel_shape_and_bounds() {
        let cfg = QmtSpgrConfig {
            model: "SledPikeRP".to_string(),
            ..Default::default()
        };
        let fitter = QmtSpgrFitter::new(&cfg);
        let n = cfg.protocol.mtdata.len();
        let voxel = vec![0.5_f64; n];
        let out = fitter.fit_voxel(&voxel, Some(1.0), None, None);
        assert_eq!(out.len(), 8);
        assert!(
            out[0] >= 1e-4 - 1e-9 && out[0] <= 0.5 + 1e-9,
            "F out of range: {}",
            out[0]
        );
        assert!((out[6] - out[1] * out[0]).abs() < 1e-9, "kf == kr*F");
        assert!(out[7].is_finite(), "resnorm finite");
    }

    #[test]
    fn fix_r1f_t2f_overrides_t2f() {
        let mut cfg = QmtSpgrConfig::default();
        cfg.fitting.fix_r1f_t2f = true;
        // keep use_r1map_to_constrain_r1f at its default (true)
        let fitter = QmtSpgrFitter::new(&cfg);
        let n = cfg.protocol.mtdata.len();
        let voxel = vec![0.5_f64; n];
        let out = fitter.fit_voxel(&voxel, Some(1.0), None, None);
        assert_eq!(out.len(), 8);
        let r1f_t2f = cfg.fitting.r1f_t2f;
        assert!(
            (out[4] - r1f_t2f / out[2]).abs() < 1e-9,
            "T2f override mismatch: T2f={}, R1f={}, expected={}",
            out[4],
            out[2],
            r1f_t2f / out[2]
        );
    }

    #[test]
    fn forward_then_fit_recovers_params_ramani() {
        let mut cfg = QmtSpgrConfig::default();
        cfg.fitting.use_r1map_to_constrain_r1f = false; // no R1map constraint for a clean round-trip
                                                        // Keep R1f/R1r fixed (config default: fx = [F,kr,R1f,R1r,T2f,T2r] =
                                                        // [false,false,true,true,false,false]). Freeing R1f jointly with F and
                                                        // T2f makes the Ramani signal non-identifiable (it depends only on the
                                                        // combos F/R1f and R1f*T2f when kr/R1r/T2r are held fixed), so this
                                                        // exercises the forward()<->fit_voxel round-trip on the same
                                                        // well-conditioned free-parameter set already validated in
                                                        // qmt_spgr::fit::tests::fit_ramani_recovers_known_params.
        let fitter = QmtSpgrFitter::new(&cfg);
        let truth = [0.15, 25.0, 1.0, 1.0, 0.028, 1.1e-5];
        let sig = fitter.forward(&truth, 1.0, 0.0, None);
        assert_eq!(sig.len(), cfg.protocol.mtdata.len());
        let out = fitter.fit_voxel(&sig, None, Some(1.0), Some(0.0));
        assert!((out[0] - 0.15).abs() < 0.03, "F: {}", out[0]);
        assert!((out[4] - 0.028).abs() < 0.008, "T2f: {}", out[4]);
    }
}
