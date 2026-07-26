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
/// name to a C-order `[nx,ny,nz]` array. `protocol_json` is the acquisition
/// resolved from the data — pass `resolve_bids`'s `protocol_json` for a BIDS
/// dataset, or `""` when the recipe itself carries the protocol. Returns
/// `{ name: number[] }`.
#[wasm_bindgen]
pub fn fit_volume(
    cfg_yaml: &str,
    data: &[f64],
    dims: &[usize],
    volume_ids_json: &str,
    mask: Option<Vec<u8>>,
    aux_json: &str,
    protocol_json: &str,
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
    let maps = api::fit_volume(
        cfg_yaml,
        data,
        d,
        volume_ids_json,
        mask.as_deref(),
        &aux,
        protocol_json,
    )
    .map_err(|e| JsError::new(&e))?;
    let obj: std::collections::BTreeMap<String, Vec<f64>> = maps.into_iter().collect();
    serde_wasm_bindgen::to_value(&obj).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn sim(mode: &str, cfg_yaml: &str) -> Result<String, JsError> {
    api::sim(mode, cfg_yaml).map_err(|e| JsError::new(&e))
}

/// Resolve a whole BIDS dataset held in memory against the model `cfg_yaml`
/// names, through the same `rust-bids` logic the CLI's `--bids-dir` path uses.
///
/// `files` is the dataset as a JS `Map`/object of dataset-relative path →
/// `Uint8Array` — whatever an unzipped archive or a dropped directory produced,
/// with paths already relative to the directory holding
/// `dataset_description.json`. `grouping_yaml` (or `""`) overrides the default
/// grouping manifest.
///
/// Returns one entry per resolved collection, each naming which files matter and
/// what they mean — never pixel data. Read those paths' bytes from your own map,
/// then pass the entry's `volume_ids_json` and `protocol_json` to `fit_volume`.
/// An empty array means the dataset holds nothing this model can fit, which is an
/// outcome to report rather than an error.
#[wasm_bindgen]
pub fn resolve_bids(
    files: JsValue,
    cfg_yaml: &str,
    grouping_yaml: &str,
) -> Result<JsValue, JsError> {
    let files: std::collections::BTreeMap<String, Vec<u8>> = serde_wasm_bindgen::from_value(files)
        .map_err(|e| JsError::new(&format!("files must be path -> bytes: {e}")))?;
    let grouping = Some(grouping_yaml).filter(|s| !s.trim().is_empty());
    let resolved = crate::bids::resolve_bids(files.into_iter().collect(), cfg_yaml, grouping)
        .map_err(|e| JsError::new(&e))?;
    serde_wasm_bindgen::to_value(&resolved).map_err(|e| JsError::new(&e.to_string()))
}
