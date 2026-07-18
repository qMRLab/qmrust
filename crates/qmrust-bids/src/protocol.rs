//! Bridge resolved collections to `qmrust_core::Protocol` by reading numeric
//! fields out of the JSON sidecars. Keeps qmrust-bids the sole place that knows
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
}
