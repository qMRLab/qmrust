//! The single contributor surface: the object-safe [`Model`] trait plus the
//! value types the shell uses to drive it. Nothing here touches I/O or
//! config-file formats — this is the functional-core boundary.

use anyhow::{bail, Context, Result as AnyResult};
use serde::de::DeserializeOwned;
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

/// One auxiliary scalar input a model consumes (e.g. a B1, B0, or R1 map).
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
#[derive(Debug, Default, Clone)]
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

/// A model's configuration, wired into the one shared build pipeline.
///
/// The platform owns the flow — parse the config, validate its options, ingest
/// the BIDS-resolved acquisition protocol, validate protocol completeness,
/// construct the model, and check it against the protocol. A model supplies only
/// these config-shaped hooks. Protocol ingestion lives in [`build_model`] alone,
/// so every model sources its acquisition from BIDS identically and none can be
/// built without it.
pub trait ModelConfig: DeserializeOwned + serde::Serialize + Default + Clone {
    /// Model name, used for error context.
    const NAME: &'static str;
    /// YAML sub-key this config lives under (e.g. `Some("qmt_spgr")`), or `None`
    /// to read the top-level document (e.g. inversion_recovery).
    const SUBKEY: Option<&'static str>;

    /// Config paths whose value `ingest_protocol` sources from the resolved
    /// protocol. A UI must not offer these as editable when a protocol is
    /// present: `ingest_protocol` overwrites them, so a typed value is
    /// discarded, and a value that does survive duplicates into the output
    /// provenance the per-volume axis `Protocol` already records.
    ///
    /// Dotted paths **relative to this config struct**, not to the emitted
    /// document — `"protocol.mtdata"`, not `"qmt_spgr.protocol.mtdata"`.
    /// `effective_model` prefixes `SUBKEY` so the emitted paths match the YAML
    /// it emits. A path may name an object (`mt_sat`'s `"mtw"`), which locks
    /// everything under it.
    ///
    /// Empty for a model with no acquisition axis (e.g. `mt_ratio`).
    const PROTOCOL_KEYS: &'static [&'static str] = &[];

    /// Config-intrinsic validation — checks that need no protocol.
    fn validate_options(&mut self) -> AnyResult<()>;

    /// Fold the BIDS-resolved per-volume protocol into this config's acquisition
    /// arrays (e.g. `InversionTime`s, or `[Angle, Offset]` rows). Runs once, in
    /// the shared pipeline, for every model. Default: a no-op, for a model whose
    /// acquisition is not BIDS-sourced. An empty `proto` leaves the config as
    /// written (the non-BIDS path, where the config carries the acquisition).
    fn ingest_protocol(&mut self, _proto: &Protocol) -> AnyResult<()> {
        Ok(())
    }

    /// Protocol-completeness validation, run after ingestion.
    fn validate_protocol(&mut self) -> AnyResult<()> {
        Ok(())
    }

    /// Construct the fit-ready model from the finalized config.
    fn into_model(self) -> Box<dyn Model>;
}

fn parse_model_config<C: ModelConfig>(v: &serde_yaml::Value) -> AnyResult<C> {
    let cfg = match C::SUBKEY {
        Some(key) => match v.get(key) {
            Some(sub) => serde_yaml::from_value(sub.clone())?,
            None => C::default(),
        },
        None => serde_yaml::from_value(v.clone())?,
    };
    Ok(cfg)
}

/// Construct a model from config alone for structural interrogation
/// (`protocol_schema`, `bids_outputs`, `required_inputs`), running only
/// config-intrinsic validation — no protocol, no completeness check. Not
/// fit-ready. The BIDS shell uses this to read a model's contract before it has
/// resolved any protocol.
pub fn describe_model<C: ModelConfig>(v: &serde_yaml::Value) -> AnyResult<Box<dyn Model>> {
    let mut cfg = parse_model_config::<C>(v)?;
    cfg.validate_options()
        .with_context(|| format!("{}: invalid config", C::NAME))?;
    Ok(cfg.into_model())
}

