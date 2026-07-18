//! Filesystem abstraction. The pure layers read only through `DatasetFs`;
//! the shell supplies `std::fs` (native) or File System Access API (wasm).

use anyhow::Result;

#[cfg(any(test, feature = "testfs"))]
use anyhow::anyhow;
#[cfg(any(test, feature = "testfs"))]
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
}

pub trait DatasetFs {
    /// List immediate children of `rel_dir` ("" is the dataset root).
    fn list(&self, rel_dir: &str) -> Result<Vec<Entry>>;
    /// Read a file's bytes by dataset-relative path.
    fn read(&self, rel_path: &str) -> Result<Vec<u8>>;
}

#[cfg(any(test, feature = "testfs"))]
#[derive(Default, Clone)]
pub struct MemFs {
    files: BTreeMap<String, Vec<u8>>,
}

#[cfg(any(test, feature = "testfs"))]
impl MemFs {
    pub fn new() -> Self {
        Self::default()
    }
    /// Add a file (parent dirs are implicit).
    pub fn with(mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.to_string(), bytes.into());
        self
    }
    /// Add an empty file (for filename-only fixtures like `.nii`).
    pub fn touch(self, path: &str) -> Self {
        self.with(path, Vec::new())
    }
}

#[cfg(any(test, feature = "testfs"))]
impl DatasetFs for MemFs {
    fn list(&self, rel_dir: &str) -> Result<Vec<Entry>> {
        let prefix = if rel_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", rel_dir.trim_end_matches('/'))
        };
        let mut seen = BTreeMap::new();
        for path in self.files.keys() {
            let Some(rest) = path.strip_prefix(&prefix) else {
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            match rest.split_once('/') {
                Some((dir, _)) => {
                    seen.insert(dir.to_string(), true);
                }
                None => {
                    seen.insert(rest.to_string(), false);
                }
            }
        }
        Ok(seen
            .into_iter()
            .map(|(name, is_dir)| Entry { name, is_dir })
            .collect())
    }
    fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        self.files
            .get(rel_path)
            .cloned()
            .ok_or_else(|| anyhow!("no such file: {rel_path}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memfs_lists_dirs_and_files() {
        let fs = MemFs::new()
            .touch("sub-01/anat/sub-01_inv-01_IRT1.nii.gz")
            .with("dataset_description.json", b"{}".to_vec());
        let root = fs.list("").unwrap();
        assert!(root.contains(&Entry {
            name: "sub-01".into(),
            is_dir: true
        }));
        assert!(root.contains(&Entry {
            name: "dataset_description.json".into(),
            is_dir: false
        }));
        let anat = fs.list("sub-01/anat").unwrap();
        assert_eq!(anat.len(), 1);
        assert!(!anat[0].is_dir);
    }
}
