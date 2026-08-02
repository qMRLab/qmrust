//! b1_afi adapter onto the core `Model` trait.
//!
//! A `Series` of exactly two spoiled gradient-echo volumes acquired at
//! interleaved excitation repetition times, combined by the closed-form AFI
//! ratio — no iterative fit. BIDS indexes the pair by the `acq` entity, whose
//! values name the shorter and longer repetition time (`tr1`/`tr2`).

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, EntityRole, FitStrategy, InputSpec, Measurement, MeasurementKind,
    Model, ProtoParam, Protocol, Scope, SeriesAxis, Source,
};
use crate::models::b1_afi::config::B1AfiConfig;
use crate::models::b1_afi::fit::{B1AfiFitter, B1_BOUNDS, T1_BOUNDS};
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct B1AfiModel {
    fitter: B1AfiFitter,
}

const B1AFI_ENTITIES: &[EntityRole] = &[EntityRole::Other("acq")];

/// The acquisition axis: one volume per RepetitionTimeExcitation.
const AXIS: SeriesAxis = SeriesAxis::new("RepetitionTimeExcitation");

impl B1AfiModel {
    pub fn new(cfg: B1AfiConfig) -> Self {
        Self {
            fitter: B1AfiFitter::new(&cfg),
        }
    }
}

impl Model for B1AfiModel {
    fn param_names(&self) -> Vec<&'static str> {
        B1AfiFitter::param_names().to_vec()
    }
    fn output_names(&self) -> Vec<String> {
        B1AfiFitter::output_names()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        vec![B1_BOUNDS, T1_BOUNDS]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        // T1 is a ground-truth parameter of the forward signal that the closed
        // form never recovers — it assumes TR << T1 and divides it out.
        vec![false, true]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        // B1+ is what this model measures; it consumes no auxiliary map.
        vec![]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: AXIS.rows(self.fitter.repetition_times()),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        let values = self.fitter.forward(params[0], params[1]);
        AXIS.samples(self.fitter.repetition_times(), values)
    }
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        // By identity, never by position: swapping the two volumes would
        // otherwise invert the ratio.
        self.fitter
            .fit_voxel(&AXIS.assemble(m, self.fitter.repetition_times()))
    }
    fn n_volumes(&self) -> usize {
        self.fitter.repetition_times().len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        // The BIDS spec names the shorter repetition time `tr1` and the longer
        // `tr2`, so the label follows the value, not the acquisition order.
        let tr = self.fitter.repetition_times()[index];
        let label = if tr == self.fitter.tr1() {
            "tr1"
        } else {
            "tr2"
        };
        BidsVolume {
            entities: vec![("acq", label.to_string())],
            sidecar: BTreeMap::from([
                ("RepetitionTimeExcitation".to_string(), json!(tr)),
                ("FlipAngle".to_string(), json!(self.fitter.flip_angle())),
            ]),
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "TB1AFI",
            entities: B1AFI_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        // The repetition time is the acquisition axis, so it alone identifies a
        // volume: a `Series`' per-volume protocol rows *are* its volume
        // identities (`engine::build_volume_ids`), and `forward` tags its
        // samples the same way. The nominal flip angle is one value for the
        // pair, so it is `Global` — as a per-volume param it would join the
        // identity and no forward sample would match its volume.
        vec![
            ProtoParam {
                name: "RepetitionTimeExcitation",
                source: Source::Field("RepetitionTimeExcitation"),
                scope: Scope::PerVolume,
                required: true,
            },
            ProtoParam {
                name: "FlipAngle",
                source: Source::Field("FlipAngle"),
                scope: Scope::Global,
                required: true,
            },
        ]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // Dimensionless: the achieved flip angle as a fraction of the nominal
        // one, which is the scaling every `B1map` aux consumer expects.
        vec![("B1", "TB1map", "")]
    }
}