/// The one build pipeline every model runs: parse → validate options → ingest
/// the resolved BIDS protocol → validate protocol completeness → construct →
/// check against the protocol. Returns a fit-ready model.
pub fn build_model<C: ModelConfig>(
    v: &serde_yaml::Value,
    proto: &Protocol,
) -> AnyResult<Box<dyn Model>> {
    let mut cfg = parse_model_config::<C>(v)?;
    cfg.validate_options()
        .with_context(|| format!("{}: invalid config", C::NAME))?;
    cfg.ingest_protocol(proto)
        .with_context(|| format!("{}: ingesting BIDS protocol", C::NAME))?;
    cfg.validate_protocol()
        .with_context(|| format!("{}: invalid protocol", C::NAME))?;
    let model = cfg.into_model();
    validate_against_protocol(&model.measurement(), proto).with_context(|| {
        format!(
            "{}: protocol inconsistent with model's measurement",
            C::NAME
        )
    })?;
    Ok(model)
}

/// Print the fully-resolved effective config (defaults materialized, options
/// validated) as YAML. Display validates options only — protocol completeness
/// is a fit concern, not a display one.
pub fn dump_model<C: ModelConfig>(v: &serde_yaml::Value) -> AnyResult<String> {
    let mut cfg = parse_model_config::<C>(v)?;
    cfg.validate_options()
        .with_context(|| format!("{}: invalid config", C::NAME))?;
    serialize_effective::<C>(&cfg)
}

/// A model's full option surface, for a UI that must show what is adjustable
/// rather than what a recipe happens to mention.
///
/// `dump_model` cannot serve this: it refuses to emit anything when validation
/// fails, and a config is routinely invalid mid-edit (and some are invalid by
/// default — `inversion_recovery` has no `method` until one is chosen). A panel
/// that blanks whenever the config is momentarily wrong is unusable, so the
/// surface is emitted regardless of validity and the error travels beside it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EffectiveConfig {
    /// Every key the model accepts, at its effective value, as YAML.
    pub yaml: String,
    /// The model's own `validate_options` message, verbatim, or `None`.
    pub error: Option<String>,
    /// Config keys this model sources from the resolved protocol.
    pub protocol_keys: Vec<String>,
}

/// Serialize `v` as `C`'s full option surface, with the resolved `proto`
/// folded in and serde defaults materialized, plus whatever `validate_options`
/// makes of it.
///
/// Order: parse, then `ingest_protocol` — so a protocol-sourced key's row
/// shows the real resolved value rather than the struct default — then
/// validate a **clone**. `ModelConfig::validate_options` takes `&mut self` and
/// several models normalize as they go, so validating the original would
/// serialize state left half-normalized by a validator that failed partway.
/// Cloning makes the surface depend only on what was parsed and ingested.
pub fn effective_model<C: ModelConfig>(
    v: &serde_yaml::Value,
    proto: &Protocol,
) -> AnyResult<EffectiveConfig> {
    let mut cfg = parse_model_config::<C>(v)?;
    cfg.ingest_protocol(proto)
        .with_context(|| format!("{}: ingesting BIDS protocol", C::NAME))?;
    let error = cfg
        .clone()
        .validate_options()
        .err()
        .map(|e| format!("{e:#}"));
    Ok(EffectiveConfig {
        yaml: serialize_effective::<C>(&cfg)?,
        error,
        protocol_keys: C::PROTOCOL_KEYS
            .iter()
            .map(|k| match C::SUBKEY {
                Some(sub) => format!("{sub}.{k}"),
                None => (*k).to_string(),
            })
            .collect(),
    })
}

/// Whether `text` (YAML) declares `key`'s leaf segment as a mapping key on its
/// own line. A dotted path (`"qmt_spgr.protocol.mtdata"`) nests across
/// separate YAML lines, so only the leaf segment can ever match a single line
/// verbatim.
pub fn states_key(text: &str, key: &str) -> bool {
    let leaf = key.rsplit('.').next().unwrap();
    text.lines()
        .any(|l| l.trim_start().starts_with(&format!("{leaf}:")))
}

