//! Browser smoke tests, run via `wasm-pack test --headless --chrome` in CI.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;
wasm_bindgen_test_configure!(run_in_browser);

const IR_YAML: &str = "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800, 950, 1100, 1250, 1400, 1700]\n";

#[wasm_bindgen_test]
fn forward_then_fit_voxel_roundtrips() {
    let sig = qmrust_wasm::api::forward(IR_YAML, &[900.0, 500.0, -1000.0], "").unwrap();
    assert_eq!(sig.len(), 9);
    let out = qmrust_wasm::api::fit_voxel(IR_YAML, &sig, "").unwrap();
    assert!((out[0] - 900.0).abs() < 1.0);
}

// fit_volume uses rayon; requires the threaded build's thread pool.
#[cfg(feature = "threads")]
#[wasm_bindgen_test]
fn fit_volume_single_voxel() {
    let sig = qmrust_wasm::api::forward(IR_YAML, &[900.0, 500.0, -1000.0], "").unwrap();
    let maps = qmrust_wasm::api::fit_volume(IR_YAML, &sig, [1, 1, 1, 9], None, &[]).unwrap();
    assert!(maps.iter().any(|(n, _)| n == "T1"));
}
