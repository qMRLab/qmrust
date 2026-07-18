//! The declarative grouping grammar: a `BidsConfig` names entities to loop
//! over and a set of grouping rules (plain/named/sequential) that turn matched
//! files into `Collection`s.

use anyhow::Result;
use serde::Deserialize;
use std::collections::BTreeMap;

pub type EntityConstraints = BTreeMap<String, String>;

#[derive(Debug, Clone, Deserialize)]
pub struct PlainSet {
    #[serde(default)]
    pub additional_extensions: Vec<String>,
    #[serde(default)]
    pub include_cross_modal: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NamedSet {
    pub groups: BTreeMap<String, EntityConstraints>,
    pub required: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SequentialSet {
    /// Entities to order the series along, e.g. ["inversion"].
    pub by: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum SetDef {
    Plain(PlainSet),
    Named(NamedSet),
    Sequential(SequentialSet),
}

#[derive(Debug, Clone)]
pub struct BidsConfig {
    pub loop_over: Vec<String>,
    pub sets: BTreeMap<String, SetDef>,
}

// --- deserialization: the YAML nests the set under a `*_set` key ------------

#[derive(Deserialize)]
struct RawSetEntry {
    plain_set: Option<PlainSet>,
    named_set: Option<BTreeMap<String, serde_yaml::Value>>,
    sequential_set: Option<SequentialSet>,
}

#[derive(Deserialize)]
struct RawConfig {
    #[serde(default = "default_loop_over")]
    loop_over: Vec<String>,
    #[serde(flatten)]
    sets: BTreeMap<String, RawSetEntry>,
}

fn default_loop_over() -> Vec<String> {
    ["subject", "session", "run", "task"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Strip a leading `entity-` prefix from a constraint value: `"flip-02" → "02"`.
/// Accepts BOTH the short (`inv-01`) and full (`inversion-01`) prefixed forms,
/// since config authors may write either.
fn strip_prefix_value(entity: &str, raw: &str) -> String {
    let short = crate::entities::short_key(entity);
    raw.strip_prefix(&format!("{short}-"))
        .or_else(|| raw.strip_prefix(&format!("{entity}-")))
        .unwrap_or(raw)
        .to_string()
}

pub fn parse_config(yaml: &str) -> Result<BidsConfig> {
    let raw: RawConfig = serde_yaml::from_str(yaml)?;
    let mut sets = BTreeMap::new();
    for (name, entry) in raw.sets {
        let def = if let Some(p) = entry.plain_set {
            SetDef::Plain(p)
        } else if let Some(s) = entry.sequential_set {
            SetDef::Sequential(s)
        } else if let Some(groups_raw) = entry.named_set {
            let mut groups = BTreeMap::new();
            let mut required = Vec::new();
            for (gname, gval) in groups_raw {
                if gname == "required" {
                    required = serde_yaml::from_value(gval)?;
                    continue;
                }
                let mut cons: EntityConstraints =
                    serde_yaml::from_value::<BTreeMap<String, String>>(gval)?
                        .into_iter()
                        .filter(|(k, _)| k != "description")
                        .collect();
                cons = cons
                    .into_iter()
                    .map(|(k, v)| {
                        let v = strip_prefix_value(&k, &v);
                        (k, v)
                    })
                    .collect();
                groups.insert(gname, cons);
            }
            SetDef::Named(NamedSet { groups, required })
        } else {
            continue; // unrecognized set shape: skipped, not rejected (permissive parsing)
        };
        sets.insert(name, def);
    }
    Ok(BidsConfig {
        loop_over: raw.loop_over,
        sets,
    })
}

pub fn default_config() -> BidsConfig {
    parse_config(
        r#"
loop_over: [subject, session, run, task]
IRT1:
  sequential_set:
    by: [inversion]
MTS:
  named_set:
    PDw:
      flip: "flip-1"
      mtransfer: "mt-off"
    MTw:
      flip: "flip-1"
      mtransfer: "mt-on"
    T1w:
      flip: "flip-2"
      mtransfer: "mt-off"
    required: [PDw, MTw, T1w]
"#,
    )
    .expect("bundled default config is valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_mts_with_stripped_values() {
        let cfg = default_config();
        let SetDef::Named(mts) = &cfg.sets["MTS"] else {
            panic!("MTS should be a named set");
        };
        assert_eq!(mts.required, vec!["PDw", "MTw", "T1w"]);
        assert_eq!(mts.groups["T1w"]["flip"], "2"); // "flip-2" → "2"
        assert_eq!(mts.groups["PDw"]["mtransfer"], "off");
    }

    #[test]
    fn parses_sequential_irt1() {
        let cfg = default_config();
        let SetDef::Sequential(irt1) = &cfg.sets["IRT1"] else {
            panic!("IRT1 should be sequential");
        };
        assert_eq!(irt1.by, vec!["inversion"]);
    }
}
