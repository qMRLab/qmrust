//! Layer 2: group table rows into collections per the declarative grouping
//! grammar (`BidsConfig`).

use crate::collection::{Collection, GroupedData, VolumeRef, Warning};
use crate::config::{BidsConfig, SetDef};
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

pub fn resolve_set(rows: &[BidsRow], cfg: &BidsConfig, set_name: &str) -> Result<Vec<Collection>> {
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
                // plain_set is intentionally parse-only (YAGNI): it is not yet
                // grouped into a Collection by this resolver.
                (
                    GroupedData::Sequential(members.iter().map(|r| vol(r)).collect()),
                    vec![Warning {
                        message: format!(
                            "plain_set '{set_name}' is not resolved into a grouped collection by this resolver"
                        ),
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
    cfg: &BidsConfig,
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
    fn resolves_sequential_qmtspgr_ordered_by_mt_then_flip() {
        // 2 flip x 5 mt = 10 volumes; canonical order is mt outer, flip inner:
        // flip-1_mt-1, flip-2_mt-1, flip-1_mt-2, flip-2_mt-2, ...
        let mut fs = MemFs::new();
        for mt in 1..=5 {
            for flip in 1..=2 {
                fs = fs
                    .touch(&format!(
                        "sub-02/anat/sub-02_flip-{flip}_mt-{mt}_QMTSPGR.nii.gz"
                    ))
                    .with(
                        &format!("sub-02/anat/sub-02_flip-{flip}_mt-{mt}_QMTSPGR.json"),
                        b"{}".to_vec(),
                    );
            }
        }
        let cols = collections_for(&fs, &default_config(), "QMTSPGR").unwrap();
        assert_eq!(cols.len(), 1);
        let GroupedData::Sequential(v) = &cols[0].data else {
            panic!("expected sequential data")
        };
        assert_eq!(v.len(), 10);
        let expected: Vec<(u32, u32)> = (1..=5)
            .flat_map(|mt| (1..=2).map(move |f| (mt, f)))
            .collect();
        for (vol, (mt, flip)) in v.iter().zip(expected) {
            assert!(
                vol.nii.contains(&format!("flip-{flip}_mt-{mt}_QMTSPGR")),
                "expected flip-{flip}_mt-{mt} at this position, got {}",
                vol.nii
            );
        }
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

    #[test]
    fn keeps_multiple_subjects_separate_and_sorted() {
        // Two subjects, each with their own IRT1 series. loop_over includes
        // `subject`, so each must resolve to its own collection with no
        // cross-subject leakage, and the output must be deterministically
        // ordered by subject.
        let mut fs = MemFs::new();
        for sub in ["01", "02"] {
            for i in 1..=3 {
                fs = fs
                    .touch(&format!("sub-{sub}/anat/sub-{sub}_inv-0{i}_IRT1.nii.gz"))
                    .with(
                        &format!("sub-{sub}/anat/sub-{sub}_inv-0{i}_IRT1.json"),
                        b"{}".to_vec(),
                    );
            }
        }
        let cols = collections_for(&fs, &default_config(), "IRT1").unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0].subject, "sub-01");
        assert_eq!(cols[1].subject, "sub-02");

        let GroupedData::Sequential(v0) = &cols[0].data else {
            panic!()
        };
        assert_eq!(v0.len(), 3);
        assert!(v0.iter().all(|vr| vr.nii.contains("sub-01/")));
        assert!(v0.iter().all(|vr| !vr.nii.contains("sub-02")));

        let GroupedData::Sequential(v1) = &cols[1].data else {
            panic!()
        };
        assert_eq!(v1.len(), 3);
        assert!(v1.iter().all(|vr| vr.nii.contains("sub-02/")));
        assert!(v1.iter().all(|vr| !vr.nii.contains("sub-01")));
    }

    #[test]
    fn resolves_mixed_session_and_no_session_subjects_without_phantom_paths() {
        // An absent entity resolves to `None`, and no path is ever derived from
        // entity values: sub-01 has a real session level, sub-02 has none (no
        // `ses-*` directory, no `ses-` filename entity). `MemFs::read` errors on
        // any path not explicitly inserted, so resolution completing for both
        // subjects proves no synthesized path was read for the session-less one.
        let mut fs = MemFs::new();
        for i in 1..=2 {
            fs = fs
                .touch(&format!(
                    "sub-01/ses-1/anat/sub-01_ses-1_inv-0{i}_IRT1.nii.gz"
                ))
                .with(
                    &format!("sub-01/ses-1/anat/sub-01_ses-1_inv-0{i}_IRT1.json"),
                    b"{}".to_vec(),
                );
        }
        for i in 1..=3 {
            fs = fs
                .touch(&format!("sub-02/anat/sub-02_inv-0{i}_IRT1.nii.gz"))
                .with(
                    &format!("sub-02/anat/sub-02_inv-0{i}_IRT1.json"),
                    b"{}".to_vec(),
                );
        }

        let cols = collections_for(&fs, &default_config(), "IRT1").unwrap();
        assert_eq!(cols.len(), 2, "both subjects must resolve independently");

        let sub01 = cols.iter().find(|c| c.subject == "sub-01").unwrap();
        assert_eq!(sub01.session.as_deref(), Some("1"));
        let GroupedData::Sequential(v01) = &sub01.data else {
            panic!("expected sequential data")
        };
        assert_eq!(v01.len(), 2, "sub-01 has 2 inversion volumes");

        let sub02 = cols.iter().find(|c| c.subject == "sub-02").unwrap();
        assert!(
            sub02.session.is_none(),
            "sub-02 genuinely has no session — must resolve to None, not a phantom \"NA\""
        );
        let GroupedData::Sequential(v02) = &sub02.data else {
            panic!("expected sequential data")
        };
        assert_eq!(v02.len(), 3, "sub-02 has 3 inversion volumes");
    }

    #[test]
    fn non_required_named_group_partial_match_warns_but_still_emits() {
        // `B` is deliberately left out of `required`, so a 0-match on `B`
        // must not drop the collection (since `A` — the only required
        // member — is present) but must still surface a Warning naming `B`.
        let cfg = crate::config::parse_config(
            r#"
loop_over: [subject, session, run, task]
TEST:
  named_set:
    A:
      flip: "1"
    B:
      flip: "2"
    required: [A]
"#,
        )
        .unwrap();
        let fs = MemFs::new().touch("sub-01/anat/sub-01_flip-1_TEST.nii.gz");
        let cols = collections_for(&fs, &cfg, "TEST").unwrap();
        assert_eq!(
            cols.len(),
            1,
            "required group A present → collection emitted"
        );
        let GroupedData::Named(g) = &cols[0].data else {
            panic!()
        };
        assert!(g.contains_key("A"));
        assert!(!g.contains_key("B"));
        assert!(
            !cols[0].warnings.is_empty(),
            "missing non-required group B should still warn"
        );
        assert!(cols[0]
            .warnings
            .iter()
            .any(|w| w.message.contains('B') && w.message.contains("no matching file")));
    }
}
