//! qMT-SPGR adapter onto the core `Model` trait.

use crate::core::model::{
    validate_against_protocol, Aux, BidsMap, BidsSpec, EntityRole, FitStrategy, InputSpec,
    Measurement, MeasurementKind, Model, ProtoParam, Protocol, Sample, Scope, Source,
};
use crate::models::qmt_spgr::config::QmtSpgrConfig;
use crate::models::qmt_spgr::QmtSpgrFitter;
use anyhow::{Context, Result};
use std::collections::BTreeMap;

pub struct QmtModel {
    fitter: QmtSpgrFitter,
    /// Per-volume saturation protocol rows `[Angle (deg), Offset (Hz)]`, in the
    /// order the fitter consumes them (mtdata order, incl. MToff rows).
    protocol: Vec<[f64; 2]>,
}

impl QmtModel {
    pub fn new(cfg: &QmtSpgrConfig) -> Self {
        Self {
            fitter: QmtSpgrFitter::new(cfg),
            protocol: cfg.protocol.mtdata.clone(),
        }
    }
}

/// One identity row per `mtdata` volume, keyed by the two acquisition axes:
/// the saturation pulse flip angle (deg) and its offset frequency (Hz).
fn qmt_rows(protocol: &[[f64; 2]]) -> Vec<BTreeMap<String, f64>> {
    protocol
        .iter()
        .map(|row| {
            BTreeMap::from([
                ("Angle".to_string(), row[0]),
                ("Offset".to_string(), row[1]),
            ])
        })
        .collect()
}

// Order matches the qMRLab QMTSPGR filename convention: flip-<i>_mt-<i>.
const QMT_ENTITIES: &[EntityRole] = &[EntityRole::Flip, EntityRole::Mt];

impl Model for QmtModel {
    fn param_names(&self) -> Vec<&'static str> {
        vec!["F", "kr", "R1f", "R1r", "T2f", "T2r"]
    }
    fn output_names(&self) -> Vec<String> {
        self.fitter
            .output_names()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    fn param_bounds(&self) -> Vec<(f64, f64)> {
        self.fitter.param_bounds().to_vec()
    }
    fn fixed_mask(&self) -> Vec<bool> {
        self.fitter.fixed_mask().to_vec()
    }
    fn required_inputs(&self) -> Vec<InputSpec> {
        vec![
            InputSpec {
                name: "R1map",
                required: false,
                // This aux is an R1 map (rate, 1/s, VFA-derived per qMRLab),
                // not a T1 map — the honest BIDS locator is `R1map`, matching
                // both the data's actual units and what `bidsify` writes
                // (`sub-XX_R1map.nii.gz`). Labeling it `T1map` would be a
                // units/semantic error that a future BIDS-aux resolver would
                // silently fail to find (searching for a `T1map`-suffixed
                // file that doesn't exist).
                bids: Some(BidsMap {
                    suffix: "R1map",
                    entity: None,
                }),
            },
            InputSpec {
                name: "B1map",
                required: false,
                bids: Some(BidsMap {
                    suffix: "TB1map",
                    entity: None,
                }),
            },
            InputSpec {
                name: "B0map",
                required: false,
                bids: Some(BidsMap {
                    suffix: "B0map",
                    entity: None,
                }),
            },
        ]
    }
    fn measurement(&self) -> MeasurementKind {
        MeasurementKind::Series {
            rows: qmt_rows(&self.protocol),
        }
    }
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement {
        let x = [
            params[0], params[1], params[2], params[3], params[4], params[5],
        ];
        let b1 = aux.get("B1map").unwrap_or(1.0);
        let b0 = aux.get("B0map").unwrap_or(0.0);
        let r1 = aux.get("R1map");
        let values = self.fitter.forward(&x, b1, b0, r1);
        let samples = self
            .protocol
            .iter()
            .zip(values)
            .map(|(row, value)| Sample {
                params: BTreeMap::from([
                    ("Angle".to_string(), row[0]),
                    ("Offset".to_string(), row[1]),
                ]),
                value,
            })
            .collect();
        Measurement::Series(samples)
    }
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64> {
        let r1 = aux.get("R1map");
        let b1 = aux.get("B1map");
        let b0 = aux.get("B0map");
        // Assemble the full-protocol signal in the fitter's mtdata order by
        // matching each protocol row to its sample by (Angle, Offset). The
        // fitter then normalizes and selects rows internally, unchanged.
        // Identities must match: assembly is never positional. A row with no
        // matching sample is a mislabeled measurement → panic (the engine
        // records the voxel as a failed fit). (Angle, Offset) tuples are
        // assumed unique per protocol; first match wins.
        let samples = m.series();
        let signal: Vec<f64> = self
            .protocol
            .iter()
            .map(|row| {
                samples
                    .iter()
                    .find(|s| {
                        s.params.get("Angle") == Some(&row[0])
                            && s.params.get("Offset") == Some(&row[1])
                    })
                    .map(|s| s.value)
                    .unwrap_or_else(|| {
                        panic!(
                            "measurement has no sample with Angle={} Offset={}",
                            row[0], row[1]
                        )
                    })
            })
            .collect();
        self.fitter.fit_voxel(&signal, r1, b1, b0)
    }
    fn bids(&self) -> Option<BidsSpec> {
        Some(BidsSpec {
            suffix: "QMTSPGR",
            entities: QMT_ENTITIES,
        })
    }
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        // Matches the "Angle"/"Offset" keys `qmt_rows`/`forward`/`fit` use, so
        // a BIDS-resolved protocol is matched by identity, never by position.
        vec![
            ProtoParam {
                name: "Angle",
                source: Source::Field("Angle"),
                scope: Scope::PerVolume,
            },
            ProtoParam {
                name: "Offset",
                source: Source::Field("Offset"),
                scope: Scope::PerVolume,
            },
        ]
    }
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        // Per qMRLab QMTSPGR convention; `kf` (derived kr*F) and `resnorm`
        // (diagnostic) are omitted.
        vec![
            ("F", "Fmap", ""),
            ("kr", "kRmap", "1/s"),
            ("R1f", "R1Fmap", "1/s"),
            ("R1r", "R1Rmap", "1/s"),
            ("T2f", "T2Fmap", "s"),
            ("T2r", "T2Rmap", "s"),
        ]
    }
}

