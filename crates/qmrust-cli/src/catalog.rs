//! `qmrust catalog --json`: registry metadata + `Model` trait introspection as
//! one JSON payload. The single contract between the Rust code and the
//! documentation generators.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qmrust_core::core::model::{MeasurementKind, Model, Scope, Source};
use qmrust_core::registry::{self, ModelEntry};

#[derive(Serialize)]
pub struct Catalog {
    pub models: Vec<ModelCard>,
}

#[derive(Serialize)]
pub struct ModelCard {
    pub name: String,
    pub bids_suffix: String,
    pub title: String,
    pub category: String,
    pub category_title: String,
    pub summary: String,
    pub equation: String,
    pub symbols: Vec<Symbol>,
    pub citations: Vec<String>,
    pub source_dir: String,
    pub recipes: RecipePaths,
    pub params: Vec<Param>,
    pub outputs: Vec<Output>,
    pub measurement: MeasurementCard,
    pub protocol_schema: Vec<ProtoParamCard>,
    pub required_inputs: Vec<InputCard>,
    pub n_volumes: usize,
    pub strategy: String,
    pub effective_config: String,
    /// Config keys restricted to a fixed set of values (dropdowns, not free
    /// text). See `qmrust_core::registry::ModelDoc::enums`.
    pub enums: Vec<EnumField>,
}

#[derive(Serialize)]
pub struct EnumField {
    /// Config key, dotted for a nested group (e.g. `"pulse.shape"`).
    pub key: String,
    pub values: Vec<String>,
}

#[derive(Serialize)]
pub struct Symbol {
    pub name: String,
    pub meaning: String,
    pub unit: String,
}

#[derive(Serialize)]
pub struct RecipePaths {
    pub bids: String,
    pub non_bids: String,
    pub sim: Option<String>,
}

/// A fit parameter. Bounds are `None` when the model reports a non-finite
/// bound — JSON has no infinity, and "unbounded" is what the docs render.
#[derive(Serialize)]
pub struct Param {
    pub name: String,
    pub lower: Option<f64>,
    pub upper: Option<f64>,
    pub fixed: bool,
}

/// An output map. `bids_suffix`/`unit` are `None` for diagnostics — entries in
/// `output_names()` the model does not declare in `bids_outputs()`.
#[derive(Serialize)]
pub struct Output {
    pub name: String,
    pub bids_suffix: Option<String>,
    pub unit: Option<String>,
    pub diagnostic: bool,
    /// The declared display window `[min, max]` in this output's unit, when the
    /// model declares one; `None` leaves the scale to the data.
    pub display_range: Option<[f64; 2]>,
}

#[derive(Serialize)]
pub struct MeasurementCard {
    /// `"series"` or `"named"`.
    pub kind: String,
    pub roles: Vec<RoleCard>,
    pub rows: Vec<BTreeMap<String, f64>>,
}

/// One role of a `Named` measurement, with the BIDS filename entities that
/// identify it — the fact a docs consumer needs to match a role to a volume
/// by entity, not by glob or array position.
#[derive(Serialize)]
pub struct RoleCard {
    pub role: String,
    pub entities: Vec<EntityCard>,
}

#[derive(Serialize)]
pub struct EntityCard {
    pub key: String,
    pub value: String,
}

#[derive(Serialize)]
pub struct ProtoParamCard {
    pub name: String,
    /// `"field"`, `"derived"` or `"option"`.
    pub source: String,
    /// Sidecar/option key; `None` for a derived value, which has no single key.
    pub key: Option<String>,
    /// `"per_volume"` or `"global"`.
    pub scope: String,
}

