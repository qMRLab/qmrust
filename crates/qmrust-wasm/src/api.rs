//! Pure marshalling layer between JS-shaped inputs and `qmrust-core`. Every
//! function here is plain Rust (no wasm-bindgen), so it is unit-tested on the
//! native target. The `#[wasm_bindgen]` wrappers in `wasm.rs` call these.

use qmrust_core::core::model::{
    Aux, Measurement, MeasurementKind, Model, Protocol, Sample, VolumeId,
};
use std::collections::BTreeMap;

/// Serialize a `Measurement` to its JSON encoding: `Named` → `{"role": v}`,
/// `Series` → `[{"params": {...}, "value": v}, ...]`.
fn emit_measurement(m: &Measurement) -> Result<String, String> {
    match m {
        Measurement::Named(map) => {
            let obj: BTreeMap<&str, f64> = map.iter().map(|(k, v)| (*k, *v)).collect();
            serde_json::to_string(&obj).map_err(|e| e.to_string())
        }
        Measurement::Series(samples) => {
            let arr: Vec<serde_json::Value> = samples
                .iter()
                .map(|s| serde_json::json!({ "params": s.params, "value": s.value }))
                .collect();
            serde_json::to_string(&arr).map_err(|e| e.to_string())
        }
    }
}

/// Parse a `{ key: number }` JSON object into a `BTreeMap<String, f64>`.
fn parse_f64_object(v: &serde_json::Value) -> Result<BTreeMap<String, f64>, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "expected a JSON object of name -> number".to_string())?;
    obj.iter()
        .map(|(k, val)| {
            val.as_f64()
                .map(|n| (k.clone(), n))
                .ok_or_else(|| format!("value for '{}' is not a number", k))
        })
        .collect()
}

/// Parse the JSON encoding of a `Measurement`, dispatching on the model's
/// declared `MeasurementKind`.
fn parse_measurement(json: &str, kind: &MeasurementKind) -> Result<Measurement, String> {
    match kind {
        MeasurementKind::Named { roles } => {
            let obj: BTreeMap<String, f64> = serde_json::from_str(json)
                .map_err(|e| format!("invalid Named measurement JSON: {}", e))?;
            let mut map = BTreeMap::new();
            for role in *roles {
                let v = obj
                    .get(*role)
                    .copied()
                    .ok_or_else(|| format!("measurement missing role '{}'", role))?;
                map.insert(*role, v);
            }
            Ok(Measurement::Named(map))
        }
        MeasurementKind::Series { .. } => {
            let val: serde_json::Value = serde_json::from_str(json)
                .map_err(|e| format!("invalid Series measurement JSON: {}", e))?;
            let arr = val
                .as_array()
                .ok_or_else(|| "Series measurement must be a JSON array".to_string())?;
            let mut samples = Vec::with_capacity(arr.len());
            for item in arr {
                let value = item
                    .get("value")
                    .and_then(|v| v.as_f64())
                    .ok_or_else(|| "sample missing numeric 'value'".to_string())?;
                let params = parse_f64_object(
                    item.get("params")
                        .ok_or_else(|| "sample missing 'params'".to_string())?,
                )?;
                samples.push(Sample { params, value });
            }
            Ok(Measurement::Series(samples))
        }
    }
}

/// Parse per-volume identities for `engine::run`. `Named` → JSON array of role
/// names; `Series` → JSON array of param-row objects. Each maps to one volume.
/// An empty `json` falls back to the model's own canonical identities (role
/// list / series rows) so a caller with no external sidecar still labels every
/// volume with real params — never a positional/empty row.
fn parse_volume_ids(json: &str, kind: &MeasurementKind) -> Result<Vec<VolumeId>, String> {
    let json = json.trim();
    match kind {
        MeasurementKind::Named { roles } => {
            let names: Vec<String> = if json.is_empty() {
                roles.iter().map(|r| r.to_string()).collect()
            } else {
                serde_json::from_str(json)
                    .map_err(|e| format!("invalid Named volume_ids JSON: {}", e))?
            };
            names
                .iter()
                .map(|n| {
                    roles
                        .iter()
                        .find(|r| **r == n.as_str())
                        .copied()
                        .map(VolumeId::Role)
                        .ok_or_else(|| format!("unknown role '{}' in volume_ids", n))
                })
                .collect()
        }
        MeasurementKind::Series { rows } => {
            let parsed: Vec<BTreeMap<String, f64>> = if json.is_empty() {
                rows.clone()
            } else {
                serde_json::from_str(json)
                    .map_err(|e| format!("invalid Series volume_ids JSON: {}", e))?
            };
            Ok(parsed.into_iter().map(VolumeId::Params).collect())
        }
    }
}

