//! Layer 1: scan a dataset into a flat table of image rows, each paired with
//! its JSON sidecar. Reads only through `DatasetFs`.

use crate::entities::parse_filename;
use crate::fs::DatasetFs;
use anyhow::Result;
use std::collections::BTreeMap;

const IMAGE_EXTS: [&str; 2] = [".nii", ".nii.gz"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BidsRow {
    pub path: String,
    pub datatype: Option<String>,
    pub suffix: String,
    pub extension: String,
    pub entities: BTreeMap<String, String>,
    pub sidecar_path: Option<String>,
}

fn stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.find('.') {
        Some(i) => &file[..i],
        None => file,
    }
}

fn is_image(name: &str) -> bool {
    IMAGE_EXTS.iter().any(|e| name.ends_with(e))
}

/// Recursively collect every file path under `dir`.
fn walk<F: DatasetFs>(fs: &F, dir: &str, out: &mut Vec<String>) -> Result<()> {
    for e in fs.list(dir)? {
        let child = if dir.is_empty() {
            e.name.clone()
        } else {
            format!("{dir}/{}", e.name)
        };
        if e.is_dir {
            walk(fs, &child, out)?;
        } else {
            out.push(child);
        }
    }
    Ok(())
}

/// The datatype directory is the immediate parent folder (anat/fmap/dwi/...).
fn datatype_of(path: &str) -> Option<String> {
    let mut segs: Vec<&str> = path.split('/').collect();
    segs.pop(); // filename
    segs.pop().map(|s| s.to_string())
}

pub fn parse_to_table<F: DatasetFs>(fs: &F) -> Result<Vec<BidsRow>> {
    // .bidsignore: newline-separated substrings (minimal glob: `*` = any).
    let ignore: Vec<String> = match fs.read(".bidsignore") {
        Ok(bytes) => String::from_utf8_lossy(&bytes)
            .lines()
            .map(|l| l.trim().trim_end_matches('*').to_string())
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect(),
        Err(_) => Vec::new(),
    };

    let mut all = Vec::new();
    walk(fs, "", &mut all)?;
    let ignored = |p: &str| ignore.iter().any(|frag| p.contains(frag.as_str()));

    // Index sidecars by stem for pairing.
    let json_by_stem: BTreeMap<&str, &String> = all
        .iter()
        .filter(|p| p.ends_with(".json") && !ignored(p))
        .map(|p| (stem(p), p))
        .collect();

    let mut rows = Vec::new();
    for path in all.iter().filter(|p| is_image(p) && !ignored(p)) {
        let file = path.rsplit('/').next().unwrap_or(path);
        let Some(parsed) = parse_filename(file) else {
            continue;
        };
        rows.push(BidsRow {
            path: path.clone(),
            datatype: datatype_of(path),
            suffix: parsed.suffix,
            extension: parsed.extension,
            entities: parsed.entities,
            sidecar_path: json_by_stem.get(stem(path)).map(|p| (*p).clone()),
        });
    }
    rows.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemFs;

    #[test]
    fn tables_irt1_series_with_sidecars() {
        let mut fs = MemFs::new();
        for i in 1..=4 {
            fs = fs
                .touch(&format!("sub-01/anat/sub-01_inv-0{i}_IRT1.nii.gz"))
                .with(
                    &format!("sub-01/anat/sub-01_inv-0{i}_IRT1.json"),
                    format!("{{\"InversionTime\": {}}}", i * 100),
                );
        }
        let rows = parse_to_table(&fs).unwrap();
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|r| r.suffix == "IRT1"));
        assert!(rows.iter().all(|r| r.sidecar_path.is_some()));
        assert_eq!(rows[0].datatype.as_deref(), Some("anat"));
    }

    #[test]
    fn respects_bidsignore() {
        let fs = MemFs::new()
            .touch("sub-01/anat/sub-01_inv-01_IRT1.nii.gz")
            .touch("derivatives/tool/sub-01/anat/sub-01_T1map.nii.gz")
            .with(".bidsignore", b"derivatives/*".to_vec());
        let rows = parse_to_table(&fs).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].suffix, "IRT1");
    }
}