#[derive(Serialize)]
pub struct InputCard {
    pub name: String,
    pub required: bool,
    pub bids_suffix: Option<String>,
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

fn card(entry: &ModelEntry, repo_root: &Path) -> Result<ModelCard> {
    let doc = &entry.doc;
    let recipe = repo_root.join(doc.recipes.non_bids);
    let (_cfg, raw) = crate::commands::load_config_raw(&recipe)
        .with_context(|| format!("{}: reading recipe {}", entry.name, doc.recipes.non_bids))?;
    let model: Box<dyn Model> = (entry.describe)(&raw)
        .with_context(|| format!("{}: describing from {}", entry.name, doc.recipes.non_bids))?;
    let effective_config = (entry.dump)(&raw)?;

    let bounds = model.param_bounds();
    let fixed = model.fixed_mask();
    let params = model
        .param_names()
        .iter()
        .enumerate()
        .map(|(i, name)| Param {
            name: name.to_string(),
            lower: bounds.get(i).and_then(|b| finite(b.0)),
            upper: bounds.get(i).and_then(|b| finite(b.1)),
            fixed: fixed.get(i).copied().unwrap_or(false),
        })
        .collect();

    let declared = model.bids_outputs();
    let window = |name: &str| {
        doc.display_ranges
            .iter()
            .find(|(out, _, _)| *out == name)
            .map(|(_, lo, hi)| [*lo, *hi])
    };
    let outputs = model
        .output_names()
        .into_iter()
        .map(
            |name| match declared.iter().find(|(out, _, _)| *out == name) {
                Some((_, suffix, unit)) => Output {
                    display_range: window(&name),
                    name,
                    bids_suffix: Some(suffix.to_string()),
                    unit: Some(unit.to_string()),
                    diagnostic: false,
                },
                None => Output {
                    display_range: window(&name),
                    name,
                    bids_suffix: None,
                    unit: None,
                    diagnostic: true,
                },
            },
        )
        .collect();

    let measurement = match model.measurement() {
        MeasurementKind::Named { roles } => MeasurementCard {
            kind: "named".to_string(),
            roles: roles
                .iter()
                .enumerate()
                .map(|(i, r)| RoleCard {
                    role: r.to_string(),
                    entities: model
                        .bids_volume(i)
                        .entities
                        .into_iter()
                        .map(|(key, value)| EntityCard {
                            key: key.to_string(),
                            value,
                        })
                        .collect(),
                })
                .collect(),
            rows: vec![],
        },
        MeasurementKind::Series { rows } => MeasurementCard {
            kind: "series".to_string(),
            roles: vec![],
            rows,
        },
    };

    let protocol_schema = model
        .protocol_schema()
        .into_iter()
        .map(|p| ProtoParamCard {
            name: p.name.to_string(),
            source: match p.source {
                Source::Field(_) => "field",
                Source::Derived(_) => "derived",
                Source::Option(_) => "option",
            }
            .to_string(),
            key: match p.source {
                Source::Field(k) | Source::Option(k) => Some(k.to_string()),
                Source::Derived(_) => None,
            },
            scope: match p.scope {
                Scope::PerVolume => "per_volume",
                Scope::Global => "global",
            }
            .to_string(),
        })
        .collect();

    let required_inputs = model
        .required_inputs()
        .into_iter()
        .map(|i| InputCard {
            name: i.name.to_string(),
            required: i.required,
            bids_suffix: i.bids.map(|b| b.suffix.to_string()),
        })
        .collect();

    Ok(ModelCard {
        name: entry.name.to_string(),
        bids_suffix: entry.bids_suffix.to_string(),
        title: doc.title.to_string(),
        category: doc.category.slug().to_string(),
        category_title: doc.category.title().to_string(),
        summary: doc.summary.to_string(),
        equation: doc.equation.to_string(),
        symbols: doc
            .symbols
            .iter()
            .map(|(name, meaning, unit)| Symbol {
                name: name.to_string(),
                meaning: meaning.to_string(),
                unit: unit.to_string(),
            })
            .collect(),
        citations: doc.citations.iter().map(|c| c.to_string()).collect(),
        source_dir: doc.source_dir.to_string(),
        recipes: RecipePaths {
            bids: doc.recipes.bids.to_string(),
            non_bids: doc.recipes.non_bids.to_string(),
            sim: doc.recipes.sim.map(|s| s.to_string()),
        },
        params,
        outputs,
        measurement,
        protocol_schema,
        required_inputs,
        n_volumes: model.n_volumes(),
        strategy: match model.strategy() {
            qmrust_core::core::model::FitStrategy::Voxelwise => "voxelwise",
            qmrust_core::core::model::FitStrategy::MatrixWise => "matrixwise",
        }
        .to_string(),
        effective_config,
        enums: doc
            .enums
            .iter()
            .map(|(key, values)| EnumField {
                key: key.to_string(),
                values: values.iter().map(|v| v.to_string()).collect(),
            })
            .collect(),
    })
}

/// Describe every registered model. `repo_root` is where declared recipe paths
/// are resolved from.
pub fn build(repo_root: &Path) -> Result<Catalog> {
    let models = registry::all()
        .iter()
        .map(|e| card(e, repo_root))
        .collect::<Result<Vec<_>>>()?;
    Ok(Catalog { models })
}

/// Print the catalog as pretty JSON on stdout.
pub fn run(repo_root: PathBuf) -> Result<()> {
    let cat = build(&repo_root)?;
    println!("{}", serde_json::to_string_pretty(&cat)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn catalog_has_one_entry_per_registered_model() {
        let cat = build(&repo_root()).unwrap();
        assert_eq!(cat.models.len(), qmrust_core::registry::all().len());
        for m in &cat.models {
            assert!(qmrust_core::registry::by_name(&m.name).is_some());
        }
    }

    #[test]
    fn every_declared_recipe_exists_and_describes() {
        // The catalog describes each model from its non-BIDS recipe, so a
        // missing or invalid recipe is a documentation build failure, not a
        // silent empty page.
        let root = repo_root();
        for e in qmrust_core::registry::all() {
            for path in [e.doc.recipes.bids, e.doc.recipes.non_bids]
                .into_iter()
                .chain(e.doc.recipes.sim)
            {
                assert!(
                    root.join(path).exists(),
                    "{}: declared recipe '{}' does not exist",
                    e.name,
                    path
                );
            }
        }
        let cat = build(&root).unwrap();
        for m in &cat.models {
            assert!(
                !m.effective_config.is_empty(),
                "{}: empty effective_config",
                m.name
            );
        }
    }

    #[test]
    fn doc_symbols_name_real_parameters() {
        let cat = build(&repo_root()).unwrap();
        for m in &cat.models {
            let params: Vec<&str> = m.params.iter().map(|p| p.name.as_str()).collect();
            for s in &m.symbols {
                assert!(
                    params.contains(&s.name.as_str()),
                    "{}: doc symbol '{}' is not a parameter {:?}",
                    m.name,
                    s.name,
                    params
                );
            }
        }
    }

    #[test]
    fn series_models_report_a_populated_acquisition_axis() {
        // Describing from the non-BIDS recipe (not an empty config) is what
        // makes identity rows and n_volumes real.
        let cat = build(&repo_root()).unwrap();
        for m in &cat.models {
            assert!(m.n_volumes > 0, "{}: n_volumes is 0", m.name);
            match m.measurement.kind.as_str() {
                "series" => assert!(!m.measurement.rows.is_empty(), "{}: no rows", m.name),
                "named" => assert!(!m.measurement.roles.is_empty(), "{}: no roles", m.name),
                other => panic!("{}: unknown measurement kind '{}'", m.name, other),
            }
        }
    }

    #[test]
    fn non_finite_bounds_serialize_as_null() {
        let cat = build(&repo_root()).unwrap();
        let json = serde_json::to_string(&cat).unwrap();
        assert!(!json.contains("inf"), "JSON must not contain infinity");
        let ir = cat
            .models
            .iter()
            .find(|m| m.name == "inversion_recovery")
            .unwrap();
        assert!(ir
            .params
            .iter()
            .all(|p| p.lower.is_none() && p.upper.is_none()));
    }

    fn get_nested<'a>(v: &'a serde_yaml::Value, dotted_key: &str) -> Option<&'a serde_yaml::Value> {
        let mut node = v;
        for part in dotted_key.split('.') {
            node = node.get(part)?;
        }
        Some(node)
    }

