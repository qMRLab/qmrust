//! Which files in a dataset are a collection's auxiliary inputs and mask.
//!
//! Selection only — this names paths and never reads them, so the same
//! resolution serves a caller that will load them from disk and one that already
//! holds their bytes. Every lookup is driven by the model's own
//! `required_inputs()` declarations, so a new model needs no change here.

use crate::table::{table_filter, BidsRow};
use crate::vocab::Vocabulary;
use anyhow::{bail, Result};
use qmrust_core::core::model::Model;
use std::collections::BTreeMap;

/// Which mask to apply, from a recipe's `mask:` key.
///
/// A dataset may hold several masks; this disambiguates by suffix plus entity
/// constraints so the choice is stated by the recipe rather than guessed.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MaskSpec {
    #[serde(default = "default_mask_suffix")]
    pub suffix: String,
    #[serde(flatten)]
    pub entities: BTreeMap<String, String>,
}

fn default_mask_suffix() -> String {
    "mask".to_string()
}

impl MaskSpec {
    /// Parse a recipe's `mask:` value, normalizing its entity keys (e.g. `desc`
    /// → `description`) to the form the file table stores them in. `None` when
    /// the recipe declares no `mask:` — no masking is then applied.
    pub fn from_recipe(raw: &serde_yaml::Value, vocab: &Vocabulary) -> Result<Option<Self>> {
        let Some(v) = raw.get("mask") else {
            return Ok(None);
        };
        let spec: Self = serde_yaml::from_value(v.clone())
            .map_err(|e| anyhow::anyhow!("invalid `mask:` config: {e}"))?;
        Ok(Some(Self {
            suffix: spec.suffix,
            entities: spec
                .entities
                .into_iter()
                .map(|(k, v)| (vocab.normalize_entity_key(&k), v))
                .collect(),
        }))
    }
}

/// The dataset-relative paths of one collection's non-measurement inputs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InputPaths {
    /// One entry per input the model declares, in declaration order. `None`
    /// means "absent" — legitimate for an optional input, which leaves the model
    /// on its own default.
    pub aux: Vec<(String, Option<String>)>,
    pub mask: Option<String>,
}

impl InputPaths {
    /// Every path actually found, aux then mask — the provenance `Sources` list.
    pub fn found(&self) -> Vec<String> {
        self.aux
            .iter()
            .filter_map(|(_, p)| p.clone())
            .chain(self.mask.clone())
            .collect()
    }
}