/// Names of all registered models.
pub fn list_models() -> Vec<String> {
    qmrust_core::registry::all()
        .iter()
        .map(|e| e.name.to_string())
        .collect()
}

/// Parse a config YAML string and build the model it names via the registry.
pub fn build_model(cfg_yaml: &str) -> Result<Box<dyn Model>, String> {
    let (cfg, raw) = qmrust_core::config::parse_config(cfg_yaml).map_err(|e| e.to_string())?;
    let entry = qmrust_core::registry::by_name(&cfg.model)
        .ok_or_else(|| format!("Unknown model: '{}'", cfg.model))?;
    (entry.build)(&raw, &Protocol::default()).map_err(|e| e.to_string())
}

/// Build an `Aux` scalar bundle from a JSON object of `name -> f64`. An empty
/// string yields an empty `Aux`.
pub fn parse_aux(aux_json: &str) -> Result<Aux, String> {
    let mut aux = Aux::new();
    let trimmed = aux_json.trim();
    if trimmed.is_empty() {
        return Ok(aux);
    }
    let map: BTreeMap<String, f64> =
        serde_json::from_str(trimmed).map_err(|e| format!("invalid aux JSON: {}", e))?;
    for (k, v) in map {
        aux.set(&k, v);
    }
    Ok(aux)
}

/// Fit a single voxel from an identity-keyed measurement (JSON, see
/// `parse_measurement`); returns values in the model's `output_names` order.
pub fn fit_voxel(
    cfg_yaml: &str,
    measurement_json: &str,
    aux_json: &str,
) -> Result<Vec<f64>, String> {
    let model = build_model(cfg_yaml)?;
    let aux = parse_aux(aux_json)?;
    let meas = parse_measurement(measurement_json, &model.measurement())?;
    Ok(model.fit(&meas, &aux))
}

/// Noise-free forward measurement for `params` (in `param_names` order),
/// returned as its JSON encoding (see `emit_measurement`).
pub fn forward(cfg_yaml: &str, params: &[f64], aux_json: &str) -> Result<String, String> {
    let model = build_model(cfg_yaml)?;
    let aux = parse_aux(aux_json)?;
    emit_measurement(&model.forward(params, &aux))
}

use ndarray::{Array3, Array4};

