//! Per-image sidecar metadata: a full JSON capture merged along the BIDS
//! directory-inheritance chain, shallow-merged so more-specific files win.
//!
//! Simplification: inheritance is resolved as a directory-chain merge (walk
//! from the dataset root down to the image's own directory — root -> sub ->
//! [ses] -> datatype), not full entity-powerset matching against every
//! ancestor directory in the dataset. At each level along that chain we
//! accept `.json` files whose parsed entities are a subset of the image's
//! entities and whose suffix matches the image's suffix; this covers both
//! bare `<suffix>.json` inherited files and partially-qualified ones (e.g.
//! `task-rest_bold.json`). Candidates within one directory are merged from
//! least to most entity-specific, and the image's own co-located sidecar is
//! re-applied last so it always wins regardless of directory-listing order.
//! This is sufficient for the shipped qMRI suffixes, which never need
//! sideways matching (e.g. a `sub-02` file influencing `sub-01`'s metadata).

use crate::entities::parse_filename;
use crate::fs::DatasetFs;
use anyhow::{Context, Result};
use serde_json::{Map, Value};

/// A merged, full-metadata JSON view of one image's sidecar chain.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sidecar(Map<String, Value>);

impl Sidecar {
    /// The raw JSON value for `k`, if present.
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.0.get(k)
    }

    /// Whether `k` is present (regardless of its value's type).
    pub fn contains(&self, k: &str) -> bool {
        self.0.contains_key(k)
    }

    /// `k` as an `f64`; `None` if absent or not a JSON number.
    pub fn f64(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(Value::as_f64)
    }

    /// `k` as a string slice; `None` if absent or not a JSON string.
    pub fn str(&self, k: &str) -> Option<&str> {
        self.get(k).and_then(Value::as_str)
    }

    /// `k` as a JSON array slice; `None` if absent or not a JSON array.
    pub fn array(&self, k: &str) -> Option<&[Value]> {
        self.get(k).and_then(Value::as_array).map(Vec::as_slice)
    }

    fn merge(&mut self, other: Map<String, Value>) {
        for (k, v) in other {
            self.0.insert(k, v);
        }
    }
}

const IMAGE_EXTS: [&str; 2] = [".nii.gz", ".nii"];

fn strip_image_ext(path: &str) -> &str {
    for ext in IMAGE_EXTS {
        if let Some(s) = path.strip_suffix(ext) {
            return s;
        }
    }
    path
}

/// Read a sidecar file and return its top-level JSON object. A malformed
/// (present but unparsable, or not a JSON object) file is an error — only a
/// *missing* file is treated as "skip" by callers.
fn read_json_object<F: DatasetFs>(fs: &F, path: &str) -> Result<Map<String, Value>> {
    let bytes = fs.read(path).with_context(|| format!("reading {path}"))?;
    let value: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("parsing {path} as JSON"))?;
    match value {
        Value::Object(map) => Ok(map),
        other => anyhow::bail!("{path}: expected a JSON object, got {other}"),
    }
}

/// Every path prefix from the dataset root ("") down to and including `dir`,
/// e.g. `"sub-01/anat"` -> `["", "sub-01", "sub-01/anat"]`.
fn directory_chain(dir: &str) -> Vec<String> {
    let mut levels = vec![String::new()];
    if dir.is_empty() {
        return levels;
    }
    let mut acc = String::new();
    for seg in dir.split('/') {
        acc = if acc.is_empty() {
            seg.to_string()
        } else {
            format!("{acc}/{seg}")
        };
        levels.push(acc.clone());
    }
    levels
}

