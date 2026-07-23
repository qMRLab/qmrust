//! MTsat adapter onto the core `Model` trait.
//!
//! MTsat is a `Named` three-volume model — `MTw`, `PDw`, `T1w` (the BIDS `MTS`
//! set) plus an optional `B1map` aux — combined by the closed-form Helms
//! computation. It is not an iterative fit. Each role carries a nominal flip
//! angle and repetition time: the non-BIDS path reads them from `--config`, the
//! BIDS path folds them from each role's sidecar in `ingest_protocol`.

use crate::core::model::{
    Aux, BidsMap, BidsSpec, BidsVolume, EntityRole, InputSpec, Measurement, MeasurementKind, Model,
    ProtoParam, Protocol, Scope, Source,
};
use crate::models::mt_sat::config::MtSatConfig;
use crate::models::mt_sat::fit::{self, Acq};
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct MtSatModel {
    acq: Acq,
    b1_correction_factor: f64,
    export_mtr: bool,
    /// FA (degrees) / TR (s) per role, retained for `bids_volume` sidecars.
    cfg: MtSatConfig,
    output_names: Vec<String>,
}

/// Volume roles, in acquisition order — index `i` maps to `bids_volume(i)`, to
/// the `MTS` grouping's `named_set` role of the same name, and (BIDS path) to
/// the reordered per-role protocol row `i`.
const ROLES: &[&str] = &["MTw", "PDw", "T1w"];
const MTS_ENTITIES: &[EntityRole] = &[EntityRole::Flip, EntityRole::Mt];

fn deg2rad(d: f64) -> f64 {
    d * std::f64::consts::PI / 180.0
}

impl MtSatModel {
    pub fn new(cfg: MtSatConfig) -> Self {
        let acq = Acq {
            alpha_mt: deg2rad(cfg.mtw.flip_angle),
            tr_mt: cfg.mtw.repetition_time,
            alpha_pd: deg2rad(cfg.pdw.flip_angle),
            tr_pd: cfg.pdw.repetition_time,
            alpha_t1: deg2rad(cfg.t1w.flip_angle),
            tr_t1: cfg.t1w.repetition_time,
        };
        let mut output_names = vec!["MTSAT".to_string(), "T1".to_string()];
        if cfg.export_mtr {
            output_names.push("MTR".to_string());
        }
        Self {
            acq,
            b1_correction_factor: cfg.b1_correction_factor,
            export_mtr: cfg.export_mtr,
            cfg,
            output_names,
        }
    }
}

impl Model for MtSatModel {
    fn param_names(&self) -> Vec<&'static str> {
        // Forward inputs: apparent amplitude, T1 (s), and MT saturation (%).
        vec!["A", "T1", "MTSAT"]
    }
    fn output_names(&self) -> Vec<String> {
        self.output_names.clone()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        vec![(f64::NEG_INFINITY, f64::INFINITY); 3]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false; 3]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        // B1 transmit map, used-if-present to correct MTsat (and R1).
        vec![InputSpec {
            name: "B1map",
            required: false,
            bids: Some(BidsMap {
                suffix: "TB1map",
                entity: None,
            }),
        }]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Named { roles: ROLES }
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let (mtw, pdw, t1w) = fit::forward_signals(&self.acq, params[0], params[1], params[2]);
        Measurement::Named(BTreeMap::from([("MTw", mtw), ("PDw", pdw), ("T1w", t1w)]))
    }
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64> {
        let mtw = m.role("MTw").expect("Named measurement has no MTw volume");
        let pdw = m.role("PDw").expect("Named measurement has no PDw volume");
        let t1w = m.role("T1w").expect("Named measurement has no T1w volume");
        let b1 = aux.get("B1map");
        let (mtsat, r1) = fit::mtsat(&self.acq, mtw, pdw, t1w, b1, self.b1_correction_factor);
        let mut out = vec![mtsat, 1.0 / r1];
        if self.export_mtr {
            out.push(fit::mtr(pdw, mtw));
        }
        out
    }
    fn n_volumes(&self) -> usize {
        ROLES.len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        // Index follows ROLES; the MTS set is distinguished by flip + mt.
        let (w, flip, mt, mt_state) = match ROLES[index] {
            "MTw" => (self.cfg.mtw, "1", "on", true),
            "PDw" => (self.cfg.pdw, "1", "off", false),
            "T1w" => (self.cfg.t1w, "2", "off", false),
            other => panic!("mt_sat has no volume role '{other}'"),
        };
        BidsVolume {
            entities: vec![("flip", flip.to_string()), ("mt", mt.to_string())],
            sidecar: BTreeMap::from([
                ("FlipAngle".to_string(), json!(w.flip_angle)),
                (
                    "RepetitionTimeExcitation".to_string(),
                    json!(w.repetition_time),
                ),
                ("MTState".to_string(), json!(mt_state)),
            ]),
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "MTS",
            entities: MTS_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![
            ProtoParam {
                name: "FlipAngle",
                source: Source::Field("FlipAngle"),
                scope: Scope::PerVolume,
            },
            ProtoParam {
                name: "RepetitionTimeExcitation",
                source: Source::Field("RepetitionTimeExcitation"),
                scope: Scope::PerVolume,
            },
        ]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        let mut outs = vec![("MTSAT", "MTsat", "%"), ("T1", "T1map", "s")];
        if self.export_mtr {
            outs.push(("MTR", "MTRmap", "%"));
        }
        outs
    }
}