/// Fit a whole volume. `data` is C-order `[nx,ny,nz,nt]`; `mask` (optional) is
/// `[nx,ny,nz]` u8 (nonzero = fit); `aux` pairs a model input name with a
/// C-order `[nx,ny,nz]` flat map. Returns each output map name with its
/// C-order `[nx,ny,nz]` values (NaN where unfitted).
pub fn fit_volume(
    cfg_yaml: &str,
    data: &[f64],
    dims: [usize; 4],
    volume_ids_json: &str,
    mask: Option<&[u8]>,
    aux: &[(String, Vec<f64>)],
) -> Result<Vec<(String, Vec<f64>)>, String> {
    let [nx, ny, nz, nt] = dims;
    let spatial = nx * ny * nz;
    if data.len() != spatial * nt {
        return Err(format!(
            "data len {} != nx*ny*nz*nt {}",
            data.len(),
            spatial * nt
        ));
    }
    let data4 = Array4::from_shape_vec((nx, ny, nz, nt), data.to_vec())
        .map_err(|e| format!("data shape: {}", e))?;

    let mask3 = match mask {
        Some(m) => {
            if m.len() != spatial {
                return Err(format!("mask len {} != nx*ny*nz {}", m.len(), spatial));
            }
            Some(
                Array3::from_shape_vec((nx, ny, nz), m.iter().map(|&b| b != 0).collect())
                    .map_err(|e| format!("mask shape: {}", e))?,
            )
        }
        None => None,
    };

    let mut aux_maps = Vec::with_capacity(aux.len());
    for (name, flat) in aux {
        if flat.len() != spatial {
            return Err(format!(
                "aux '{}' len {} != nx*ny*nz {}",
                name,
                flat.len(),
                spatial
            ));
        }
        let arr = Array3::from_shape_vec((nx, ny, nz), flat.clone())
            .map_err(|e| format!("aux '{}' shape: {}", name, e))?;
        aux_maps.push((name.clone(), Some(arr)));
    }
    let aux_maps = qmrust_core::engine::AuxMaps::new(aux_maps);

    let model = build_model(cfg_yaml)?;
    let volume_ids = parse_volume_ids(volume_ids_json, &model.measurement())?;
    if volume_ids.len() != nt {
        return Err(format!(
            "volume_ids length {} != nt {}",
            volume_ids.len(),
            nt
        ));
    }
    let results: qmrust_core::fitting::FitResults = qmrust_core::engine::run(
        model.as_ref(),
        &data4,
        &volume_ids,
        mask3.as_ref(),
        &aux_maps,
        &mut |_| {},
    )
    .map_err(|e| e.to_string())?;

    // Emit in the model's output_names order, C-order flat.
    let mut out = Vec::new();
    for name in model.output_names() {
        if let Some(map) = results.get(&name) {
            out.push((name.clone(), map.iter().copied().collect::<Vec<f64>>()));
        }
    }
    Ok(out)
}

