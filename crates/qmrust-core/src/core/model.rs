//! The single contributor surface: the object-safe [`Model`] trait plus the
//! value types the shell uses to drive it. Nothing here touches I/O or
//! config-file formats — this is the functional-core boundary.

use anyhow::{bail, Result as AnyResult};
use std::collections::BTreeMap;

/// How the engine iterates the volume when fitting.
pub enum FitStrategy {
    /// Fit each voxel independently (parallel). The only strategy implemented.
    Voxelwise,
    /// Fit the whole volume jointly (e.g. dictionary/matrix methods). Seam only.
    MatrixWise,
}

/// BIDS locator for a single auxiliary input (used by the shell, not the core).
pub struct BidsMap {
    /// BIDS suffix that identifies the map, e.g. `"TB1map"`.
    pub suffix: &'static str,
    /// Entity that indexes it within a collection, if any.
    pub entity: Option<&'static str>,
}

/// One auxiliary input a model consumes (B1/B0/R1 today).
pub struct InputSpec {
    /// Logical name the compute layer reads: `aux.get(name)`.
    pub name: &'static str,
    /// Whether the fit requires it (vs. a sensible default when absent).
    pub required: bool,
    /// How to locate it in a BIDS dataset; `None` = not BIDS-locatable.
    pub bids: Option<BidsMap>,
}

/// Role an entity plays in indexing a model's acquisition axis. Seam for the
/// BIDS protocol mapping that the shell / `rust-bids` crate fills in.
pub enum EntityRole {
    Inv,
    Flip,
    Mt,
    Echo,
    Other(&'static str),
}

/// A model's BIDS identity: its grouping suffix and the entities that index
/// its protocol axis.
pub struct BidsSpec {
    pub suffix: &'static str,
    pub entities: &'static [EntityRole],
}

/// Resolved acquisition protocol, in BIDS-sidecar shape: one metadata map per
/// volume plus shared globals. An empty `Protocol` means "model, use the
/// protocol from your own config".
#[derive(Default, Clone)]
pub struct Protocol {
    pub volumes: Vec<BTreeMap<String, f64>>,
    pub global: BTreeMap<String, f64>,
}

impl Protocol {
    pub fn is_empty(&self) -> bool {
        self.volumes.is_empty() && self.global.is_empty()
    }
}

/// Per-voxel (or per-sim) scalar auxiliary values, keyed by [`InputSpec::name`].
#[derive(Default, Clone)]
pub struct Aux(BTreeMap<String, f64>);

impl Aux {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn get(&self, key: &str) -> Option<f64> {
        self.0.get(key).copied()
    }
    pub fn set(&mut self, key: &str, value: f64) {
        self.0.insert(key.to_string(), value);
    }
}

/// What shape of measurement a model consumes, and the identities it reads by.
pub enum MeasurementKind {
    /// A fixed set of role-labeled volumes (e.g. MTS: `["PDw","MTw","T1w"]`).
    Named { roles: &'static [&'static str] },
    /// A variable-length series whose per-volume identity rows the model owns:
    /// one `params` row per acquired volume, in the model's canonical order
    /// (e.g. IRT1: one `{"InversionTime": ti}` per TI). The shell labels each
    /// data volume with one of these rows, so `fit` assembles the signal by
    /// matching identities by value — never by array position. Each row's keys
    /// are the series' acquisition axes.
    Series { rows: Vec<BTreeMap<String, f64>> },
}

/// Fail loudly, at build time, if a supplied `Protocol` is inconsistent with a
/// model's declared measurement shape. An empty `proto` means "model, use
/// your own config" and is always consistent (nothing to check). A non-empty
/// `proto` against a `Series` model must supply exactly one volume per
/// expected identity row, each carrying every key the model's rows use — a
/// count or key mismatch here would otherwise surface only per-voxel, as a
/// fit-time panic for every voxel whose identity has no matching sample.
///
/// `Named` models are not designed to accept an external protocol today (no
/// shipping `Named` model reads one): a non-empty `proto` supplied to one is
/// therefore rejected rather than silently ignored.
pub fn validate_against_protocol(kind: &MeasurementKind, proto: &Protocol) -> AnyResult<()> {
    if proto.volumes.is_empty() {
        return Ok(());
    }
    match kind {
        MeasurementKind::Series { rows } => {
            if proto.volumes.len() != rows.len() {
                let keys: Vec<&str> = series_keys(rows);
                bail!(
                    "expected {} volumes ({}), protocol supplies {}",
                    rows.len(),
                    keys.join(", "),
                    proto.volumes.len()
                );
            }
            for key in series_keys(rows) {
                if let Some((i, _)) = proto
                    .volumes
                    .iter()
                    .enumerate()
                    .find(|(_, vol)| !vol.contains_key(key))
                {
                    bail!("protocol volume {} is missing expected key '{}'", i, key);
                }
            }
            Ok(())
        }
        MeasurementKind::Named { .. } => {
            bail!(
                "Named measurement does not accept an external protocol; supplied protocol has {} volumes",
                proto.volumes.len()
            );
        }
    }
}

/// Every distinct param key used across a `Series` model's canonical rows, in
/// sorted order (rows may all share the same key set, but this holds even if
/// a future model's rows vary per-volume).
fn series_keys(rows: &[BTreeMap<String, f64>]) -> Vec<&str> {
    let mut keys: Vec<&str> = rows
        .iter()
        .flat_map(|r| r.keys())
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}

/// One acquired volume's value with the metadata identifying it.
pub struct Sample {
    pub params: BTreeMap<String, f64>,
    pub value: f64,
}

