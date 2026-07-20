//! Browser smoke tests, run via `wasm-pack test --headless --chrome` in CI.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

// Inversion times in seconds (BIDS units).
const IR_YAML: &str = "model: inversion_recovery\nmethod: complex\ninversion_times: [0.350, 0.500, 0.650, 0.800, 0.950, 1.100, 1.250, 1.400, 1.700]\n";

#[wasm_bindgen_test]
fn forward_then_fit_voxel_roundtrips() {
    // `forward` returns the identity-keyed measurement as JSON: a Series of
    // `{ params: { InversionTime }, value }` samples. Fitting it back recovers T1.
    let meas = qmrust_wasm::api::forward(IR_YAML, &[0.9, 500.0, -1000.0], "").unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&meas).unwrap();
    assert_eq!(arr.len(), 9);
    let out = qmrust_wasm::api::fit_voxel(IR_YAML, &meas, "").unwrap();
    assert!((out[0] - 0.9).abs() < 1e-3);
}

// fit_volume uses rayon; requires the threaded build's thread pool.
#[cfg(feature = "threads")]
#[wasm_bindgen_test]
fn fit_volume_single_voxel() {
    // Raw per-volume values plus their identity rows (InversionTime), the two
    // inputs `fit_volume` needs: the volume data and each volume's identity.
    let meas = qmrust_wasm::api::forward(IR_YAML, &[0.9, 500.0, -1000.0], "").unwrap();
    let arr: Vec<serde_json::Value> = serde_json::from_str(&meas).unwrap();
    let data: Vec<f64> = arr.iter().map(|s| s["value"].as_f64().unwrap()).collect();
    let rows: Vec<&serde_json::Value> = arr.iter().map(|s| &s["params"]).collect();
    let ids = serde_json::to_string(&rows).unwrap();
    let maps = qmrust_wasm::api::fit_volume(IR_YAML, &data, [1, 1, 1, 9], &ids, None, &[]).unwrap();
    assert!(maps.iter().any(|(n, _)| n == "T1"));
}
