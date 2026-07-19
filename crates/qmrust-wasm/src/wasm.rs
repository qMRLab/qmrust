//! Thin `#[wasm_bindgen]` layer — converts JS values and delegates to `api`.
//! Compiled only for `wasm32`.

use crate::api;
use wasm_bindgen::prelude::*;

/// Multithreading entry point (feature `threads`, enabled by CI on nightly).
/// Call `await initThreadPool(navigator.hardwareConcurrency)` once before
/// `fit_volume`. Requires the page to be cross-origin isolated (COOP/COEP).
#[cfg(feature = "threads")]
pub use wasm_bindgen_rayon::init_thread_pool;

#[wasm_bindgen]
pub fn list_models() -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(&api::list_models()).map_err(|e| JsError::new(&e.to_string()))
}

/// `measurement_json` is the identity-keyed measurement: a `{ role: value }`
/// object for `Named` models, or a `[{ params, value }, ...]` array for
/// `Series` models. Returns values in the model's `output_names` order.
#[wasm_bindgen]
pub fn fit_voxel(
    cfg_yaml: &str,
    measurement_json: &str,
    aux_json: &str,
) -> Result<Vec<f64>, JsError> {
    api::fit_voxel(cfg_yaml, measurement_json, aux_json).map_err(|e| JsError::new(&e))
}

/// Noise-free forward measurement for `params`, JSON-encoded (see `fit_voxel`).
#[wasm_bindgen]
pub fn forward(cfg_yaml: &str, params: &[f64], aux_json: &str) -> Result<String, JsError> {
    api::forward(cfg_yaml, params, aux_json).map_err(|e| JsError::new(&e))
}

/// `dims` is `[nx, ny, nz, nt]`. `volume_ids_json` supplies each volume's
/// identity (a JSON array of role names for `Named`, or of param-row objects
/// for `Series`), length `nt`. `aux_json` is a JSON object mapping an input
/// name to a C-order `[nx,ny,nz]` array. Returns `{ name: number[] }`.
#[wasm_bindgen]
pub fn fit_volume(
    cfg_yaml: &str,
    data: &[f64],
    dims: &[usize],
    volume_ids_json: &str,
    mask: Option<Vec<u8>>,
    aux_json: &str,
) -> Result<JsValue, JsError> {
    if dims.len() != 4 {
        return Err(JsError::new("dims must have length 4 [nx,ny,nz,nt]"));
    }
    let d = [dims[0], dims[1], dims[2], dims[3]];
    // aux: JSON object of name -> number[] (flat [nx,ny,nz]).
    let aux_map: std::collections::BTreeMap<String, Vec<f64>> = if aux_json.trim().is_empty() {
        Default::default()
    } else {
        serde_json::from_str(aux_json).map_err(|e| JsError::new(&format!("aux JSON: {}", e)))?
    };
    let aux: Vec<(String, Vec<f64>)> = aux_map.into_iter().collect();
    let maps = api::fit_volume(cfg_yaml, data, d, volume_ids_json, mask.as_deref(), &aux)
        .map_err(|e| JsError::new(&e))?;
    let obj: std::collections::BTreeMap<String, Vec<f64>> = maps.into_iter().collect();
    serde_wasm_bindgen::to_value(&obj).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn sim(mode: &str, cfg_yaml: &str) -> Result<String, JsError> {
    api::sim(mode, cfg_yaml).map_err(|e| JsError::new(&e))
}