    fn set_nested(v: &mut serde_yaml::Value, dotted_key: &str, new_value: &str) {
        let parts: Vec<&str> = dotted_key.split('.').collect();
        let mut node = v;
        for part in &parts[..parts.len() - 1] {
            node = node
                .as_mapping_mut()
                .unwrap()
                .entry(serde_yaml::Value::String(part.to_string()))
                .or_insert(serde_yaml::Value::Mapping(Default::default()));
        }
        node.as_mapping_mut().unwrap().insert(
            serde_yaml::Value::String(parts.last().unwrap().to_string()),
            serde_yaml::Value::String(new_value.to_string()),
        );
    }

    /// A display window that names an output the model does not produce is a
    /// window that silently never applies, leaving that map on a data-derived
    /// scale while the declaration suggests otherwise. Names must also be the
    /// model's own output names — not BIDS suffixes, which differ (`MTSAT` vs
    /// `MTsat`) and would match nothing.
    #[test]
    fn declared_display_ranges_name_real_outputs() {
        for entry in qmrust_core::registry::all() {
            let card = card(entry, Path::new("../..")).expect(entry.name);
            let names: Vec<&str> = card.outputs.iter().map(|o| o.name.as_str()).collect();
            for (out, lo, hi) in entry.doc.display_ranges {
                assert!(
                    names.contains(out),
                    "{}: display_ranges names '{out}', which is not one of its outputs {names:?}",
                    entry.name,
                );
                assert!(
                    lo < hi,
                    "{}: display range for '{out}' is empty or inverted ({lo}, {hi})",
                    entry.name,
                );
            }
            // And the window must reach the catalog, so the playground sees it.
            for o in &card.outputs {
                let declared = entry
                    .doc
                    .display_ranges
                    .iter()
                    .any(|(n, _, _)| n == &o.name);
                assert_eq!(
                    declared,
                    o.display_range.is_some(),
                    "{}: '{}' declared={declared} but catalog carries {:?}",
                    entry.name,
                    o.name,
                    o.display_range,
                );
            }
        }
    }