/// Build the full, inheritance-merged `Sidecar` for one image.
pub fn sidecar_for<F: DatasetFs>(fs: &F, nii_rel_path: &str) -> Result<Sidecar> {
    let dir = nii_rel_path.rsplit_once('/').map_or("", |(d, _)| d);
    let file = nii_rel_path.rsplit('/').next().unwrap_or(nii_rel_path);
    let image = parse_filename(file).context("image filename is not valid BIDS")?;

    let mut sidecar = Sidecar::default();
    for level in directory_chain(dir) {
        let mut candidates: Vec<(usize, String)> = Vec::new();
        for entry in fs.list(&level)? {
            if entry.is_dir || !entry.name.ends_with(".json") {
                continue;
            }
            let Some(parsed) = parse_filename(&entry.name) else {
                continue;
            };
            if parsed.suffix != image.suffix {
                continue;
            }
            let is_subset = parsed
                .entities
                .iter()
                .all(|(k, v)| image.entities.get(k) == Some(v));
            if !is_subset {
                continue;
            }
            let path = if level.is_empty() {
                entry.name.clone()
            } else {
                format!("{level}/{}", entry.name)
            };
            candidates.push((parsed.entities.len(), path));
        }
        // Fewer-entity (less specific) files first, so a more-specific
        // inherited file at the same directory level wins ties.
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        for (_, path) in candidates {
            sidecar.merge(read_json_object(fs, &path)?);
        }
    }

    // The co-located sidecar is already picked up by the scan above (it
    // lives in the image's own, last-visited directory and its entities are
    // trivially a subset of the image's), but re-apply it explicitly so it
    // always wins regardless of that scan's tie-break order. A missing
    // co-located file is fine (many raw images have none); malformed JSON is
    // still an error.
    let co_located = format!("{}.json", strip_image_ext(nii_rel_path));
    if let Ok(bytes) = fs.read(&co_located) {
        let value: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing {co_located} as JSON"))?;
        match value {
            Value::Object(map) => sidecar.merge(map),
            other => anyhow::bail!("{co_located}: expected a JSON object, got {other}"),
        }
    }

    Ok(sidecar)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemFs;

    #[test]
    fn reads_co_located_sidecar() {
        let fs = MemFs::new().with(
            "sub-01/anat/sub-01_inv-01_IRT1.json",
            br#"{"InversionTime":30,"FlipAngle":4,"PartialFourier":"0.75"}"#.to_vec(),
        );
        let sc = sidecar_for(&fs, "sub-01/anat/sub-01_inv-01_IRT1.nii.gz").unwrap();
        assert_eq!(sc.f64("InversionTime"), Some(30.0));
        assert_eq!(sc.f64("FlipAngle"), Some(4.0));
        assert_eq!(sc.str("PartialFourier"), Some("0.75"));
        assert_eq!(sc.f64("PartialFourier"), None); // string, not a number
    }

    #[test]
    fn merges_inherited_root_sidecar_with_co_located_winning_ties() {
        let fs = MemFs::new()
            .with(
                "IRT1.json",
                br#"{"RepetitionTimeExcitation":15,"FlipAngle":9}"#.to_vec(),
            )
            .with(
                "sub-01/anat/sub-01_inv-01_IRT1.json",
                br#"{"InversionTime":30,"FlipAngle":4}"#.to_vec(),
            );
        let sc = sidecar_for(&fs, "sub-01/anat/sub-01_inv-01_IRT1.nii.gz").unwrap();
        assert_eq!(sc.f64("RepetitionTimeExcitation"), Some(15.0));
        assert_eq!(sc.f64("InversionTime"), Some(30.0));
        // Present in both root and co-located; co-located (more specific) wins.
        assert_eq!(sc.f64("FlipAngle"), Some(4.0));
    }

    #[test]
    fn array_accessor_returns_slice() {
        let fs = MemFs::new().with(
            "sub-01/anat/sub-01_inv-01_IRT1.json",
            br#"{"ReconMatrixPE":[128,128]}"#.to_vec(),
        );
        let sc = sidecar_for(&fs, "sub-01/anat/sub-01_inv-01_IRT1.nii.gz").unwrap();
        let arr = sc.array("ReconMatrixPE").unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_f64(), Some(128.0));
        assert_eq!(arr[1].as_f64(), Some(128.0));
    }

    #[test]
    fn missing_sidecar_is_empty_not_an_error() {
        let fs = MemFs::new().touch("sub-01/anat/sub-01_inv-01_IRT1.nii.gz");
        let sc = sidecar_for(&fs, "sub-01/anat/sub-01_inv-01_IRT1.nii.gz").unwrap();
        assert!(!sc.contains("InversionTime"));
    }

    #[test]
    fn malformed_json_is_an_error() {
        let fs = MemFs::new().with(
            "sub-01/anat/sub-01_inv-01_IRT1.json",
            b"{not valid json".to_vec(),
        );
        assert!(sidecar_for(&fs, "sub-01/anat/sub-01_inv-01_IRT1.nii.gz").is_err());
    }
}
