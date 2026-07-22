//! Resolve an acquisition [`Protocol`] from a source: `Yaml` (a model reads its
//! protocol from its own config) or `Mat` (a `.mat` file supplies acquisition
//! parameters). BIDS-sidecar protocols are produced by the separate `rust-bids`
//! crate.

use crate::core::model::Protocol;
use std::collections::BTreeMap;

/// Where the protocol comes from.
pub enum ProtocolSource {
    /// Model reads its protocol from its own YAML config sub-tree. Yields an
    /// empty `Protocol`.
    Yaml,
    /// `.mat` input supplied acquisition parameters (currently IR TI values).
    Mat { inversion_times: Option<Vec<f64>> },
}

/// Build a `Protocol` from a source.
pub fn resolve(src: ProtocolSource) -> Protocol {
    match src {
        ProtocolSource::Yaml => Protocol::default(),
        ProtocolSource::Mat { inversion_times } => {
            let mut p = Protocol::default();
            if let Some(tis) = inversion_times {
                p.volumes = tis
                    .into_iter()
                    .map(|ti| {
                        let mut m = BTreeMap::new();
                        m.insert("InversionTime".to_string(), ti);
                        m
                    })
                    .collect();
            }
            p
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_source_is_empty_protocol() {
        let p = resolve(ProtocolSource::Yaml);
        assert!(p.is_empty());
    }

    #[test]
    fn mat_source_carries_inversion_times() {
        let p = resolve(ProtocolSource::Mat {
            inversion_times: Some(vec![350.0, 500.0, 650.0]),
        });
        assert_eq!(p.volumes.len(), 3);
        assert_eq!(p.volumes[0].get("InversionTime"), Some(&350.0));
        assert_eq!(p.volumes[2].get("InversionTime"), Some(&650.0));
    }
}
