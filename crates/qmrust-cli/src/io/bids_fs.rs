use anyhow::Result;
use rust_bids::{DatasetFs, Entry};
use std::path::PathBuf;

/// Native filesystem feeder for `rust-bids` (the browser supplies its own).
/// Not yet wired into a CLI command — that lands with `--bids-dir` in a later task.
#[allow(dead_code)]
pub struct StdFs {
    pub root: PathBuf,
}

impl DatasetFs for StdFs {
    fn list(&self, rel_dir: &str) -> Result<Vec<Entry>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(self.root.join(rel_dir))? {
            let e = e?;
            out.push(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                is_dir: e.file_type()?.is_dir(),
            });
        }
        Ok(out)
    }
    fn read(&self, rel_path: &str) -> Result<Vec<u8>> {
        Ok(std::fs::read(self.root.join(rel_path))?)
    }
}