/// Per-voxel measurement handed to a model. Read by identity, never by index.
pub enum Measurement {
    Named(BTreeMap<&'static str, f64>),
    Series(Vec<Sample>),
}

impl Measurement {
    pub fn role(&self, name: &str) -> Option<f64> {
        match self {
            Measurement::Named(m) => m.get(name).copied(),
            Measurement::Series(_) => None,
        }
    }
    pub fn series(&self) -> &[Sample] {
        match self {
            Measurement::Series(s) => s,
            Measurement::Named(_) => &[],
        }
    }
}

/// Identity of one volume along the acquisition axis, supplied by the shell.
pub enum VolumeId {
    Role(&'static str),
    Params(BTreeMap<String, f64>),
}

/// The single surface a model contributor implements. Object-safe so the
/// registry can hold `Box<dyn Model>`.
pub trait Model: Send + Sync {
    /// Ground-truth parameter names, in `forward` order.
    fn param_names(&self) -> Vec<&'static str>;
    /// Names of the fitted output maps, in `fit` return order.
    fn output_names(&self) -> Vec<String>;
    /// Per-parameter `(lower, upper)` fit bounds, in `param_names` order.
    fn param_bounds(&self) -> Vec<(f64, f64)>;
    /// Per-parameter fixed flags (true = not independently recovered).
    fn fixed_mask(&self) -> Vec<bool>;
    /// Auxiliary inputs this model consumes.
    fn required_inputs(&self) -> Vec<InputSpec>;
    /// The shape of measurement this model consumes and the identities it reads by.
    fn measurement(&self) -> MeasurementKind;
    /// Fit granularity. Defaults to voxelwise.
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    /// Noise-free forward signal for `params`, identity-tagged per `measurement`.
    fn forward(&self, params: &[f64], aux: &Aux) -> Measurement;
    /// Fit an identity-keyed measurement, returning values in `output_names` order.
    fn fit(&self, m: &Measurement, aux: &Aux) -> Vec<f64>;
    /// BIDS identity, if this model maps to a BIDS grouping suffix.
    fn bids(&self) -> Option<BidsSpec> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aux_set_and_get() {
        let mut a = Aux::new();
        a.set("B1map", 1.2);
        assert_eq!(a.get("B1map"), Some(1.2));
        assert_eq!(a.get("missing"), None);
    }

    #[test]
    fn protocol_empty_default() {
        let p = Protocol::default();
        assert!(p.is_empty());
    }

    // Compile-time proof the trait is object-safe.
    #[test]
    fn model_is_object_safe() {
        fn _takes(_m: &dyn Model) {}
    }

    #[test]
    fn measurement_named_reads_by_role() {
        let mut m = std::collections::BTreeMap::new();
        m.insert("MTw", 2.0);
        m.insert("PDw", 1.0);
        let meas = Measurement::Named(m);
        assert_eq!(meas.role("MTw"), Some(2.0));
        assert_eq!(meas.role("absent"), None);
        assert!(meas.series().is_empty());
    }

    #[test]
    fn measurement_series_reads_samples() {
        let meas = Measurement::Series(vec![Sample {
            params: [("InversionTime".to_string(), 30.0)].into(),
            value: 5.0,
        }]);
        assert_eq!(meas.series().len(), 1);
        assert_eq!(meas.series()[0].params["InversionTime"], 30.0);
        assert_eq!(meas.role("MTw"), None);
    }

    fn series_kind(tis: &[f64]) -> MeasurementKind {
        MeasurementKind::Series {
            rows: tis
                .iter()
                .map(|&ti| BTreeMap::from([("InversionTime".to_string(), ti)]))
                .collect(),
        }
    }

    fn proto_with_tis(tis: &[f64]) -> Protocol {
        Protocol {
            volumes: tis
                .iter()
                .map(|&ti| BTreeMap::from([("InversionTime".to_string(), ti)]))
                .collect(),
            global: BTreeMap::new(),
        }
    }

    #[test]
    fn validate_against_protocol_ok_when_empty_protocol() {
        let kind = series_kind(&[350.0, 500.0, 650.0]);
        assert!(validate_against_protocol(&kind, &Protocol::default()).is_ok());
    }

    #[test]
    fn validate_against_protocol_ok_when_consistent() {
        let tis = [350.0, 500.0, 650.0];
        let kind = series_kind(&tis);
        let proto = proto_with_tis(&tis);
        assert!(validate_against_protocol(&kind, &proto).is_ok());
    }

    #[test]
    fn validate_against_protocol_rejects_wrong_volume_count() {
        let kind = series_kind(&[350.0, 500.0, 650.0]);
        let proto = proto_with_tis(&[350.0, 500.0]);
        let err = validate_against_protocol(&kind, &proto).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("expected 3 volumes"), "{msg}");
        assert!(msg.contains("InversionTime"), "{msg}");
        assert!(msg.contains("supplies 2"), "{msg}");
    }

    #[test]
    fn validate_against_protocol_rejects_missing_key() {
        let kind = series_kind(&[350.0, 500.0]);
        let mut proto = proto_with_tis(&[350.0, 500.0]);
        proto.volumes[1].remove("InversionTime");
        proto.volumes[1].insert("OtherKey".to_string(), 1.0);
        let err = validate_against_protocol(&kind, &proto).unwrap_err();
        assert!(err.to_string().contains("InversionTime"));
    }

    #[test]
    fn validate_against_protocol_rejects_named_with_nonempty_protocol() {
        let kind = MeasurementKind::Named {
            roles: &["PDw", "MTw", "T1w"],
        };
        let proto = proto_with_tis(&[1.0]);
        assert!(validate_against_protocol(&kind, &proto).is_err());
    }
}
