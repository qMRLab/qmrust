//! Bridge resolved collections to `qmrust_core::Protocol` by reading numeric
//! fields out of the JSON sidecars. Keeps rust-bids the sole place that knows
//! how BIDS metadata maps onto the protocol axis.

use crate::collection::{Collection, GroupedData};
use crate::fs::DatasetFs;
use crate::sidecar::{sidecar_for, Sidecar};
use anyhow::{bail, Context, Result};
use qmrust_core::core::model::{Meta, ProtoParam, Protocol, Scope, Source};
use serde_json::Value;
use std::collections::BTreeMap;

impl Meta for Sidecar {
    fn f64(&self, k: &str) -> Option<f64> {
        Sidecar::f64(self, k)
    }
    fn str(&self, k: &str) -> Option<&str> {
        Sidecar::str(self, k)
    }
    fn array(&self, k: &str) -> Option<&[Value]> {
        Sidecar::array(self, k)
    }
}

fn ordered_sidecars(c: &Collection) -> Vec<Option<&String>> {
    match &c.data {
        GroupedData::Sequential(vols) => vols.iter().map(|v| v.json.as_ref()).collect(),
        GroupedData::Named(groups) => groups.values().map(|v| v.json.as_ref()).collect(),
    }
}

/// Each volume's `.nii` path, in the same order `ordered_sidecars` walks the
/// collection, so a schema evaluation can build the matching full `Sidecar`
/// (inheritance-resolved) rather than just the co-located JSON.
fn ordered_nii_paths(c: &Collection) -> Vec<&str> {
    match &c.data {
        GroupedData::Sequential(vols) => vols.iter().map(|v| v.nii.as_str()).collect(),
        GroupedData::Named(groups) => groups.values().map(|v| v.nii.as_str()).collect(),
    }
}

/// Evaluate one [`ProtoParam`]'s source against a volume's `Sidecar` (or the
/// caller-supplied `options` for `Source::Option`). `None` means "could not
/// resolve" — the caller decides whether that's fatal.
fn eval_source(
    source: &Source,
    sidecar: &Sidecar,
    options: &BTreeMap<String, f64>,
) -> Result<Option<f64>> {
    match source {
        Source::Field(k) => Ok(Meta::f64(sidecar, k)),
        Source::Derived(f) => Ok(Some(f(sidecar as &dyn Meta)?)),
        Source::Option(k) => Ok(options.get(*k).copied()),
    }
}

/// Resolve a `Protocol` from a `schema` of [`ProtoParam`]s against `c`'s
/// sidecars (per-volume, inheritance-resolved via [`sidecar_for`]) and the
/// supplied `options` (the non-BIDS fallback for `Source::Option` params).
///
/// `Scope::PerVolume` params are evaluated once per volume, in the same
/// order `ordered_sidecars`/`ordered_nii_paths` walk the collection.
/// `Scope::Global` params are evaluated once, against the *first* volume's
/// sidecar (global fields are expected to be dataset-wide and hence
/// consistent across volumes); an empty collection makes any `Global` param
/// unresolvable. A param that cannot be resolved is a hard error naming the
/// param and (for `PerVolume`) the offending volume's path — a silently
/// missing value would otherwise surface only as a per-voxel fit failure.
pub fn resolve_protocol<F: DatasetFs>(
    fs: &F,
    c: &Collection,
    schema: &[ProtoParam],
    options: &BTreeMap<String, f64>,
) -> Result<Protocol> {
    let nii_paths = ordered_nii_paths(c);
    let mut proto = Protocol::default();

    let mut first_sidecar: Option<Sidecar> = None;
    for path in &nii_paths {
        let sidecar =
            sidecar_for(fs, path).with_context(|| format!("resolving sidecar for {path}"))?;
        let mut vol = BTreeMap::new();
        for param in schema
            .iter()
            .filter(|p| matches!(p.scope, Scope::PerVolume))
        {
            let value = eval_source(&param.source, &sidecar, options)?;
            match value {
                Some(v) => {
                    vol.insert(param.name.to_string(), v);
                }
                None => bail!(
                    "protocol param '{}' could not be resolved for volume '{}'",
                    param.name,
                    path
                ),
            }
        }
        proto.volumes.push(vol);
        if first_sidecar.is_none() {
            first_sidecar = Some(sidecar);
        }
    }

    for param in schema.iter().filter(|p| matches!(p.scope, Scope::Global)) {
        let sidecar = first_sidecar.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "protocol param '{}' is global but the collection has no volumes",
                param.name
            )
        })?;
        let value = eval_source(&param.source, sidecar, options)?;
        match value {
            Some(v) => {
                proto.global.insert(param.name.to_string(), v);
            }
            None => bail!(
                "protocol param '{}' (global) could not be resolved",
                param.name
            ),
        }
    }

    Ok(proto)
}