    #[test]
    fn declared_enums_match_validate_options() {
        // Every enum a model declares in `ModelDoc::enums` (the dropdown the
        // playground's Form view renders) must actually be accepted by that
        // model's own `validate_options`, and the recipe's current value must
        // be one of the declared choices — otherwise the dropdown and the
        // model silently disagree.
        let root = repo_root();
        for entry in qmrust_core::registry::all() {
            if entry.doc.enums.is_empty() {
                continue;
            }
            let recipe_path = root.join(entry.doc.recipes.non_bids);
            let text = std::fs::read_to_string(&recipe_path)
                .unwrap_or_else(|e| panic!("{}: reading {:?}: {e}", entry.name, recipe_path));
            let base: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();

            for (key, values) in entry.doc.enums {
                let current = get_nested(&base, key)
                    .unwrap_or_else(|| panic!("{}: recipe has no key '{}'", entry.name, key));
                let current_str = current.as_str().unwrap_or_else(|| {
                    panic!(
                        "{}: key '{}' is not a string in the recipe",
                        entry.name, key
                    )
                });
                assert!(
                    values.contains(&current_str),
                    "{}: recipe's current value '{}' for '{}' is not among declared {:?}",
                    entry.name,
                    current_str,
                    key,
                    values
                );

                for value in *values {
                    let mut cfg = base.clone();
                    set_nested(&mut cfg, key, value);
                    (entry.build)(&cfg, &qmrust_core::core::model::Protocol::default())
                        .unwrap_or_else(|e| {
                            panic!(
                                "{}: declared enum value '{}' for '{}' rejected by validate_options: {e}",
                                entry.name, value, key
                            )
                        });
                }
            }
        }
    }

    #[test]
    fn named_model_roles_declare_entities() {
        // A docs consumer matches a role to a filesystem volume by entity
        // token (e.g. "mt-on"), never by glob or array position. A role with
        // no entities would silently fall back to guessing order.
        let cat = build(&repo_root()).unwrap();
        for m in &cat.models {
            if m.measurement.kind != "named" {
                continue;
            }
            for r in &m.measurement.roles {
                assert!(
                    !r.entities.is_empty(),
                    "{}: role '{}' declares no entities",
                    m.name,
                    r.role
                );
            }
        }
    }

    #[test]
    fn bids_recipes_omit_the_acquisition_and_non_bids_recipes_carry_it() {
        // The recipe split is a rule enforced by this test: a BIDS recipe must
        // not restate an acquisition the sidecars supply, or the output
        // provenance records the per-volume axis twice.
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        for entry in qmrust_core::registry::all() {
            let v: serde_yaml::Value =
                serde_yaml::from_str(&format!("model: {}\n", entry.name)).unwrap();
            let keys = (entry.effective)(&v, &qmrust_core::core::model::Protocol::default())
                .unwrap()
                .protocol_keys;
            if keys.is_empty() {
                continue;
            }
            let read = |p: &str| {
                std::fs::read_to_string(format!("{root}/{p}"))
                    .unwrap_or_else(|e| panic!("{}: cannot read {p}: {e}", entry.name))
            };
            let bids = read(entry.doc.recipes.bids);
            let non_bids = read(entry.doc.recipes.non_bids);
            for key in &keys {
                assert!(
                    !qmrust_core::core::model::states_key(&bids, key),
                    "{}: BIDS recipe {} states '{key}', which the sidecars supply",
                    entry.name,
                    entry.doc.recipes.bids
                );
                assert!(
                    qmrust_core::core::model::states_key(&non_bids, key),
                    "{}: non-BIDS recipe {} must carry '{key}'",
                    entry.name,
                    entry.doc.recipes.non_bids
                );
            }
        }
    }
}
