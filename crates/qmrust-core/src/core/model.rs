//! The single contributor surface: the object-safe [`Model`] trait plus the
//! value types the shell uses to drive it. Nothing here touches I/O or
//! config-file formats — this is the functional-core boundary.

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
    /// Number of acquisition volumes the model's protocol expects.
    fn n_acquisitions(&self) -> usize;
    /// Fit granularity. Defaults to voxelwise.
    fn strategy(&self) -> FitStrategy {
        FitStrategy::Voxelwise
    }
    /// Noise-free forward signal for `params`, protocol-aligned.
    fn forward(&self, params: &[f64], aux: &Aux) -> Vec<f64>;
    /// Fit a signal, returning values in `output_names` order.
    fn fit(&self, signal: &[f64], aux: &Aux) -> Vec<f64>;
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
}