/// Superseded by [`resolve_protocol`], which resolves against a declarative
/// [`ProtoParam`] schema and the full inheritance-resolved `Sidecar` rather
/// than a bare key list read off the co-located JSON only. Kept (delegating
/// where the semantics line up) for its one remaining caller; retire once
/// that caller moves to `resolve_protocol` directly.
///
/// Deliberately NOT reimplemented in terms of `resolve_protocol`: unlike
/// `resolve_protocol`'s `Source::Field`, a missing key/sidecar here is
/// silently skipped rather than an error, and only the co-located sidecar
/// (not the full inheritance chain) is consulted — delegating would change
/// both behaviors for its existing caller.
pub fn protocol_for<F: DatasetFs>(fs: &F, c: &Collection, keys: &[&str]) -> Result<Protocol> {
    let mut proto = Protocol::default();
    for sidecar in ordered_sidecars(c) {
        let mut vol = BTreeMap::new();
        if let Some(path) = sidecar {
            let bytes = fs.read(path)?;
            let json: Value = serde_json::from_slice(&bytes)?;
            for k in keys {
                if let Some(n) = json.get(*k).and_then(|v| v.as_f64()) {
                    vol.insert((*k).to_string(), n);
                }
            }
        }
        proto.volumes.push(vol);
    }
    Ok(proto)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::{GroupedData, VolumeRef};
    use crate::fs::MemFs;

    #[test]
    fn reads_inversion_times_in_order() {
        let fs = MemFs::new()
            .with("a_inv-01_IRT1.json", br#"{"InversionTime": 30}"#.to_vec())
            .with("a_inv-02_IRT1.json", br#"{"InversionTime": 530}"#.to_vec());
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "IRT1".into(),
            data: GroupedData::Sequential(vec![
                VolumeRef {
                    nii: "a_inv-01_IRT1.nii.gz".into(),
                    json: Some("a_inv-01_IRT1.json".into()),
                },
                VolumeRef {
                    nii: "a_inv-02_IRT1.nii.gz".into(),
                    json: Some("a_inv-02_IRT1.json".into()),
                },
            ]),
            warnings: vec![],
        };
        let proto = protocol_for(&fs, &c, &["InversionTime"]).unwrap();
        assert_eq!(proto.volumes.len(), 2);
        assert_eq!(proto.volumes[0].get("InversionTime"), Some(&30.0));
        assert_eq!(proto.volumes[1].get("InversionTime"), Some(&530.0));
    }

    /// `GroupedData::Named` is backed by a `BTreeMap<String, VolumeRef>`, so
    /// `ordered_sidecars` (and hence `protocol_for`) iterates groups in
    /// alphabetical-by-group-name order — NOT the order a model's config
    /// declares in `required`. Here "MTw" < "PDw" < "T1w" alphabetically,
    /// which happens to differ from a typical qMT-style `required` order of
    /// ["T1w", "PDw", "MTw"]. This test pins down and documents that current
    /// behavior: the fitting-integration layer is responsible for
    /// re-ordering `Protocol.volumes` to match a model's `required` order
    /// before feeding it to a model — `protocol_for` itself does not do so.
    #[test]
    fn protocol_for_named_collection_orders_alphabetically_by_group_name() {
        let fs = MemFs::new()
            .with("a_MTw.json", br#"{"FlipAngle": 3}"#.to_vec())
            .with("a_PDw.json", br#"{"FlipAngle": 6}"#.to_vec())
            .with("a_T1w.json", br#"{"FlipAngle": 20}"#.to_vec());
        let mut groups = BTreeMap::new();
        groups.insert(
            "PDw".to_string(),
            VolumeRef {
                nii: "a_PDw.nii.gz".into(),
                json: Some("a_PDw.json".into()),
            },
        );
        groups.insert(
            "MTw".to_string(),
            VolumeRef {
                nii: "a_MTw.nii.gz".into(),
                json: Some("a_MTw.json".into()),
            },
        );
        groups.insert(
            "T1w".to_string(),
            VolumeRef {
                nii: "a_T1w.nii.gz".into(),
                json: Some("a_T1w.json".into()),
            },
        );
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "MTS".into(),
            data: GroupedData::Named(groups),
            warnings: vec![],
        };
        let proto = protocol_for(&fs, &c, &["FlipAngle"]).unwrap();
        assert_eq!(proto.volumes.len(), 3);
        // BTreeMap alphabetical order: MTw, PDw, T1w — not the "T1w, PDw, MTw"
        // order a qMT-style config's `required` list would typically use.
        assert_eq!(proto.volumes[0].get("FlipAngle"), Some(&3.0)); // MTw
        assert_eq!(proto.volumes[1].get("FlipAngle"), Some(&6.0)); // PDw
        assert_eq!(proto.volumes[2].get("FlipAngle"), Some(&20.0)); // T1w
    }

    /// Adds a co-located sidecar for one IRT1 volume to `fs` and returns its
    /// `VolumeRef`; `fs` is threaded through the builder-style `MemFs` API.
    fn with_irt1_volume(fs: MemFs, sub: &str, inv: &str, ti: f64) -> (MemFs, VolumeRef) {
        let base = format!("sub-{sub}/anat/sub-{sub}_inv-{inv}_IRT1");
        let json_path = format!("{base}.json");
        let fs = fs.with(&json_path, format!(r#"{{"InversionTime": {ti}}}"#));
        let vol = VolumeRef {
            nii: format!("{base}.nii.gz"),
            json: Some(json_path),
        };
        (fs, vol)
    }

    fn ir_schema() -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "InversionTime",
            source: Source::Field("InversionTime"),
            scope: Scope::PerVolume,
        }]
    }

    #[test]
    fn resolve_protocol_field_reads_inversion_times_in_order() {
        let tis = [30.0, 530.0, 1030.0, 1530.0];
        let mut fs = MemFs::new();
        let mut vols = Vec::new();
        for (i, &ti) in tis.iter().enumerate() {
            let (next_fs, vol) = with_irt1_volume(fs, "01", &format!("{:02}", i + 1), ti);
            fs = next_fs;
            vols.push(vol);
        }
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "IRT1".into(),
            data: GroupedData::Sequential(vols),
            warnings: vec![],
        };
        let proto = resolve_protocol(&fs, &c, &ir_schema(), &BTreeMap::new()).unwrap();
        assert_eq!(proto.volumes.len(), 4);
        for (vol, &ti) in proto.volumes.iter().zip(tis.iter()) {
            assert_eq!(vol.get("InversionTime"), Some(&ti));
        }
    }

    fn derived_schema() -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "prod",
            source: Source::Derived(|m| {
                let a = m.f64("A").ok_or_else(|| anyhow::anyhow!("A"))?;
                let b = m.f64("B").ok_or_else(|| anyhow::anyhow!("B"))?;
                Ok(a * b)
            }),
            scope: Scope::PerVolume,
        }]
    }

    #[test]
    fn resolve_protocol_derived_combines_two_sidecar_fields() {
        let fs = MemFs::new()
            .with(
                "sub-01/anat/sub-01_inv-01_IRT1.json",
                br#"{"A": 3, "B": 4}"#.to_vec(),
            )
            .with(
                "sub-01/anat/sub-01_inv-02_IRT1.json",
                br#"{"A": 5, "B": 6}"#.to_vec(),
            );
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "IRT1".into(),
            data: GroupedData::Sequential(vec![
                VolumeRef {
                    nii: "sub-01/anat/sub-01_inv-01_IRT1.nii.gz".into(),
                    json: Some("sub-01/anat/sub-01_inv-01_IRT1.json".into()),
                },
                VolumeRef {
                    nii: "sub-01/anat/sub-01_inv-02_IRT1.nii.gz".into(),
                    json: Some("sub-01/anat/sub-01_inv-02_IRT1.json".into()),
                },
            ]),
            warnings: vec![],
        };
        let proto = resolve_protocol(&fs, &c, &derived_schema(), &BTreeMap::new()).unwrap();
        assert_eq!(proto.volumes.len(), 2);
        assert_eq!(proto.volumes[0].get("prod"), Some(&12.0));
        assert_eq!(proto.volumes[1].get("prod"), Some(&30.0));
    }

    #[test]
    fn resolve_protocol_option_reads_from_options_map() {
        let (fs, vol) = with_irt1_volume(MemFs::new(), "01", "01", 30.0);
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "IRT1".into(),
            data: GroupedData::Sequential(vec![vol]),
            warnings: vec![],
        };
        let schema = vec![ProtoParam {
            name: "x",
            source: Source::Option("x"),
            scope: Scope::PerVolume,
        }];
        let mut options = BTreeMap::new();
        options.insert("x".to_string(), 42.0);
        let proto = resolve_protocol(&fs, &c, &schema, &options).unwrap();
        assert_eq!(proto.volumes[0].get("x"), Some(&42.0));
    }

    #[test]
    fn resolve_protocol_errors_naming_param_and_volume_when_field_missing() {
        // No InversionTime in this sidecar.
        let fs = MemFs::new().with(
            "sub-01/anat/sub-01_inv-01_IRT1.json",
            br#"{"FlipAngle": 9}"#.to_vec(),
        );
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "IRT1".into(),
            data: GroupedData::Sequential(vec![VolumeRef {
                nii: "sub-01/anat/sub-01_inv-01_IRT1.nii.gz".into(),
                json: Some("sub-01/anat/sub-01_inv-01_IRT1.json".into()),
            }]),
            warnings: vec![],
        };
        let err = resolve_protocol(&fs, &c, &ir_schema(), &BTreeMap::new()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("InversionTime"), "{msg}");
        assert!(
            msg.contains("sub-01/anat/sub-01_inv-01_IRT1.nii.gz"),
            "{msg}"
        );
    }
}