impl crate::core::model::ModelConfig for MtSatConfig {
    const NAME: &'static str = "mt_sat";
    const SUBKEY: Option<&'static str> = None;

    fn validate_options(&mut self) -> Result<()> {
        MtSatConfig::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        // The shell orders a named set's per-role protocol to ROLES, so
        // `proto.volumes[i]` carries role `ROLES[i]`'s FlipAngle/TR sidecar.
        if proto.volumes.is_empty() {
            return Ok(());
        }
        let weightings = [&mut self.mtw, &mut self.pdw, &mut self.t1w];
        for (w, vol) in weightings.into_iter().zip(&proto.volumes) {
            if let Some(&fa) = vol.get("FlipAngle") {
                w.flip_angle = fa;
            }
            if let Some(&tr) = vol.get("RepetitionTimeExcitation") {
                w.repetition_time = tr;
            }
        }
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        MtSatConfig::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(MtSatModel::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`](crate::core::model::describe_model)).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<MtSatConfig>(v)
}

/// Registry builder (see [`build_model`](crate::core::model::build_model)).
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<MtSatConfig>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)).
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<MtSatConfig>(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mtsat_value() -> serde_yaml::Value {
        serde_yaml::from_str(
            "model: mt_sat\nmtw: {flip_angle: 6, repetition_time: 0.028}\npdw: {flip_angle: 6, repetition_time: 0.028}\nt1w: {flip_angle: 20, repetition_time: 0.018}\n",
        )
        .unwrap()
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&mtsat_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["A", "T1", "MTSAT"]);
        assert_eq!(m.output_names(), vec!["MTSAT", "T1", "MTR"]); // export default on
        assert_eq!(m.n_volumes(), 3);
        // forward known (A, T1, MTsat), recover T1 and MTsat exactly.
        let sig = m.forward(&[1000.0, 0.9, 1.5], &Aux::new());
        let fitted = m.fit(&sig, &Aux::new());
        assert!((fitted[0] - 1.5).abs() < 1e-6, "MTSAT: {}", fitted[0]);
        assert!((fitted[1] - 0.9).abs() < 1e-6, "T1: {}", fitted[1]);
    }

    #[test]
    fn fit_reads_by_role_not_position() {
        let m = build(&mtsat_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[1000.0, 0.9, 1.5], &Aux::new());
        let Measurement::Named(map) = &sig else {
            unreachable!()
        };
        // Rebuild the map in a different insertion order; result must not change.
        let reordered = Measurement::Named(BTreeMap::from([
            ("T1w", map["T1w"]),
            ("MTw", map["MTw"]),
            ("PDw", map["PDw"]),
        ]));
        assert_eq!(m.fit(&sig, &Aux::new()), m.fit(&reordered, &Aux::new()));
    }

    #[test]
    fn bids_folds_per_role_flip_and_tr_from_protocol() {
        // Per-role protocol in ROLES order (MTw, PDw, T1w), as the shell hands it.
        let proto = Protocol {
            volumes: vec![
                BTreeMap::from([
                    ("FlipAngle".to_string(), 6.0),
                    ("RepetitionTimeExcitation".to_string(), 0.028),
                ]),
                BTreeMap::from([
                    ("FlipAngle".to_string(), 6.0),
                    ("RepetitionTimeExcitation".to_string(), 0.028),
                ]),
                BTreeMap::from([
                    ("FlipAngle".to_string(), 20.0),
                    ("RepetitionTimeExcitation".to_string(), 0.018),
                ]),
            ],
            global: BTreeMap::new(),
        };
        // Config carries no acquisition; the protocol supplies it.
        let v: serde_yaml::Value = serde_yaml::from_str("model: mt_sat\n").unwrap();
        let m = build(&v, &proto).unwrap();
        // The T1w volume's sidecar must echo the folded 20°/18 ms.
        let t1w = m.bids_volume(2);
        assert_eq!(
            t1w.entities,
            vec![("flip", "2".into()), ("mt", "off".into())]
        );
        assert_eq!(t1w.sidecar["FlipAngle"], json!(20.0));
        assert_eq!(t1w.sidecar["RepetitionTimeExcitation"], json!(0.018));
    }

    #[test]
    fn declares_bids_mts_and_b1_aux() {
        let m = build(&mtsat_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "MTS");
        let inputs = m.required_inputs();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "B1map");
        assert!(!inputs[0].required);
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&mtsat_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _s, _u) in m.bids_outputs() {
            assert!(names.iter().any(|n| n == out), "{out} not in {names:?}");
        }
    }

    #[test]
    fn export_mtr_off_drops_mtr_output() {
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: mt_sat\nexport_mtr: false\nmtw: {flip_angle: 6, repetition_time: 0.028}\npdw: {flip_angle: 6, repetition_time: 0.030}\nt1w: {flip_angle: 20, repetition_time: 0.018}\n",
        )
        .unwrap();
        let m = build(&v, &Protocol::default()).unwrap();
        assert_eq!(m.output_names(), vec!["MTSAT", "T1"]);
        assert_eq!(
            m.fit(&m.forward(&[1000.0, 0.9, 1.5], &Aux::new()), &Aux::new())
                .len(),
            2
        );
    }
}
