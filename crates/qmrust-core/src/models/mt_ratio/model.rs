//! MTR adapter onto the core `Model` trait.
//!
//! MTR is a `Named` two-volume model: an `MTon` (MT-weighted) and an `MToff`
//! (reference) volume, combined by a closed-form ratio — there is no
//! acquisition protocol and no iterative fit. `forward` picks a reference
//! `MToff` level of 1 so the sim round-trip recovers a known MTR exactly.

use crate::core::model::{
    Aux, BidsSpec, BidsVolume, EntityRole, InputSpec, Measurement, MeasurementKind, Model, Protocol,
};
use crate::models::mt_ratio::config::MtRatioConfig;
use crate::models::mt_ratio::fit;
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;

pub struct MtRatioModel;

/// Volume roles, in acquisition order. Index `i` maps to `bids_volume(i)` and
/// to the BIDS grouping's `named_set` role of the same name.
const ROLES: &[&str] = &["MTon", "MToff"];
const MTR_ENTITIES: &[EntityRole] = &[EntityRole::Mt];

impl MtRatioModel {
    pub fn new(_cfg: MtRatioConfig) -> Self {
        Self
    }
}

impl Model for MtRatioModel {
    fn param_names(&self) -> Vec<&'static str> {
        vec!["MTR"]
    }
    fn output_names(&self) -> Vec<String> {
        vec!["MTR".to_string()]
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        // MTR is computed, not fitted; report it unbounded.
        vec![(f64::NEG_INFINITY, f64::INFINITY)]
    }
    fn fixed_mask(&self) -> Vec<bool> {
        vec![false]
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        vec![]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Named { roles: ROLES }
    }
    fn forward(&self, params: &[f64], _aux: &Aux) -> Measurement {
        // Reference MToff level of 1; MTon follows from the target ratio.
        let mton = fit::forward_mton(params[0]);
        Measurement::Named(BTreeMap::from([("MToff", 1.0), ("MTon", mton)]))
    }
    fn fit(&self, m: &Measurement, _aux: &Aux) -> Vec<f64> {
        let mt_on = m
            .role("MTon")
            .expect("Named measurement has no MTon volume");
        let mt_off = m
            .role("MToff")
            .expect("Named measurement has no MToff volume");
        vec![fit::mtr(mt_off, mt_on)]
    }
    fn n_volumes(&self) -> usize {
        ROLES.len()
    }
    fn bids_volume(&self, index: usize) -> BidsVolume {
        // Index follows ROLES: 0 -> MTon (mt-on), 1 -> MToff (mt-off). The
        // sidecar records the MT-pulse state; MTR needs no other acquisition
        // metadata.
        let (mt_value, mt_state) = match ROLES[index] {
            "MTon" => ("on", true),
            "MToff" => ("off", false),
            other => panic!("mt_ratio has no volume role '{other}'"),
        };
        BidsVolume {
            entities: vec![("mt", mt_value.to_string())],
            sidecar: BTreeMap::from([("MTState".to_string(), json!(mt_state))]),
        }
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "MTR",
            entities: MTR_ENTITIES,
        })
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        vec![("MTR", "MTRmap", "%")]
    }
}

impl crate::core::model::ModelConfig for MtRatioConfig {
    const NAME: &'static str = "mt_ratio";
    const SUBKEY: Option<&'static str> = None;

    fn validate_options(&mut self) -> Result<()> {
        MtRatioConfig::validate_options(self)
    }

    fn into_model(self) -> Box<dyn Model> {
        Box::new(MtRatioModel::new(self))
    }
}

/// Structural interrogation entry point (see [`describe_model`](crate::core::model::describe_model)).
pub fn describe(v: &serde_yaml::Value) -> Result<Box<dyn Model>> {
    crate::core::model::describe_model::<MtRatioConfig>(v)
}

/// Registry builder (see [`build_model`](crate::core::model::build_model)).
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    crate::core::model::build_model::<MtRatioConfig>(v, proto)
}

/// Registry dumper (see [`dump_model`](crate::core::model::dump_model)).
pub fn dump(v: &serde_yaml::Value) -> Result<String> {
    crate::core::model::dump_model::<MtRatioConfig>(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mtr_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: mt_ratio\n").unwrap()
    }

    #[test]
    fn build_and_roundtrip_via_trait() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        assert_eq!(m.param_names(), vec!["MTR"]);
        assert_eq!(m.output_names(), vec!["MTR".to_string()]);
        assert_eq!(m.n_volumes(), 2);
        // forward a known MTR, then recover it exactly.
        let sig = m.forward(&[37.5], &Aux::new());
        let fitted = m.fit(&sig, &Aux::new());
        assert!((fitted[0] - 37.5).abs() < 1e-9, "MTR: {}", fitted[0]);
    }

    #[test]
    fn fit_reads_by_role_not_position() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        // Named is role-keyed: the map order cannot change the result.
        let a = m.fit(
            &Measurement::Named(BTreeMap::from([("MTon", 150.0), ("MToff", 200.0)])),
            &Aux::new(),
        );
        let b = m.fit(
            &Measurement::Named(BTreeMap::from([("MToff", 200.0), ("MTon", 150.0)])),
            &Aux::new(),
        );
        assert_eq!(a[0], b[0]);
        assert!((a[0] - 25.0).abs() < 1e-12, "MTR: {}", a[0]);
    }

    #[test]
    fn declares_bids_mtr() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "MTR");
    }

    #[test]
    fn bids_volume_maps_roles_to_mt_entity() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        let on = m.bids_volume(0);
        assert_eq!(on.entities, vec![("mt", "on".to_string())]);
        assert_eq!(on.sidecar["MTState"], json!(true));
        let off = m.bids_volume(1);
        assert_eq!(off.entities, vec![("mt", "off".to_string())]);
        assert_eq!(off.sidecar["MTState"], json!(false));
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _suffix, _units) in m.bids_outputs() {
            assert!(
                names.iter().any(|n| n == out),
                "bids_outputs references '{out}', not in output_names {names:?}"
            );
        }
    }

    #[test]
    fn declares_no_protocol_schema() {
        let m = build(&mtr_value(), &Protocol::default()).unwrap();
        assert!(m.protocol_schema().is_empty());
    }
}
