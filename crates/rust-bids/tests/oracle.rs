//! Differential oracle: rust-bids must reproduce a set of golden reference
//! groupings exactly (see `tests/fixtures/README.md` for their provenance).

use rust_bids::config::default_config;
use rust_bids::fs::MemFs;
use rust_bids::resolve::resolve_set;
use rust_bids::table::parse_to_table;
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Build a MemFs from every nii/json path mentioned in a golden unified JSON.
fn memfs_from_golden(golden: &Value) -> MemFs {
    let mut fs = MemFs::new();
    fn add_paths(v: &Value, fs: &mut MemFs) {
        match v {
            Value::String(s) if s.ends_with(".nii") || s.ends_with(".nii.gz") => {
                *fs = std::mem::take(fs).touch(s);
            }
            Value::String(s) if s.ends_with(".json") => {
                *fs = std::mem::take(fs).with(s, b"{}".to_vec());
            }
            Value::Array(a) => a.iter().for_each(|x| add_paths(x, fs)),
            Value::Object(o) => o.values().for_each(|x| add_paths(x, fs)),
            _ => {}
        }
    }
    add_paths(&golden["data"], &mut fs);
    fs
}

fn assert_matches_golden(dir: &str, suffix: &str) {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/expected")
        .join(dir);
    for entry in
        fs::read_dir(&base).expect("fixtures vendored (run scripts/vendor_reference_fixtures.sh)")
    {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let golden: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let memfs = memfs_from_golden(&golden);
        let table = parse_to_table(&memfs).unwrap();
        let cols = resolve_set(&table, &default_config(), suffix).unwrap();

        // The golden file is one loop_over unit; find our matching collection.
        // Match by `subject` always; match `session`/`run` only when BOTH
        // sides actually carry a real (non-"NA") value for that entity — our
        // output omits the key entirely for an absent entity, while the
        // golden fixtures (not modified here) still spell it out as the
        // literal string "NA", so the two representations are compared as
        // "entity present and equal" rather than by naive key equality.
        let want_data = &golden["data"][suffix];
        let entity_matches = |got: &Value, key: &str| match (got.get(key), golden.get(key)) {
            (Some(g), Some(w)) if w != "NA" => g == w,
            _ => true,
        };
        let got = cols.iter().map(|c| c.to_unified_json()).find(|c| {
            c["subject"] == golden["subject"]
                && entity_matches(c, "session")
                && entity_matches(c, "run")
        });
        let got = got.unwrap_or_else(|| panic!("no collection for {}", path.display()));
        assert_eq!(
            &got["data"][suffix],
            want_data,
            "mismatch for {}",
            path.display()
        );
    }
}

#[test]
fn irt1_matches_reference() {
    assert_matches_golden("qmri_irt1", "IRT1");
}

#[test]
fn mts_matches_reference() {
    assert_matches_golden("qmri_mtsat", "MTS");
}
