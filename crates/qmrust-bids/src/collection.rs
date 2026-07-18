//! Resolved collections + serialization to bids2nf's `*_unified.json` shape
//! (used by the differential oracle tests).

use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeRef {
    pub nii: String,
    pub json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupedData {
    Sequential(Vec<VolumeRef>),
    Named(BTreeMap<String, VolumeRef>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub subject: String,
    pub session: Option<String>,
    pub run: Option<String>,
    pub task: Option<String>,
    pub suffix: String,
    pub data: GroupedData,
    pub warnings: Vec<Warning>,
}

fn na(opt: &Option<String>, prefix: &str) -> String {
    match opt {
        Some(v) => {
            if v.starts_with(prefix) {
                v.clone()
            } else {
                format!("{prefix}-{v}")
            }
        }
        None => "NA".to_string(),
    }
}

impl Collection {
    /// Serialize to the bids2nf unified shape: `{subject, session, run, task, data}`.
    pub fn to_unified_json(&self) -> Value {
        let data_body = match &self.data {
            GroupedData::Sequential(vols) => {
                let nii: Vec<Value> = vols.iter().map(|v| json!(v.nii)).collect();
                let jsn: Vec<Value> = vols
                    .iter()
                    .filter_map(|v| v.json.clone())
                    .map(|p| json!(p))
                    .collect();
                json!({ "nii": nii, "json": jsn })
            }
            GroupedData::Named(groups) => {
                let mut m = serde_json::Map::new();
                for (name, v) in groups {
                    let mut inner = serde_json::Map::new();
                    inner.insert("nii".into(), json!(v.nii));
                    if let Some(j) = &v.json {
                        inner.insert("json".into(), json!(j));
                    }
                    m.insert(name.clone(), Value::Object(inner));
                }
                Value::Object(m)
            }
        };
        json!({
            "subject": na(&Some(self.subject.clone()), "sub"),
            "session": na(&self.session, "ses"),
            "run": na(&self.run, "run"),
            "task": na(&self.task, "task"),
            "data": { self.suffix.clone(): data_body },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_unified_shape_matches_bids2nf() {
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
        let v = c.to_unified_json();
        assert_eq!(v["subject"], "sub-01");
        assert_eq!(v["session"], "NA");
        assert_eq!(v["data"]["IRT1"]["nii"].as_array().unwrap().len(), 2);
    }
}