/// `model: <name>` plus the config body, nested under `C::SUBKEY` when it has
/// one. Shared with `dump_model`, so both emit the same shape.
fn serialize_effective<C: ModelConfig>(cfg: &C) -> AnyResult<String> {
    let body = serde_yaml::to_string(cfg)?;
    let mut out = format!("model: {}\n", C::NAME);
    // A fieldless config serializes to the flow mapping `{}`; appending that
    // after `model: <name>` would be invalid YAML, so emit just the model line.
    if body.trim() == "{}" {
        return Ok(out);
    }
    match C::SUBKEY {
        Some(key) => {
            out.push_str(&format!("{key}:\n"));
            for line in body.lines() {
                if line.is_empty() {
                    out.push('\n');
                } else {
                    out.push_str("  ");
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        None => out.push_str(&body),
    }
    Ok(out)
}

/// Fail loudly, at build time, if a supplied `Protocol` is inconsistent with a
/// model's declared measurement shape. An empty `proto` means "model, use
/// your own config" and is always consistent (nothing to check). A non-empty
/// `proto` against a `Series` model must supply exactly one volume per
/// expected identity row, each carrying every key the model's rows use — a
/// count or key mismatch here would otherwise surface only per-voxel, as a
/// fit-time panic for every voxel whose identity has no matching sample.
///
/// A `Named` measurement's identities are its fixed roles, not the protocol; a
/// non-empty `proto` against one is a per-role acquisition (e.g. MTS's
/// FlipAngle/RepetitionTimeExcitation per role), which the shell has already
/// ordered to the model's roles. The only check is one acquisition row per
/// role — a count mismatch means the shell mis-assembled the per-role protocol.
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
            let keys = series_keys(rows);
            for key in &keys {
                if let Some((i, _)) = proto
                    .volumes
                    .iter()
                    .enumerate()
                    .find(|(_, vol)| !vol.contains_key(*key))
                {
                    bail!("protocol volume {} is missing expected key '{}'", i, key);
                }
            }
            // Count + keys match; now confirm the supplied rows are the same
            // *multiset* of identities as the model's canonical rows (order-free —
            // the shell may hand volumes back in any order). Matching each
            // canonical row against a shrinking pool of supplied rows, by exact
            // value, catches a sidecar/protocol whose TIs (etc.) don't line up
            // with the model's rows even though the count and keys do.
            let mut pool: Vec<&BTreeMap<String, f64>> = proto.volumes.iter().collect();
            for model_row in rows {
                let row_bits = |m: &BTreeMap<String, f64>| -> Vec<u64> {
                    keys.iter().map(|k| m[*k].to_bits()).collect()
                };
                let target = row_bits(model_row);
                match pool.iter().position(|vol| row_bits(vol) == target) {
                    Some(pos) => {
                        pool.remove(pos);
                    }
                    None => {
                        let desc: Vec<String> = keys
                            .iter()
                            .map(|k| format!("{}={}", k, model_row[*k]))
                            .collect();
                        bail!(
                            "protocol does not supply a volume matching expected identity ({}); \
                             supplied values differ from the model's canonical protocol",
                            desc.join(", ")
                        );
                    }
                }
            }
            Ok(())
        }
        MeasurementKind::Named { roles } => {
            if proto.volumes.len() != roles.len() {
                bail!(
                    "Named measurement has {} roles ({:?}) but the per-role protocol supplies {} rows",
                    roles.len(),
                    roles,
                    proto.volumes.len()
                );
            }
            Ok(())
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

/// One acquired volume's BIDS write descriptor: filename entities + sidecar
/// metadata, computed by the model from its own protocol. The shell writes it
/// verbatim, knowing nothing about the entity meanings.
pub struct BidsVolume {
    /// Filename entities as (key, value), in filename order:
    /// e.g. [("inv", "1")] or [("flip", "1"), ("mt", "off")].
    pub entities: Vec<(&'static str, String)>,
    /// Per-volume sidecar metadata as BIDS JSON values.
    pub sidecar: BTreeMap<String, serde_json::Value>,
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

/// Read-only metadata view a `Source::Derived` fn reads from. Lets a model
/// declare a derivation over sidecar-shaped metadata without core naming
/// `rust_bids::Sidecar` (the dependency arrow points rust-bids -> core, never
/// the reverse); `rust-bids`'s `Sidecar` implements this trait.
pub trait Meta {
    fn f64(&self, k: &str) -> Option<f64>;
    fn str(&self, k: &str) -> Option<&str>;
    fn array(&self, k: &str) -> Option<&[serde_json::Value]>;
}

/// Whether a [`ProtoParam`] is evaluated once per volume or once for the
/// whole collection.
pub enum Scope {
    PerVolume,
    Global,
}

/// Where a [`ProtoParam`]'s value comes from.
pub enum Source {
    /// Direct sidecar field, read by key.
    Field(&'static str),
    /// Computed from sidecar metadata (e.g. combining two fields). A plain fn
    /// pointer (not a closure) keeps `Model` object-safe and dependency-free.
    Derived(fn(&dyn Meta) -> AnyResult<f64>),
    /// Non-BIDS fallback: read from the caller-supplied options map instead
    /// of any sidecar.
    Option(&'static str),
}

/// One parameter a model wants resolved from a BIDS protocol: its name in the
/// resulting `Protocol` map, where its value comes from, and at what scope.
pub struct ProtoParam {
    pub name: &'static str,
    pub source: Source,
    pub scope: Scope,
    /// Whether an unresolvable value is a hard error. `false` means the
    /// fitter does not need this param — `resolve_protocol` silently omits it
    /// from the `Protocol` rather than bailing, so a dataset whose sidecars
    /// lack it still fits.
    pub required: bool,
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
    /// Auxiliary inputs (by logical name) that this *configured* model actively
    /// uses, such that a simulation omitting them would silently fail to
    /// exercise the model's real fitting behaviour. The sim layer requires the
    /// sim block to supply each. Empty (the default) means no sim-critical aux.
    /// Distinct from [`Model::required_inputs`]: an aux the fit uses only when
    /// present (never a hard fit requirement) can still be sim-critical here.
    fn sim_required_aux(&self) -> Vec<&'static str> {
        vec![]
    }
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
    /// Number of acquired volumes this model's protocol describes.
    fn n_volumes(&self) -> usize;
    /// BIDS write descriptor for the i-th volume (0-based).
    fn bids_volume(&self, index: usize) -> BidsVolume;
    /// BIDS identity, if this model maps to a BIDS grouping suffix.
    fn bids(&self) -> Option<BidsSpec> {
        None
    }
    /// Declarative mapping from BIDS sidecar metadata (or config options) to
    /// this model's protocol axis. Empty means "no declared mapping" — the
    /// shell falls back to the model's own config.
    fn protocol_schema(&self) -> Vec<ProtoParam> {
        vec![]
    }
    /// BIDS output-map declarations: triples of (an entry in
    /// [`Model::output_names`], its qMRLab-convention BIDS map suffix, and the
    /// map's physical unit as a BIDS/SI string, e.g. `("T1", "T1map", "s")`;
    /// unitless quantities (e.g. a bound-pool fraction) use `""`. Invariant:
    /// only real quantitative maps are listed here — diagnostics (residuals,
    /// scenario indices, …) are omitted, and every first element must be a
    /// genuine `output_names()` entry. Empty means "no declared BIDS outputs".
    fn bids_outputs(&self) -> Vec<(&'static str, &'static str, &'static str)> {
        vec![]
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
    fn validate_against_protocol_ok_when_permuted() {
        let kind = series_kind(&[350.0, 500.0, 650.0]);
        // Same TIs, count-matching, but supplied in a different order.
        let proto = proto_with_tis(&[650.0, 350.0, 500.0]);
        assert!(validate_against_protocol(&kind, &proto).is_ok());
    }

    #[test]
    fn validate_against_protocol_rejects_wrong_value_at_matching_count() {
        let kind = series_kind(&[350.0, 500.0, 650.0]);
        // Same count and keys, but one TI value doesn't match any canonical row.
        let proto = proto_with_tis(&[350.0, 500.0, 700.0]);
        let err = validate_against_protocol(&kind, &proto).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("InversionTime=650"), "{msg}");
    }

    struct NoSchemaModel;

    impl Model for NoSchemaModel {
        fn param_names(&self) -> Vec<&'static str> {
            vec![]
        }
        fn output_names(&self) -> Vec<String> {
            vec![]
        }
        fn param_bounds(&self) -> Vec<(f64, f64)> {
            vec![]
        }
        fn fixed_mask(&self) -> Vec<bool> {
            vec![]
        }
        fn required_inputs(&self) -> Vec<InputSpec> {
            vec![]
        }
        fn measurement(&self) -> MeasurementKind {
            MeasurementKind::Named { roles: &[] }
        }
        fn forward(&self, _params: &[f64], _aux: &Aux) -> Measurement {
            Measurement::Named(BTreeMap::new())
        }
        fn fit(&self, _m: &Measurement, _aux: &Aux) -> Vec<f64> {
            vec![]
        }
        fn n_volumes(&self) -> usize {
            0
        }
        fn bids_volume(&self, _index: usize) -> BidsVolume {
            BidsVolume {
                entities: vec![],
                sidecar: BTreeMap::new(),
            }
        }
    }

    #[test]
    fn protocol_schema_defaults_to_empty_and_stays_object_safe() {
        let m = NoSchemaModel;
        assert!(m.protocol_schema().is_empty());
        let dyn_m: &dyn Model = &m;
        assert!(dyn_m.protocol_schema().is_empty());
    }

    #[test]
    fn bids_outputs_defaults_to_empty_and_stays_object_safe() {
        let m = NoSchemaModel;
        assert!(m.bids_outputs().is_empty());
        let dyn_m: &dyn Model = &m;
        assert!(dyn_m.bids_outputs().is_empty());
    }

    #[test]
    fn validate_against_protocol_named_accepts_one_row_per_role() {
        // A Named measurement may carry a per-role acquisition protocol; the
        // check is one row per role (the shell orders rows to the roles).
        let kind = MeasurementKind::Named {
            roles: &["MTw", "PDw", "T1w"],
        };
        let ok = Protocol {
            volumes: vec![
                BTreeMap::from([("FlipAngle".to_string(), 6.0)]),
                BTreeMap::from([("FlipAngle".to_string(), 6.0)]),
                BTreeMap::from([("FlipAngle".to_string(), 20.0)]),
            ],
            global: BTreeMap::new(),
        };
        assert!(validate_against_protocol(&kind, &ok).is_ok());

        // A count mismatch (2 rows for 3 roles) means the shell mis-assembled
        // the per-role protocol — a hard error.
        let bad = proto_with_tis(&[1.0, 2.0]);
        let err = validate_against_protocol(&kind, &bad).unwrap_err();
        assert!(err.to_string().contains("3 roles"), "{err}");
    }

    #[test]
    fn effective_config_fills_defaults_and_reports_no_error_when_valid() {
        use crate::models::mono_t2::config::MonoT2Config;
        let v: serde_yaml::Value =
            serde_yaml::from_str("model: mono_t2\necho_times: [0.01, 0.02, 0.03]\n").unwrap();
        let eff = effective_model::<MonoT2Config>(&v, &Protocol::default()).unwrap();
        // Every option appears, including ones the recipe never mentioned.
        assert!(eff.yaml.contains("fit_type:"), "{}", eff.yaml);
        assert!(eff.yaml.contains("drop_first_echo:"), "{}", eff.yaml);
        assert!(eff.yaml.contains("offset_term:"), "{}", eff.yaml);
        assert!(eff.error.is_none(), "{:?}", eff.error);
    }

    #[test]
    fn effective_config_returns_the_surface_and_the_error_when_invalid() {
        use crate::models::mono_t2::config::MonoT2Config;
        // `offset_term` is rejected with the linear fit_type — but the reader still
        // needs to see every field in order to fix it, so the surface must survive.
        let v: serde_yaml::Value = serde_yaml::from_str(
            "model: mono_t2\necho_times: [0.01, 0.02, 0.03]\nfit_type: linear\noffset_term: true\n",
        )
        .unwrap();
        let eff = effective_model::<MonoT2Config>(&v, &Protocol::default()).unwrap();
        assert!(
            eff.yaml.contains("offset_term:"),
            "surface withheld: {}",
            eff.yaml
        );
        let msg = eff.error.expect("expected a validation error");
        assert!(msg.contains("offset_term"), "{msg}");
    }

    #[test]
    fn effective_config_is_unaffected_by_validation_mutating_the_config() {
        use crate::models::inversion_recovery::config::IrConfig;
        // IR's validate_options bails without a `method`. A validator that mutates
        // before failing must not leak half-normalized state into the surface, so
        // the surface is serialized from the config validation never touched.
        let v: serde_yaml::Value = serde_yaml::from_str("model: inversion_recovery\n").unwrap();
        let eff = effective_model::<IrConfig>(&v, &Protocol::default()).unwrap();
        assert!(eff.yaml.contains("t1_range:"), "{}", eff.yaml);
        assert!(
            eff.error.is_some(),
            "IR with no method must report its error"
        );
    }

    #[test]
    fn effective_config_nests_under_a_subkey() {
        use crate::models::qmt_spgr::config::QmtSpgrConfig;
        let v: serde_yaml::Value = serde_yaml::from_str("model: qmt_spgr\n").unwrap();
        let eff = effective_model::<QmtSpgrConfig>(&v, &Protocol::default()).unwrap();
        assert!(
            eff.yaml.starts_with("model: qmt_spgr\nqmt_spgr:\n"),
            "{}",
            eff.yaml
        );
    }

    #[test]
    fn effective_config_shows_the_protocol_ingested_value_for_a_locked_key() {
        use crate::models::vfa_t1::config::VfaT1Config;
        // The BIDS recipe omits the acquisition; the surface must show what
        // ingest_protocol actually resolved, not the struct default.
        let v: serde_yaml::Value = serde_yaml::from_str("model: vfa_t1\n").unwrap();
        let proto = Protocol {
            volumes: vec![
                BTreeMap::from([("FlipAngle".to_string(), 3.0)]),
                BTreeMap::from([("FlipAngle".to_string(), 20.0)]),
            ],
            global: BTreeMap::from([("RepetitionTimeExcitation".to_string(), 0.015)]),
        };
        let eff = effective_model::<VfaT1Config>(&v, &proto).unwrap();
        let parsed: serde_yaml::Value = serde_yaml::from_str(&eff.yaml).unwrap();
        assert_eq!(
            parsed["flip_angles"],
            serde_yaml::to_value(vec![3.0, 20.0]).unwrap(),
            "{}",
            eff.yaml
        );
        assert_eq!(
            parsed["repetition_time"],
            serde_yaml::to_value(0.015).unwrap(),
            "{}",
            eff.yaml
        );
    }

    #[test]
    fn every_registered_model_yields_a_surface() {
        for entry in crate::registry::all() {
            let v: serde_yaml::Value =
                serde_yaml::from_str(&format!("model: {}\n", entry.name)).unwrap();
            let eff = (entry.effective)(&v, &Protocol::default())
                .unwrap_or_else(|e| panic!("{}: effective failed: {e:#}", entry.name));
            assert!(
                eff.yaml.contains(&format!("model: {}", entry.name)),
                "{}: {}",
                entry.name,
                eff.yaml
            );
        }
    }

    #[test]
    fn protocol_keys_travel_with_the_surface_and_name_real_fields() {
        // Every declared key must exist in the serialized config, or the UI would
        // be told to treat a key that does not exist as protocol-sourced.
        for entry in crate::registry::all() {
            let v: serde_yaml::Value =
                serde_yaml::from_str(&format!("model: {}\n", entry.name)).unwrap();
            let eff = (entry.effective)(&v, &Protocol::default()).unwrap();
            for key in &eff.protocol_keys {
                assert!(
                    states_key(&eff.yaml, key),
                    "{}: declares protocol key '{key}', absent from its config: {}",
                    entry.name,
                    eff.yaml
                );
            }
        }
    }

    fn set_nested(v: &mut serde_yaml::Value, dotted_key: &str, new_value: &str) {
        let parts: Vec<&str> = dotted_key.split('.').collect();
        let mut node = v;
        for part in &parts[..parts.len() - 1] {
            node = node
                .as_mapping_mut()
                .unwrap()
                .entry(serde_yaml::Value::String(part.to_string()))
                .or_insert(serde_yaml::Value::Mapping(Default::default()));
        }
        node.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::String(parts.last().unwrap().to_string()),
            serde_yaml::Value::String(new_value.to_string()),
        );
    }

    #[test]
    fn models_with_an_acquisition_axis_declare_it() {
        // A model whose protocol_schema has per-volume params folds them into some
        // config key; leaving PROTOCOL_KEYS empty would make the UI offer an
        // editable field whose value ingest_protocol discards.
        use crate::core::model::Scope;
        let mut skipped = Vec::new();
        for entry in crate::registry::all() {
            let mut v: serde_yaml::Value =
                serde_yaml::from_str(&format!("model: {}\n", entry.name)).unwrap();
            // Some models (inversion_recovery's `method`) have no valid default and
            // only describe once an enum-constrained option is set; seed the first
            // declared value for each so a bare `model: <name>` still describes.
            for (key, values) in entry.doc.enums {
                set_nested(&mut v, key, values[0]);
            }
            let model = match (entry.describe)(&v) {
                Ok(model) => model,
                Err(_) => {
                    skipped.push(entry.name);
                    continue;
                }
            };
            let per_volume = model
                .protocol_schema()
                .iter()
                .any(|p| matches!(p.scope, Scope::PerVolume));
            let declared = !(entry.effective)(&v, &Protocol::default())
                .unwrap()
                .protocol_keys
                .is_empty();
            assert_eq!(
                per_volume, declared,
                "{}: per-volume protocol params and PROTOCOL_KEYS must agree",
                entry.name
            );
        }
        assert!(
            skipped.is_empty(),
            "describe still failed after seeding declared enums, so per-volume \
             coverage for these models went unchecked: {skipped:?}"
        );
    }
}
