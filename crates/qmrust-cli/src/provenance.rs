//! Reproducibility metadata for BIDS-derivatives output sidecars: software +
//! build environment, the input volumes and resolved protocol a fit actually
//! used, and its execution timestamp/duration. Lives in the CLI (not
//! `qmrust-core`) because it depends on `std::time`, `std::env`, and
//! build-script-captured values — none of which belong in the pure core.

use serde_json::{json, Value};

/// The proleptic-Gregorian civil date for a given day count since the Unix
/// epoch (1970-01-01) — the inverse of Howard Hinnant's `days_from_civil`.
/// Returns `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Format a Unix timestamp (seconds since 1970-01-01T00:00:00Z) as UTC
/// ISO-8601 with second precision: `"YYYY-MM-DDTHH:MM:SSZ"`.
pub fn unix_to_iso8601(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let secs_of_day = secs.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The `GeneratedBy` entry shared by every map sidecar and
/// `dataset_description.json`: software identity, build environment, and the
/// exact commit built from.
pub fn generated_by() -> Value {
    json!({
        "Name": "qmrust",
        "Version": env!("CARGO_PKG_VERSION"),
        "Description": "Native-Rust quantitative-MRI fitting",
        "CodeURL": env!("CARGO_PKG_REPOSITORY"),
        "CommitHash": env!("QMRUST_GIT_COMMIT"),
        "Environment": {
            "RustVersion": env!("QMRUST_RUSTC_VERSION"),
            "Target": env!("QMRUST_TARGET"),
            "BuildProfile": env!("QMRUST_PROFILE"),
            "OperatingSystem": std::env::consts::OS,
            "Architecture": std::env::consts::ARCH,
        }
    })
}

/// Everything a single `run_fit_bids` fit knows about how it produced its
/// output maps, independent of which map's sidecar is being written.
pub struct FitProvenance {
    pub model: String,
    pub config_json: Value,
    pub protocol_json: Value,
    pub sources: Vec<String>,
    pub executed_at_unix: i64,
    pub duration_s: f64,
}

impl FitProvenance {
    /// The full sidecar JSON for one output map, given that map's physical
    /// unit (from `Model::bids_outputs()`; `""` for a unitless quantity).
    pub fn sidecar(&self, units: &str) -> Value {
        json!({
            "GeneratedBy": [generated_by()],
            "Sources": self.sources,
            "Model": self.model,
            "Parameters": self.config_json,
            "Protocol": self.protocol_json,
            "Units": units,
            "DateExecuted": unix_to_iso8601(self.executed_at_unix),
            "FitDurationSeconds": self.duration_s,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_formats_as_1970() {
        assert_eq!(unix_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn known_timestamp_round_trips() {
        assert_eq!(unix_to_iso8601(1_600_000_000), "2020-09-13T12:26:40Z");
    }

    #[test]
    fn sidecar_has_expected_provenance_fields() {
        let prov = FitProvenance {
            model: "inversion_recovery".to_string(),
            config_json: json!({"model": "inversion_recovery"}),
            protocol_json: json!({"InversionTime": [0.35, 0.5]}),
            sources: vec!["bids::sub-01/anat/sub-01_inv-1_IRT1.nii.gz".to_string()],
            executed_at_unix: 1_600_000_000,
            duration_s: 2.14,
        };
        let sidecar = prov.sidecar("s");

        assert_eq!(sidecar["GeneratedBy"][0]["Name"], "qmrust");
        assert!(!sidecar["GeneratedBy"][0]["Version"]
            .as_str()
            .unwrap()
            .is_empty());
        assert!(sidecar["GeneratedBy"][0]["Environment"]["OperatingSystem"].is_string());
        assert_eq!(
            sidecar["Sources"],
            json!(["bids::sub-01/anat/sub-01_inv-1_IRT1.nii.gz"])
        );
        assert_eq!(sidecar["Model"], "inversion_recovery");
        assert_eq!(sidecar["Units"], "s");
        let date = sidecar["DateExecuted"].as_str().unwrap();
        assert!(
            regex_like_iso8601(date),
            "DateExecuted {date} doesn't look like ISO-8601"
        );
        assert!(sidecar["FitDurationSeconds"].is_number());
    }

    /// Hand-rolled check for `^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$` (no regex
    /// dependency in this crate).
    fn regex_like_iso8601(s: &str) -> bool {
        let bytes = s.as_bytes();
        bytes.len() == 20
            && bytes[0..4].iter().all(u8::is_ascii_digit)
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(u8::is_ascii_digit)
            && bytes[7] == b'-'
            && bytes[8..10].iter().all(u8::is_ascii_digit)
            && bytes[10] == b'T'
            && bytes[11..13].iter().all(u8::is_ascii_digit)
            && bytes[13] == b':'
            && bytes[14..16].iter().all(u8::is_ascii_digit)
            && bytes[16] == b':'
            && bytes[17..19].iter().all(u8::is_ascii_digit)
            && bytes[19] == b'Z'
    }
}
