//! Layer 2: group table rows into collections per the bids2nf grammar.

use crate::collection::{Collection, GroupedData, VolumeRef, Warning};
use crate::config::{Bids2nfConfig, SetDef};
use crate::fs::DatasetFs;
use crate::table::{parse_to_table, BidsRow};
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

/// The loop_over identity of a row (entity values, `None` when absent).
type GroupKey = Vec<Option<String>>;

fn group_key(row: &BidsRow, loop_over: &[String]) -> GroupKey {
    loop_over
        .iter()
        .map(|e| row.entities.get(e).cloned())
        .collect()
}

fn vol(row: &BidsRow) -> VolumeRef {
    VolumeRef {
        nii: row.path.clone(),
        json: row.sidecar_path.clone(),
    }
}

fn key_fields(
    key: &GroupKey,
    loop_over: &[String],
) -> (String, Option<String>, Option<String>, Option<String>) {
    let get = |name: &str| {
        loop_over
            .iter()
            .position(|e| e == name)
            .and_then(|i| key[i].clone())
    };
    (
        get("subject")
            .map(|s| format!("sub-{s}"))
            .unwrap_or_default(),
        get("session"),
        get("run"),
        get("task"),
    )
}

pub fn resolve_set(
    rows: &[BidsRow],
    cfg: &Bids2nfConfig,
    set_name: &str,
) -> Result<Vec<Collection>> {
    let def = cfg
        .sets
        .get(set_name)
        .ok_or_else(|| anyhow!("no set definition named {set_name}"))?;

    // Only rows whose suffix matches the set name participate.
    let mut by_group: BTreeMap<GroupKey, Vec<&BidsRow>> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.suffix == set_name) {
        by_group
            .entry(group_key(r, &cfg.loop_over))
            .or_default()
            .push(r);
    }

    let mut out = Vec::new();
    for (key, members) in by_group {
        let (subject, session, run, task) = key_fields(&key, &cfg.loop_over);
        let (data, warnings) = match def {
            SetDef::Sequential(seq) => {
                let mut sorted: Vec<&BidsRow> = members.clone();
                sorted.sort_by(|a, b| {
                    for e in &seq.by {
                        let av = a.entities.get(e);
                        let bv = b.entities.get(e);
                        match av.cmp(&bv) {
                            std::cmp::Ordering::Equal => continue,
                            ord => return ord,
                        }
                    }
                    std::cmp::Ordering::Equal
                });
                (
                    GroupedData::Sequential(sorted.iter().map(|r| vol(r)).collect()),
                    vec![],
                )
            }
            SetDef::Named(named) => resolve_named(named, &members),
            SetDef::Plain(_) => {
                // Out of scope for this plan; skip with a warning.
                (
                    GroupedData::Sequential(members.iter().map(|r| vol(r)).collect()),
                    vec![Warning {
                        message: format!("plain_set {set_name} not yet supported"),
                    }],
                )
            }
        };
        // A named set missing all members yields nothing; drop empties.
        let empty = matches!(&data, GroupedData::Named(m) if m.is_empty())
            || matches!(&data, GroupedData::Sequential(v) if v.is_empty());
        if empty {
            continue;
        }
        out.push(Collection {
            subject,
            session,
            run,
            task,
            suffix: set_name.to_string(),
            data,
            warnings,
        });
    }
    out.sort_by(|a, b| (&a.subject, &a.session, &a.run).cmp(&(&b.subject, &b.session, &b.run)));
    Ok(out)
}

fn resolve_named(
    named: &crate::config::NamedSet,
    members: &[&BidsRow],
) -> (GroupedData, Vec<Warning>) {
    let mut groups = BTreeMap::new();
    let mut warnings = Vec::new();
    for (gname, constraints) in &named.groups {
        let matched: Vec<&&BidsRow> = members
            .iter()
            .filter(|r| {
                constraints
                    .iter()
                    .all(|(k, v)| r.entities.get(k).map(|rv| rv == v).unwrap_or(false))
            })
            .collect();
        match matched.as_slice() {
            [one] => {
                groups.insert(gname.clone(), vol(one));
            }
            [] => warnings.push(Warning {
                message: format!("named group {gname}: no matching file"),
            }),
            many => warnings.push(Warning {
                message: format!(
                    "named group {gname}: {} files matched (expected 1)",
                    many.len()
                ),
            }),
        }
    }
    // Enforce `required`: if any required member is absent, the collection is invalid.
    let missing: Vec<&String> = named
        .required
        .iter()
        .filter(|r| !groups.contains_key(*r))
        .collect();
    if !missing.is_empty() {
        return (GroupedData::Named(BTreeMap::new()), warnings); // dropped as empty upstream
    }
    (GroupedData::Named(groups), warnings)
}

pub fn collections_for<F: DatasetFs>(
    fs: &F,
    cfg: &Bids2nfConfig,
    suffix: &str,
) -> Result<Vec<Collection>> {
    let table = parse_to_table(fs)?;
    resolve_set(&table, cfg, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::default_config;
    use crate::fs::MemFs;

    fn irt1_fs() -> MemFs {
        let mut fs = MemFs::new();
        for i in 1..=4 {
            fs = fs
                .touch(&format!("sub-01/anat/sub-01_inv-0{i}_IRT1.nii.gz"))
                .with(
                    &format!("sub-01/anat/sub-01_inv-0{i}_IRT1.json"),
                    b"{}".to_vec(),
                );
        }
        fs
    }

    #[test]
    fn resolves_sequential_irt1_in_order() {
        let cols = collections_for(&irt1_fs(), &default_config(), "IRT1").unwrap();
        assert_eq!(cols.len(), 1);
        let GroupedData::Sequential(v) = &cols[0].data else {
            panic!()
        };
        assert_eq!(v.len(), 4);
        assert!(v[0].nii.contains("inv-01"));
        assert!(v[3].nii.contains("inv-04"));
    }

    #[test]
    fn resolves_named_mts_groups() {
        let fs = MemFs::new()
            .touch("sub-01/anat/sub-01_flip-1_mt-off_MTS.nii.gz")
            .touch("sub-01/anat/sub-01_flip-1_mt-on_MTS.nii.gz")
            .touch("sub-01/anat/sub-01_flip-2_mt-off_MTS.nii.gz");
        let cols = collections_for(&fs, &default_config(), "MTS").unwrap();
        assert_eq!(cols.len(), 1);
        let GroupedData::Named(g) = &cols[0].data else {
            panic!()
        };
        assert!(g.contains_key("PDw") && g.contains_key("MTw") && g.contains_key("T1w"));
    }

    #[test]
    fn named_missing_required_drops_collection() {
        // Only PDw present → required [PDw,MTw,T1w] unmet → no collection.
        let fs = MemFs::new().touch("sub-01/anat/sub-01_flip-1_mt-off_MTS.nii.gz");
        let cols = collections_for(&fs, &default_config(), "MTS").unwrap();
        assert!(cols.is_empty());
    }
}
