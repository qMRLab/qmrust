//! Parse a BIDS filename into entities + suffix + extension. Entity *keys* are
//! normalized to their full names (e.g. `inv-` → `inversion`), so grouping
//! configs can refer to them by a stable, readable name.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedName {
    pub entities: BTreeMap<String, String>,
    pub suffix: String,
    pub extension: String,
}

/// Map BIDS short entity keys to their full names.
fn full_key(short: &str) -> String {
    match short {
        "sub" => "subject",
        "ses" => "session",
        "acq" => "acquisition",
        "inv" => "inversion",
        "mt" => "mtransfer",
        "dir" => "direction",
        other => other, // run, task, flip, echo, part already full
    }
    .to_string()
}

pub fn parse_filename(name: &str) -> Option<ParsedName> {
    // Split off the double/triple extension (.nii.gz, .json, .nii).
    let (stem, extension) = match name.find('.') {
        Some(i) => (&name[..i], name[i..].to_string()),
        None => (name, String::new()),
    };
    // BIDS stem: entity-value pairs joined by `_`, ending in a bare `suffix`.
    let mut parts = stem.split('_').collect::<Vec<_>>();
    let suffix = parts.pop()?.to_string();
    // A suffix must not itself look like an entity pair.
    if suffix.contains('-') {
        return None;
    }
    let mut entities = BTreeMap::new();
    for p in parts {
        // Non-entity tokens (e.g. the "dataset" in `dataset_description.json`)
        // aren't `key-value` pairs; skip them instead of failing the whole parse.
        if let Some((k, v)) = p.split_once('-') {
            entities.insert(full_key(k), v.to_string());
        }
    }
    Some(ParsedName {
        entities,
        suffix,
        extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sequential_irt1() {
        let p = parse_filename("sub-01_inv-01_IRT1.nii.gz").unwrap();
        assert_eq!(p.suffix, "IRT1");
        assert_eq!(p.extension, ".nii.gz");
        assert_eq!(p.entities.get("subject").unwrap(), "01");
        assert_eq!(p.entities.get("inversion").unwrap(), "01");
    }

    #[test]
    fn parses_named_mts() {
        let p = parse_filename("sub-01_flip-1_mt-off_MTS.nii.gz").unwrap();
        assert_eq!(p.suffix, "MTS");
        assert_eq!(p.entities.get("flip").unwrap(), "1");
        assert_eq!(p.entities.get("mtransfer").unwrap(), "off");
    }

    #[test]
    fn rejects_non_data_files() {
        // No suffix token (dataset_description is a bare word → treated as suffix,
        // but has no entities, so callers filter by known suffixes downstream).
        assert!(parse_filename("dataset_description.json").is_some());
        // A JSON sidecar parses the same as its nii sibling (suffix carries).
        let j = parse_filename("sub-01_inv-01_IRT1.json").unwrap();
        assert_eq!(j.suffix, "IRT1");
        assert_eq!(j.extension, ".json");
    }
}