/// Run a simulation mode and return its report as JSON. `mode` is one of
/// `signal | single-voxel | sensitivity | montecarlo`.
pub fn sim(mode: &str, cfg_yaml: &str) -> Result<String, String> {
    let (cfg, raw) = qmrust_core::config::parse_config(cfg_yaml).map_err(|e| e.to_string())?;
    let json = match mode {
        "signal" => serde_json::to_string(
            &qmrust_core::sim::run_signal(&cfg, &raw).map_err(|e| e.to_string())?,
        ),
        "single-voxel" => serde_json::to_string(
            &qmrust_core::sim::run_single_voxel(&cfg, &raw).map_err(|e| e.to_string())?,
        ),
        "sensitivity" => serde_json::to_string(
            &qmrust_core::sim::run_sensitivity(&cfg, &raw).map_err(|e| e.to_string())?,
        ),
        "montecarlo" => serde_json::to_string(
            &qmrust_core::sim::run_montecarlo(&cfg, &raw).map_err(|e| e.to_string())?,
        ),
        other => return Err(format!("unknown sim mode '{}'", other)),
    };
    json.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Inversion times in seconds (BIDS units).
    const IR_YAML: &str = "model: inversion_recovery\nmethod: complex\ninversion_times: [0.350, 0.500, 0.650, 0.800, 0.950, 1.100, 1.250, 1.400, 1.700]\n";

    /// Build a clean IR volume as raw values plus their per-volume identity
    /// JSON (the `InversionTime` param rows) for `fit_volume`.
    fn ir_signal_and_ids() -> (Vec<f64>, String) {
        let meas = forward(IR_YAML, &[0.9, 500.0, -1000.0], "").unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&meas).unwrap();
        let data: Vec<f64> = arr.iter().map(|s| s["value"].as_f64().unwrap()).collect();
        let rows: Vec<&serde_json::Value> = arr.iter().map(|s| &s["params"]).collect();
        let ids = serde_json::to_string(&rows).unwrap();
        (data, ids)
    }

    #[test]
    fn list_models_contains_both() {
        let m = list_models();
        assert!(m.contains(&"inversion_recovery".to_string()));
        assert!(m.contains(&"qmt_spgr".to_string()));
    }

    #[test]
    fn forward_then_fit_voxel_roundtrips_ir() {
        // forward with known params, then fit the clean measurement back.
        let meas = forward(IR_YAML, &[0.9, 500.0, -1000.0], "").unwrap();
        let arr: Vec<serde_json::Value> = serde_json::from_str(&meas).unwrap();
        assert_eq!(arr.len(), 9);
        let out = fit_voxel(IR_YAML, &meas, "").unwrap();
        // output_names[0] == "T1"
        assert!((out[0] - 0.9).abs() < 1e-3, "T1: {}", out[0]);
    }

    #[test]
    fn fit_voxel_is_order_free() {
        // Reversing the measurement's samples must not change the fitted T1:
        // the model matches by InversionTime, never by position.
        let meas = forward(IR_YAML, &[0.9, 500.0, -1000.0], "").unwrap();
        let mut arr: Vec<serde_json::Value> = serde_json::from_str(&meas).unwrap();
        arr.reverse();
        let reversed = serde_json::to_string(&arr).unwrap();
        let a = fit_voxel(IR_YAML, &meas, "").unwrap();
        let b = fit_voxel(IR_YAML, &reversed, "").unwrap();
        assert_eq!(a[0], b[0], "T1 must be identical under reordering");
    }

    #[test]
    fn parse_aux_reads_scalars() {
        let a = parse_aux(r#"{"B1map": 1.2, "R1map": 0.9}"#).unwrap();
        assert_eq!(a.get("B1map"), Some(1.2));
        assert_eq!(a.get("R1map"), Some(0.9));
        assert!(parse_aux("").unwrap().get("B1map").is_none());
    }

    #[test]
    fn unknown_model_errs() {
        let err = fit_voxel("model: nope\n", "[]", "").unwrap_err();
        assert!(
            err.to_lowercase().contains("nope") || err.to_lowercase().contains("unknown"),
            "{}",
            err
        );
    }

    #[test]
    fn fit_volume_ir_single_voxel() {
        // 1x1x1x9 volume filled with a clean IR signal → T1 recovered.
        let (sig, ids) = ir_signal_and_ids();
        let dims = [1usize, 1, 1, 9];
        let maps = fit_volume(IR_YAML, &sig, dims, &ids, None, &[]).unwrap();
        let t1 = maps.iter().find(|(n, _)| n == "T1").expect("T1 map");
        assert_eq!(t1.1.len(), 1);
        assert!((t1.1[0] - 0.9).abs() < 1e-3, "T1: {}", t1.1[0]);
    }

    #[test]
    fn fit_volume_respects_mask() {
        let (sig, ids) = ir_signal_and_ids();
        // two voxels; mask out the second.
        let mut data = sig.clone();
        data.extend(std::iter::repeat_n(0.0, 9));
        let dims = [2usize, 1, 1, 9];
        let mask = [1u8, 0u8];
        let maps = fit_volume(IR_YAML, &data, dims, &ids, Some(&mask), &[]).unwrap();
        let t1 = &maps.iter().find(|(n, _)| n == "T1").unwrap().1;
        assert_eq!(t1.len(), 2);
        assert!((t1[0] - 0.9).abs() < 1e-3);
        assert!(t1[1].is_nan(), "masked voxel should be NaN, got {}", t1[1]);
    }

    const RAMANI_SIM_YAML: &str = "model: qmt_spgr\nqmt_spgr:\n  fitting:\n    use_r1map_to_constrain_r1f: false\nsim:\n  params: { F: 0.15, kr: 25.0, R1f: 1.0, R1r: 1.0, T2f: 0.028, T2r: 1.1e-5 }\n";

    #[test]
    fn sim_signal_returns_json() {
        let json = sim("signal", RAMANI_SIM_YAML).unwrap();
        assert!(
            json.contains("\"signal\""),
            "json: {}",
            &json[..json.len().min(120)]
        );
        // valid JSON
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn sim_single_voxel_returns_stats_json() {
        let cfg = format!(
            "{}  noise: {{ type: none }}\n  trials: 1\n",
            RAMANI_SIM_YAML
        );
        let json = sim("single-voxel", &cfg).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("stats").is_some(), "expected stats field");
    }

    #[test]
    fn sim_unknown_mode_errs() {
        assert!(sim("bogus", RAMANI_SIM_YAML).is_err());
    }
}