/// The single row matching `identity` plus `extra`, or `None`.
///
/// Ambiguity is an error naming every candidate: two files that equally answer
/// "this collection's B1 map" is a dataset question the caller cannot resolve by
/// picking one.
pub fn find_row<'a>(
    table: &'a [BidsRow],
    identity: &BTreeMap<String, String>,
    extra: &[(&str, &str)],
) -> Result<Option<&'a BidsRow>> {
    let mut constraints: Vec<(&str, &str)> = identity
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    constraints.extend_from_slice(extra);
    let hits = table_filter(table, &constraints);
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => bail!(
            "ambiguous input for {:?}: {} candidates ({})",
            constraints,
            many.len(),
            many.iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Locate a model's declared auxiliary inputs and its mask for one collection.
///
/// Each input is matched on the collection's full entity `identity` plus its
/// declared BIDS suffix (and any `BidsMap.entity` the model declares), so it is
/// keyed on whatever entities identify the collection — subject, session, run
/// alike — rather than a fixed pair. Found wherever it lives: the raw tree or
/// any `derivatives/` pipeline, since the table spans the whole dataset.
///
/// A `required` input that is absent is a hard error; an optional one becomes a
/// `None` entry.
pub fn resolve_input_paths(
    table: &[BidsRow],
    model: &dyn Model,
    identity: &BTreeMap<String, String>,
    mask_spec: Option<&MaskSpec>,
) -> Result<InputPaths> {
    let mut aux: Vec<(String, Option<String>)> = Vec::new();
    for spec in model.required_inputs() {
        let Some(bids) = spec.bids.as_ref() else {
            // Not BIDS-locatable: absent, for the model's own default.
            aux.push((spec.name.to_string(), None));
            continue;
        };
        let mut extra = vec![("suffix", bids.suffix)];
        if let Some(entity) = bids.entity {
            // The model declares an entity indexing this input; match it against
            // the value the collection carries for that entity.
            if let Some(val) = identity.get(entity) {
                extra.push((entity, val.as_str()));
            }
        }
        match find_row(table, identity, &extra)? {
            Some(row) => aux.push((spec.name.to_string(), Some(row.path.clone()))),
            None if spec.required => bail!(
                "required input '{}' (BIDS suffix '{}') not found for {:?}",
                spec.name,
                bids.suffix,
                identity
            ),
            None => aux.push((spec.name.to_string(), None)),
        }
    }

    let mask = match mask_spec {
        Some(spec) => {
            let mut extra: Vec<(&str, &str)> = vec![("suffix", spec.suffix.as_str())];
            extra.extend(spec.entities.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            find_row(table, identity, &extra)?.map(|row| row.path.clone())
        }
        None => None,
    };

    Ok(InputPaths { aux, mask })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Input resolution is driven by a collection's *full* entity identity, not
    /// a fixed subject/session pair: the same suffix present once per session
    /// must resolve to the file matching the collection's own session, and an
    /// underspecified identity that matches several files is a hard error (no
    /// silent pick). This is what lets the backbone scale past two models —
    /// any entity a dataset groups by participates in locating inputs.
    #[test]
    fn find_input_matches_full_identity_and_flags_ambiguity() {
        use std::collections::BTreeMap;
        let row = |path: &str, ses: &str| BidsRow {
            path: path.into(),
            derivatives: Some("preprocessed".into()),
            datatype: Some("fmap".into()),
            suffix: "TB1map".into(),
            extension: ".nii.gz".into(),
            entities: BTreeMap::from([
                ("subject".to_string(), "01".to_string()),
                ("session".to_string(), ses.to_string()),
            ]),
            sidecar_path: None,
        };
        let rows = vec![
            row(
                "derivatives/preprocessed/sub-01/ses-1/fmap/sub-01_ses-1_TB1map.nii.gz",
                "1",
            ),
            row(
                "derivatives/preprocessed/sub-01/ses-2/fmap/sub-01_ses-2_TB1map.nii.gz",
                "2",
            ),
        ];

        let identity = |ses: Option<&str>| {
            let mut m = BTreeMap::from([("subject".to_string(), "01".to_string())]);
            if let Some(s) = ses {
                m.insert("session".to_string(), s.to_string());
            }
            m
        };

        // Full identity (subject + session) → the matching session's file.
        let hit = find_row(&rows, &identity(Some("2")), &[("suffix", "TB1map")])
            .unwrap()
            .unwrap();
        assert!(hit.path.contains("ses-2"));
        // Underspecified identity (subject only) matches both → ambiguity error.
        assert!(find_row(&rows, &identity(None), &[("suffix", "TB1map")]).is_err());
        // No match → None (an optional input the fit does without).
        assert!(
            find_row(&rows, &identity(Some("9")), &[("suffix", "TB1map")])
                .unwrap()
                .is_none()
        );
    }

    /// A `mask:` config disambiguates which mask to use by entity constraint,
    /// and its short entity keys must be matched against the table's full names.
    /// A dataset holding several masks resolves to the configured one; a spec
    /// too loose to be unique is a hard error rather than a silent pick.
    #[test]
    fn mask_spec_disambiguates_and_flags_ambiguity() {
        use std::collections::BTreeMap;
        // `desc: brain` parses (suffix defaults to `mask`); `desc` is the short
        // key the config author writes.
        let spec: MaskSpec = serde_yaml::from_str("desc: brain\n").unwrap();
        assert_eq!(spec.suffix, "mask");
        assert_eq!(spec.entities.get("desc").map(String::as_str), Some("brain"));

        // Two masks for one subject; the table stores the entity as its full
        // name `description` (as `parse_to_table` would), so the config key is
        // normalized to match.
        let mask_row = |desc: &str| BidsRow {
            path: format!("derivatives/preprocessed/sub-01/anat/sub-01_desc-{desc}_mask.nii.gz"),
            derivatives: Some("preprocessed".into()),
            datatype: Some("anat".into()),
            suffix: "mask".into(),
            extension: ".nii.gz".into(),
            entities: BTreeMap::from([
                ("subject".to_string(), "01".to_string()),
                ("description".to_string(), desc.to_string()),
            ]),
            sidecar_path: None,
        };
        let rows = vec![mask_row("brain"), mask_row("tumor")];
        let identity = BTreeMap::from([("subject".to_string(), "01".to_string())]);

        // Config `desc` -> full `description`, matching the table.
        let extra = [("suffix", "mask"), ("description", "brain")];
        let hit = find_row(&rows, &identity, &extra).unwrap().unwrap();
        assert!(hit.path.contains("desc-brain"));

        // Suffix alone is too loose — both masks match, so it errors.
        assert!(find_row(&rows, &identity, &[("suffix", "mask")]).is_err());
    }
}
