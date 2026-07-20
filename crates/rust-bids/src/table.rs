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

/// Directory-qualified stem: the full path with its known extension removed
/// (directory segments kept intact). Used to pair an image with its sidecar
/// *in the same directory* — two files with the same basename but living in
/// different directories (e.g. a raw file and its mirror under
/// `derivatives/<pipeline>/...`) must NOT collide on a bare-filename stem.
fn path_stem(path: &str) -> &str {
    for ext in [".nii.gz", ".nii", ".json"] {
        if let Some(s) = path.strip_suffix(ext) {
            return s;
        }
    }
    path
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
    // Only a *trailing* `*` is stripped; leading-wildcard lines like `*.log`
    // are left as-is (and thus effectively inert against the `contains`
    // check below) — this matches the brief's minimal-glob scope.
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
    // A path whose filename parses to a *registered model suffix* is exempt
    // from `.bidsignore`: custom (non-standard-BIDS) suffixes like QMTSPGR are
    // deliberately `.bidsignore`'d so generic BIDS validators don't choke on
    // them, but they must still be discoverable by qmrust itself.
    let is_registered_suffix = |p: &str| {
        let file = p.rsplit('/').next().unwrap_or(p);
        parse_filename(file)
            .is_some_and(|parsed| qmrust_core::registry::by_bids_suffix(&parsed.suffix).is_some())
    };
    let ignored =
        |p: &str| !is_registered_suffix(p) && ignore.iter().any(|frag| p.contains(frag.as_str()));

    // Index sidecars by directory-qualified stem for pairing, so a raw file
    // never pairs with a same-named sidecar living under a different
    // directory (e.g. a `derivatives/<pipeline>/...` mirror of the raw tree).
    let json_by_stem: BTreeMap<&str, &String> = all
        .iter()
        .filter(|p| p.ends_with(".json") && !ignored(p))
        .map(|p| (path_stem(p), p))
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
            sidecar_path: json_by_stem.get(path_stem(path)).map(|p| (*p).clone()),
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
    fn does_not_cross_pair_sidecars_across_directories() {
        // Same basename, two different directories: a raw file + co-located
        // sidecar, and an unrelated sidecar under a derivatives mirror with
        // the identical filename. The raw image must pair with the raw
        // (co-located) sidecar only, never the derivatives one.
        let fs = MemFs::new()
            .touch("sub-01/anat/sub-01_inv-01_IRT1.nii.gz")
            .with(
                "sub-01/anat/sub-01_inv-01_IRT1.json",
                b"{\"InversionTime\": 100}".to_vec(),
            )
            .with(
                "derivatives/toolX/sub-01/anat/sub-01_inv-01_IRT1.json",
                b"{\"InversionTime\": 999}".to_vec(),
            );
        let rows = parse_to_table(&fs).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].sidecar_path.as_deref(),
            Some("sub-01/anat/sub-01_inv-01_IRT1.json")
        );
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

    #[test]
    fn bidsignore_exempts_registered_model_suffixes() {
        // QMTSPGR is a registered model suffix (qmrust_core::registry), so a
        // `.bidsignore` line targeting it must NOT hide it from discovery —
        // only unrelated, non-registered ignored paths stay excluded.
        let fs = MemFs::new()
            .touch("sub-02/anat/sub-02_flip-1_mt-1_QMTSPGR.nii.gz")
            .with(
                "sub-02/anat/sub-02_flip-1_mt-1_QMTSPGR.json",
                b"{}".to_vec(),
            )
            // Not a registered suffix ("NOTREG" isn't in qmrust_core::registry)
            // and IS covered by a `.bidsignore` line — must stay excluded.
            .touch("derivatives/tool/sub-02/anat/sub-02_NOTREG.nii.gz")
            .with(".bidsignore", b"*QMTSPGR*\nderivatives/*".to_vec());
        let rows = parse_to_table(&fs).unwrap();
        assert_eq!(rows.len(), 1, "only the QMTSPGR volume should surface");
        assert_eq!(rows[0].suffix, "QMTSPGR");
        assert!(
            rows[0].sidecar_path.is_some(),
            "sidecar must also be exempted so pairing still works"
        );
    }
}
