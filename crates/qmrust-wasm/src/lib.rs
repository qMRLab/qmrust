//! qmrust-wasm — browser bindings for qmrust. The pure marshalling layer lives
//! in [`api`] (native-testable); the `#[wasm_bindgen]` layer in `wasm` is
//! compiled only for `wasm32`.

pub mod api;

#[cfg(target_arch = "wasm32")]
mod wasm;
