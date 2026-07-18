//! Bridge resolved collections to `qmrust_core::Protocol` by reading numeric
//! fields out of the JSON sidecars. Keeps rust-bids the sole place that knows
//! how BIDS metadata maps onto the protocol axis.

use crate::collection::{Collection, GroupedData};
use crate::fs::DatasetFs;
use anyhow::Result;
use qmrust_core::core::model::Protocol;
use serde_json::Value;
use std::collections::BTreeMap;

fn ordered_sidecars(c: &Collection) -> Vec<Option<&String>> {
    match &c.data {
        GroupedData::Sequential(vols) => vols.iter().map(|v| v.json.as_ref()).collect(),
        GroupedData::Named(groups) => groups.values().map(|v| v.json.as_ref()).collect(),
    }
}

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
}