impl crate::core::model::ModelConfig for B1AfiConfig {
    const NAME: &'static str = "b1_afi";
    const SUBKEY: Option<&'static str> = None;
    const PROTOCOL_KEYS: &'static [&'static str] = &["repetition_times", "flip_angle"];

    fn validate_options(&mut self) -> Result<()> {
        B1AfiConfig::validate_options(self)
    }

    fn ingest_protocol(&mut self, proto: &Protocol) -> Result<()> {
        let times = AXIS.ingest(proto);
        if !times.is_empty() {
            self.repetition_times = times;
        }
        // Global scope, so this is resolved once for the collection rather than
        // per volume — read it independently of the per-volume rows.
        if let Some(&fa) = proto.global.get("FlipAngle") {
            self.flip_angle = Some(fa);
        }
        Ok(())
    }

    fn validate_protocol(&mut self) -> Result<()> {
        B1AfiConfig::validate_protocol(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(B1AfiModel::new(self))
    }
}

crate::model_entry_points!(B1AfiConfig);

#[cfg(test)]
mod tests {
    use super::*;

    fn b1_afi_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: b1_afi\nrepetition_times: [0.02, 0.1]\nflip_angle: 60\n")
            .unwrap()
    }

    /// A resolved protocol as the shell hands it over: one row per volume
    /// carrying the identifying axis, plus the collection-wide flip angle.
    fn resolved_proto(times: &[f64], flip_angle: f64) -> Protocol {
        Protocol {
            volumes: times
                .iter()
                .map(|&tr| BTreeMap::from([("RepetitionTimeExcitation".to_string(), tr)]))
                .collect(),
            global: BTreeMap::from([("FlipAngle".to_string(), flip_angle)]),
        }
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&b1_afi_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["B1", "T1"]);
        assert_eq!(m.output_names(), vec!["B1".to_string()]);
        assert_eq!(m.n_volumes(), 2);
        // The estimator assumes TR << T1, so recovery carries the method's own
        // bias rather than being exact (see the fitter's own tests).
        let sig = m.forward(&[0.85, 0.9], &Aux::new());
        let fitted = m.fit(&sig, &Aux::new());
        assert!((fitted[0] - 0.85).abs() / 0.85 < 0.01, "B1: {}", fitted[0]);
    }

    #[test]
    fn t1_is_a_forward_parameter_the_fit_never_recovers() {
        let m = build(&b1_afi_value(), &Protocol::default()).unwrap();
        assert_eq!(m.fixed_mask(), vec![false, true]);
        // It is real, not decorative: the forward signal moves with it.
        let a = m.forward(&[1.0, 0.5], &Aux::new());
        let b = m.forward(&[1.0, 2.0], &Aux::new());
        assert!((a.series()[0].value - b.series()[0].value).abs() > 1e-6);
        // ...and it is absent from the fitted outputs.
        assert_eq!(m.fit(&a, &Aux::new()).len(), 1);
    }

    #[test]
    fn bids_folds_the_acquisition_from_protocol_and_labels_by_repetition_time() {
        let proto = resolved_proto(&[0.03, 0.15], 55.0);
        // Config carries no acquisition; the sidecars supply it.
        let v: serde_yaml::Value = serde_yaml::from_str("model: b1_afi\n").unwrap();
        let m = build(&v, &proto).unwrap();
        assert_eq!(m.n_volumes(), 2);
        let short = m.bids_volume(0);
        assert_eq!(short.entities, vec![("acq", "tr1".to_string())]);
        assert_eq!(short.sidecar["RepetitionTimeExcitation"], json!(0.03));
        assert_eq!(short.sidecar["FlipAngle"], json!(55.0));
        assert_eq!(m.bids_volume(1).entities, vec![("acq", "tr2".to_string())]);
    }

    #[test]
    fn the_tr_labels_follow_the_value_not_the_acquisition_order() {
        // A series listed longest-first must still label the shorter time
        // `tr1`, as the BIDS spec defines it.
        let proto = resolved_proto(&[0.15, 0.03], 55.0);
        let v: serde_yaml::Value = serde_yaml::from_str("model: b1_afi\n").unwrap();
        let m = build(&v, &proto).unwrap();
        assert_eq!(m.bids_volume(0).entities, vec![("acq", "tr2".to_string())]);
        assert_eq!(m.bids_volume(1).entities, vec![("acq", "tr1".to_string())]);
    }
}