/// Registry builder: parse `QmtSpgrConfig` from the `qmt_spgr` sub-key of the
/// raw YAML tree, validate, and box the model. qMT reads its own acquisition
/// protocol from its config today, so `proto` is expected empty; it is still
/// checked for consistency in case a BIDS protocol source populates it later.
pub fn build(v: &serde_yaml::Value, proto: &Protocol) -> Result<Box<dyn Model>> {
    let mut cfg: QmtSpgrConfig = match v.get("qmt_spgr") {
        Some(sub) => serde_yaml::from_value(sub.clone())?,
        None => QmtSpgrConfig::default(),
    };
    cfg.validate()?;
    let model = QmtModel::new(&cfg);
    validate_against_protocol(&model.measurement(), proto)
        .context("qmt_spgr: protocol inconsistent with model's measurement")?;
    Ok(Box::new(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qmt_value() -> serde_yaml::Value {
        serde_yaml::from_str("model: qmt_spgr\n").unwrap()
    }

    #[test]
    fn build_defaults_and_shapes() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let sig = m.forward(&[0.16, 30.0, 1.0, 1.0, 0.03, 1.3e-5], &Aux::new());
        assert_eq!(sig.series().len(), 10);
        assert_eq!(m.param_names().len(), 6);
        assert_eq!(m.output_names().len(), 8);
        assert_eq!(m.fixed_mask(), vec![false, false, true, true, false, false]);
    }

    fn qmt_series(value: f64) -> Measurement {
        let cfg = crate::models::qmt_spgr::config::QmtSpgrConfig::default();
        let samples = cfg
            .protocol
            .mtdata
            .iter()
            .map(|row| Sample {
                params: BTreeMap::from([
                    ("Angle".to_string(), row[0]),
                    ("Offset".to_string(), row[1]),
                ]),
                value,
            })
            .collect();
        Measurement::Series(samples)
    }

    #[test]
    fn fit_shape_via_trait() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let mut aux = Aux::new();
        aux.set("R1map", 1.0);
        let out = m.fit(&qmt_series(0.5), &aux);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn fit_assembles_by_identity_not_position() {
        // Reversing the (distinct) protocol samples must not change the fit:
        // matching is by (Angle, Offset), never by array position.
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let mut aux = Aux::new();
        aux.set("R1map", 1.0);
        // Distinct per-volume values via a clean forward, then reverse them.
        let forward = m.forward(&[0.15, 25.0, 1.0, 1.0, 0.028, 1.1e-5], &aux);
        let mut reversed: Vec<Sample> = match forward {
            Measurement::Series(ref s) => s
                .iter()
                .map(|s| Sample {
                    params: s.params.clone(),
                    value: s.value,
                })
                .collect(),
            _ => unreachable!(),
        };
        reversed.reverse();
        let a = m.fit(&forward, &aux);
        let b = m.fit(&Measurement::Series(reversed), &aux);
        assert_eq!(a, b, "qMT fit must be identical under sample reordering");
    }

    #[test]
    #[should_panic(expected = "no sample with Angle")]
    fn fit_panics_on_unmatched_identity() {
        // Sample identities that match no protocol row must fail loudly, never
        // fall back to positional assembly.
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let bogus = Measurement::Series(vec![Sample {
            params: BTreeMap::from([("Angle".to_string(), 999.0), ("Offset".to_string(), 999.0)]),
            value: 0.5,
        }]);
        let _ = m.fit(&bogus, &Aux::new());
    }

    #[test]
    fn bids_outputs_reference_real_output_names() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        let names = m.output_names();
        for (out, _suffix, _units) in m.bids_outputs() {
            assert!(
                names.iter().any(|n| n == out),
                "bids_outputs references '{out}', not in output_names {names:?}"
            );
        }
    }

    #[test]
    fn declares_bids_qmtspgr() {
        let m = build(&qmt_value(), &Protocol::default()).unwrap();
        assert_eq!(m.bids().unwrap().suffix, "QMTSPGR");
    }
}
