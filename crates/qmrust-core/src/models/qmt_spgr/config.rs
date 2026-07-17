//! qMT-SPGR config (moved out of the monolithic top-level config).

use serde::{Deserialize, Serialize};

fn qmt_default_mtdata() -> Vec<[f64; 2]> {
    vec![
        [142.0, 443.0],
        [426.0, 443.0],
        [142.0, 1088.0],
        [426.0, 1088.0],
        [142.0, 2732.0],
        [426.0, 2732.0],
        [142.0, 6862.0],
        [426.0, 6862.0],
        [142.0, 17235.0],
        [426.0, 17235.0],
    ]
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QmtTiming {
    #[serde(default = "def_tmt")]
    pub tmt: f64,
    // ts, tp, tr are not used by the Ramani physics model but are retained
    // as legitimate protocol/config fields for future sub-models (e.g.
    // Yarnykh/SledPike) that need saturation/spoiler/read-pulse timings.
    #[allow(dead_code)]
    #[serde(default = "def_ts")]
    pub ts: f64,
    #[allow(dead_code)]
    #[serde(default = "def_tp")]
    pub tp: f64,
    #[allow(dead_code)]
    #[serde(default = "def_tr")]
    pub tr: f64,
    #[serde(default = "def_trep", rename = "TR")]
    pub trep: f64,
}
fn def_tmt() -> f64 {
    0.0102
}
fn def_ts() -> f64 {
    0.0030
}
fn def_tp() -> f64 {
    0.0018
}
fn def_tr() -> f64 {
    0.0100
}
fn def_trep() -> f64 {
    0.0250
}
impl Default for QmtTiming {
    fn default() -> Self {
        Self {
            tmt: def_tmt(),
            ts: def_ts(),
            tp: def_tp(),
            tr: def_tr(),
            trep: def_trep(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QmtProtocol {
    #[serde(default = "qmt_default_mtdata")]
    pub mtdata: Vec<[f64; 2]>,
    #[serde(default)]
    pub timing: QmtTiming,
}
impl Default for QmtProtocol {
    fn default() -> Self {
        Self {
            mtdata: qmt_default_mtdata(),
            timing: QmtTiming::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QmtPulse {
    #[serde(default = "def_shape")]
    pub shape: String,
    #[serde(default = "def_bw")]
    pub bandwidth: f64,
    // Not used by the Ramani physics model (which computes saturation from
    // the continuous-wave-equivalent power), but retained for future
    // sub-models (e.g. Yarnykh/SledPike) that model discrete pulse trains.
    #[allow(dead_code)]
    #[serde(default = "def_npulse")]
    pub n_pulses: usize,
}
fn def_shape() -> String {
    "gausshann".to_string()
}
fn def_bw() -> f64 {
    200.0
}
fn def_npulse() -> usize {
    600
}
impl Default for QmtPulse {
    fn default() -> Self {
        Self {
            shape: def_shape(),
            bandwidth: def_bw(),
            n_pulses: def_npulse(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QmtFitting {
    #[serde(default = "def_st")]
    pub st: [f64; 6],
    #[serde(default = "def_lb")]
    pub lb: [f64; 6],
    #[serde(default = "def_ub")]
    pub ub: [f64; 6],
    #[serde(default = "def_fx")]
    pub fx: [bool; 6],
    #[serde(default = "def_true")]
    pub use_r1map_to_constrain_r1f: bool,
    #[serde(default)]
    pub fix_r1r_eq_r1f: bool,
    #[serde(default)]
    pub fix_r1f_t2f: bool,
    #[serde(default = "def_r1ft2f")]
    pub r1f_t2f: f64,
}
fn def_st() -> [f64; 6] {
    [0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5]
}
fn def_lb() -> [f64; 6] {
    [1e-4, 1e-4, 0.05, 0.05, 0.003, 3e-6]
}
fn def_ub() -> [f64; 6] {
    [0.5, 100.0, 5.0, 5.0, 0.5, 5e-5]
}
fn def_fx() -> [bool; 6] {
    [false, false, true, true, false, false]
}
fn def_true() -> bool {
    true
}
fn def_r1ft2f() -> f64 {
    0.055
}
impl Default for QmtFitting {
    fn default() -> Self {
        Self {
            st: def_st(),
            lb: def_lb(),
            ub: def_ub(),
            fx: def_fx(),
            use_r1map_to_constrain_r1f: true,
            fix_r1r_eq_r1f: false,
            fix_r1f_t2f: false,
            r1f_t2f: def_r1ft2f(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QmtSpgrConfig {
    #[serde(default)]
    pub protocol: QmtProtocol,
    #[serde(default)]
    pub pulse: QmtPulse,
    #[serde(default = "def_lineshape")]
    pub lineshape: String,
    #[serde(default = "def_qmt_model")]
    pub model: String,
    #[serde(default = "def_alpha")]
    pub read_pulse_alpha: f64,
    #[serde(default)]
    pub fitting: QmtFitting,
}
fn def_lineshape() -> String {
    "SuperLorentzian".to_string()
}
fn def_qmt_model() -> String {
    "Ramani".to_string()
}
fn def_alpha() -> f64 {
    7.0
}
impl Default for QmtSpgrConfig {
    fn default() -> Self {
        Self {
            protocol: QmtProtocol::default(),
            pulse: QmtPulse::default(),
            lineshape: def_lineshape(),
            model: def_qmt_model(),
            read_pulse_alpha: def_alpha(),
            fitting: QmtFitting::default(),
        }
    }
}

use anyhow::{bail, Result};

impl QmtSpgrConfig {
    pub fn validate(&mut self) -> Result<()> {
        if self.pulse.shape != "gausshann" {
            bail!(
                "Only 'gausshann' pulse shape is supported, got '{}'",
                self.pulse.shape
            );
        }
        if self.lineshape != "SuperLorentzian" {
            bail!(
                "Only 'SuperLorentzian' lineshape is supported, got '{}'",
                self.lineshape
            );
        }
        if self.model != "Ramani" && self.model != "SledPikeRP" {
            bail!(
                "Only 'Ramani' and 'SledPikeRP' sub-models are supported, got '{}'",
                self.model
            );
        }
        if self.protocol.mtdata.is_empty() {
            bail!("qmt_spgr protocol.mtdata must have at least one row");
        }
        for i in 0..6 {
            if self.fitting.lb[i] >= self.fitting.ub[i] {
                bail!("fitting.lb[{}] must be < fitting.ub[{}]", i, i);
            }
        }
        if self.fitting.use_r1map_to_constrain_r1f {
            self.fitting.fx[2] = true;
        }
        if self.fitting.fix_r1r_eq_r1f {
            self.fitting.fx[3] = true;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Migrated from the old top-level `config::qmt_tests` (Task 9): these
    // assertions used to run against `Config.qmt_spgr`; now that `Config` no
    // longer owns a typed qmt_spgr field, they exercise `QmtSpgrConfig`
    // directly.

    #[test]
    fn minimal_qmt_config_uses_defaults() {
        let yaml = "model: qmt_spgr\n";
        let v: serde_yaml::Value = serde_yaml::from_str(yaml).unwrap();
        let q: QmtSpgrConfig = match v.get("qmt_spgr") {
            Some(sub) => serde_yaml::from_value(sub.clone()).unwrap(),
            None => QmtSpgrConfig::default(),
        };
        assert_eq!(q.protocol.mtdata.len(), 10);
        assert_eq!(q.protocol.mtdata[0], [142.0, 443.0]);
        assert!((q.protocol.timing.trep - 0.025).abs() < 1e-12);
        assert_eq!(q.pulse.bandwidth as i32, 200);
        assert_eq!(q.model, "Ramani");
        assert_eq!(q.fitting.fx, [false, false, true, true, false, false]);
        assert!(q.fitting.use_r1map_to_constrain_r1f);
    }

    #[test]
    fn validation_forces_r1f_fixed_when_r1map_on() {
        let mut q = QmtSpgrConfig::default();
        q.validate().unwrap();
        assert!(q.fitting.fx[2], "R1f must be fixed");
    }

    #[test]
    fn accepts_sledpikerp_model() {
        let yaml = "model: SledPikeRP\n";
        let mut q: QmtSpgrConfig = serde_yaml::from_str(yaml).unwrap();
        q.validate().unwrap();
        assert_eq!(q.model, "SledPikeRP");
    }
}
