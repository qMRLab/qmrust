//! Sf free-pool saturation: single-pulse ODE (computeSf.m), the CacheSf axis
//! grid, the precomputed table (BuildSfTable.m), trilinear GetSf, and a
//! reference loader for validation.

use ndarray::Array3;
use rayon::prelude::*;

#[cfg(not(target_arch = "wasm32"))]
use anyhow::{Context, Result};
#[cfg(not(target_arch = "wasm32"))]
use matfile::MatFile;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

use super::ode::{bloch_no_mt_deriv, rk_bs23};
use super::pulse::{GaussHannPulse, GAMMA};

/// Saturation of the free pool after one MT pulse: integrate BlochNoMT over the
/// pulse from M0=[0,0,1] and return the final Mz (computeSf.m).
pub fn compute_sf(pulse: &GaussHannPulse, angle_deg: f64, offset_hz: f64, t2f: f64) -> f64 {
    let amp = pulse.amp(angle_deg);
    let rhs = |t: f64, m: &[f64; 3]| {
        let omega = GAMMA * amp * pulse.envelope(t);
        bloch_no_mt_deriv(m, t2f, offset_hz, omega)
    };
    let m = rk_bs23(&rhs, 0.0, pulse.trf, [0.0, 0.0, 1.0], 1e-3, 1e-6);
    m[2]
}

/// The fixed T2f grid used by CacheSf.m.
pub fn t2f_grid() -> Vec<f64> {
    vec![
        0.0010, 0.0050, 0.0100, 0.0150, 0.0200, 0.0250, 0.0300, 0.0350, 0.0400, 0.0450, 0.0500,
        0.0550, 0.0600, 0.0650, 0.0700, 0.0750, 0.0800, 0.0850, 0.0900, 0.2500, 0.5000, 1.0000,
    ]
}

fn unique_sorted(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    v
}

/// CacheSf.m angle/offset grid construction. Returns (angles, offsets, t2f).
pub fn build_sf_axes(
    protocol_angles: &[f64],
    protocol_offsets: &[f64],
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let uang = unique_sorted(protocol_angles.to_vec());
    let max_ang = uang.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mut sf_angles = vec![0.0];
    for a in &uang {
        sf_angles.push(0.75 * a);
        sf_angles.push(*a);
        sf_angles.push(1.25 * a);
    }
    sf_angles.push(1.5 * max_ang);
    let sf_angles = unique_sorted(sf_angles);

    let uoff = unique_sorted(protocol_offsets.to_vec());
    let max_off = uoff.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let with_zero: Vec<f64> = std::iter::once(0.0).chain(uoff.iter().cloned()).collect();
    let max_off_step = 100.0;
    let mut sf_offsets = vec![100.0];
    for i in 1..with_zero.len() {
        sf_offsets.push(0.5 * (with_zero[i] + with_zero[i - 1]));
        sf_offsets.push(with_zero[i] - max_off_step);
        sf_offsets.push(with_zero[i]);
        sf_offsets.push(with_zero[i] + max_off_step);
    }
    sf_offsets.push(max_off + 1000.0);
    let sf_offsets = unique_sorted(sf_offsets);

    (sf_angles, sf_offsets, t2f_grid())
}

/// Precomputed Sf lookup table over (angle × offset × T2f).
pub struct SfTable {
    pub angles: Vec<f64>,
    pub offsets: Vec<f64>,
    pub t2f: Vec<f64>,
    pub values: Array3<f64>,
}

/// Build the Sf table by integrating the pulse ODE at every grid node
/// (BuildSfTable.m), parallelized across nodes.
pub fn build_sf_table(
    pulse: &GaussHannPulse,
    angles: &[f64],
    offsets: &[f64],
    t2f: &[f64],
) -> SfTable {
    let (na, no, nt) = (angles.len(), offsets.len(), t2f.len());
    let flat: Vec<f64> = (0..na * no * nt)
        .into_par_iter()
        .map(|idx| {
            let i = idx / (no * nt);
            let j = (idx / nt) % no;
            let k = idx % nt;
            compute_sf(pulse, angles[i], offsets[j], t2f[k])
        })
        .collect();
    let values = Array3::from_shape_vec((na, no, nt), flat).expect("shape matches");
    SfTable {
        angles: angles.to_vec(),
        offsets: offsets.to_vec(),
        t2f: t2f.to_vec(),
        values,
    }
}

/// Find (i0, i1, frac) bracketing `x` in ascending `axis`. Returns None if
/// `x` is outside [min, max].
fn bracket(axis: &[f64], x: f64) -> Option<(usize, usize, f64)> {
    if x < axis[0] - 1e-9 || x > axis[axis.len() - 1] + 1e-9 {
        return None;
    }
    for i in 1..axis.len() {
        if x <= axis[i] {
            let (lo, hi) = (axis[i - 1], axis[i]);
            let f = if (hi - lo).abs() < 1e-30 {
                0.0
            } else {
                (x - lo) / (hi - lo)
            };
            return Some((i - 1, i, f));
        }
    }
    Some((axis.len() - 2, axis.len() - 1, 1.0))
}

impl SfTable {
    /// Trilinear interpolation of Sf at (angle, offset, t2f). Falls back to a
    /// direct compute_sf when the query is outside the grid (GetSf.m).
    pub fn get(&self, angle: f64, offset: f64, t2f: f64, pulse: &GaussHannPulse) -> f64 {
        let (ba, bo, bt) = match (
            bracket(&self.angles, angle),
            bracket(&self.offsets, offset),
            bracket(&self.t2f, t2f),
        ) {
            (Some(a), Some(o), Some(t)) => (a, o, t),
            _ => return compute_sf(pulse, angle, offset, t2f),
        };
        let v = &self.values;
        let corner = |i: usize, j: usize, k: usize| v[[i, j, k]];
        let (a0, a1, fa) = ba;
        let (o0, o1, fo) = bo;
        let (t0, t1, ft) = bt;
        let lerp = |a: f64, b: f64, f: f64| a + (b - a) * f;
        // interpolate along t2f, then offset, then angle
        let c00 = lerp(corner(a0, o0, t0), corner(a0, o0, t1), ft);
        let c01 = lerp(corner(a0, o1, t0), corner(a0, o1, t1), ft);
        let c10 = lerp(corner(a1, o0, t0), corner(a1, o0, t1), ft);
        let c11 = lerp(corner(a1, o1, t0), corner(a1, o1, t1), ft);
        let c0 = lerp(c00, c01, fo);
        let c1 = lerp(c10, c11, fo);
        lerp(c0, c1, fa)
    }
}

impl SfTable {
    /// Max absolute value difference vs another table. None if grids differ in size.
    #[allow(dead_code)]
    pub fn max_abs_diff(&self, other: &SfTable) -> Option<f64> {
        if self.values.dim() != other.values.dim() {
            return None;
        }
        let mut m = 0.0_f64;
        for (a, b) in self.values.iter().zip(other.values.iter()) {
            m = m.max((a - b).abs());
        }
        Some(m)
    }
}

/// Extract f64 values from a matfile NumericData. Duplicated from
/// qmrust-cli's `io::mat` (which this core validation helper cannot depend
/// on) — keep in sync if changed.
#[cfg(not(target_arch = "wasm32"))]
fn numeric_to_f64(data: &matfile::NumericData) -> Vec<f64> {
    use matfile::NumericData::*;
    match data {
        Double { real, .. } => real.clone(),
        Single { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int8 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt8 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int16 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt16 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int32 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt32 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        Int64 { real, .. } => real.iter().map(|&v| v as f64).collect(),
        UInt64 { real, .. } => real.iter().map(|&v| v as f64).collect(),
    }
}

/// Read a reference Sf table from a .mat file exported as flat arrays:
/// `values` (nA×nO×nT, column-major), `angles`, `offsets`, `T2f`. Returns
/// Ok(None) if the required arrays are not present (e.g. a struct-only file).
#[cfg(not(target_arch = "wasm32"))]
#[allow(dead_code)]
pub fn load_reference_arrays(path: &Path) -> Result<Option<SfTable>> {
    let file = std::fs::File::open(path).with_context(|| format!("open {:?}", path))?;
    let mat = MatFile::parse(file).with_context(|| format!("parse {:?}", path))?;
    let (values, angles, offsets, t2f) = match (
        mat.find_by_name("values"),
        mat.find_by_name("angles"),
        mat.find_by_name("offsets"),
        mat.find_by_name("T2f"),
    ) {
        (Some(v), Some(a), Some(o), Some(t)) => (v, a, o, t),
        _ => return Ok(None),
    };
    let a = numeric_to_f64(angles.data());
    let o = numeric_to_f64(offsets.data());
    let t = numeric_to_f64(t2f.data());
    let (na, no, nt) = (a.len(), o.len(), t.len());
    let raw = numeric_to_f64(values.data());
    if raw.len() != na * no * nt {
        return Ok(None);
    }
    // MATLAB column-major (nA,nO,nT): idx = i + j*nA + k*nA*nO
    let mut arr = ndarray::Array3::<f64>::zeros((na, no, nt));
    for k in 0..nt {
        for j in 0..no {
            for i in 0..na {
                arr[[i, j, k]] = raw[i + j * na + k * na * no];
            }
        }
    }
    Ok(Some(SfTable {
        angles: a,
        offsets: o,
        t2f: t,
        values: arr,
    }))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn far_off_resonance_barely_saturates() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        // Very large offset → little saturation → Sf near 1.
        let sf = compute_sf(&p, 142.0, 100000.0, 0.03);
        assert!(
            sf > 0.9 && sf <= 1.0 + 1e-9,
            "far off-res Sf ~1, got {}",
            sf
        );
    }

    #[test]
    fn larger_flip_saturates_more() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let sf_small = compute_sf(&p, 142.0, 2732.0, 0.03);
        let sf_large = compute_sf(&p, 426.0, 2732.0, 0.03);
        assert!(
            sf_large < sf_small,
            "larger flip -> smaller Sf ({} !< {})",
            sf_large,
            sf_small
        );
    }

    #[test]
    fn axes_match_cachesf_for_default_protocol() {
        // default protocol angles {142,426}, offsets {443,1088,2732,6862,17235}
        let angles = [
            142.0, 426.0, 142.0, 426.0, 142.0, 426.0, 142.0, 426.0, 142.0, 426.0,
        ];
        let offsets = [
            443.0, 443.0, 1088.0, 1088.0, 2732.0, 2732.0, 6862.0, 6862.0, 17235.0, 17235.0,
        ];
        let (a, o, t) = build_sf_axes(&angles, &offsets);

        // Angles: {0, 0.75*142, 142, 1.25*142, 0.75*426, 426, 1.25*426, 1.5*426}
        let expect_a = [0.0, 106.5, 142.0, 177.5, 319.5, 426.0, 532.5, 639.0];
        assert_eq!(a.len(), expect_a.len());
        for (g, e) in a.iter().zip(expect_a.iter()) {
            assert!((g - e).abs() < 1e-9, "angle axis {} != {}", g, e);
        }
        // ascending + unique
        assert!(
            o.windows(2).all(|w| w[0] < w[1]),
            "offsets ascending/unique"
        );
        // T2f fixed list
        assert_eq!(t.len(), 22);
        assert!((t[0] - 0.0010).abs() < 1e-12 && (t[21] - 1.0).abs() < 1e-12);
        // offsets start at 100, end at max+1000
        assert!((o[0] - 100.0).abs() < 1e-9);
        assert!((*o.last().unwrap() - (17235.0 + 1000.0)).abs() < 1e-9);
    }

    #[test]
    fn get_returns_node_value_exactly() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let angles = vec![106.5, 142.0, 177.5];
        let offsets = vec![1000.0, 2732.0, 5000.0];
        let t2f = vec![0.02, 0.03, 0.05];
        let table = build_sf_table(&p, &angles, &offsets, &t2f);
        // At an exact grid node, get() == the stored value.
        let node = table.values[[1, 1, 1]];
        let got = table.get(142.0, 2732.0, 0.03, &p);
        assert!(
            (got - node).abs() < 1e-9,
            "node interp {} != stored {}",
            got,
            node
        );
    }

    #[test]
    fn get_interpolates_within_range() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let angles = vec![106.5, 142.0, 177.5];
        let offsets = vec![1000.0, 2732.0, 5000.0];
        let t2f = vec![0.02, 0.03, 0.05];
        let table = build_sf_table(&p, &angles, &offsets, &t2f);
        // Midpoint query stays between neighboring node values.
        let lo = table.values[[1, 0, 1]];
        let hi = table.values[[1, 1, 1]];
        let mid = table.get(142.0, 0.5 * (1000.0 + 2732.0), 0.03, &p);
        let (a, b) = if lo < hi { (lo, hi) } else { (hi, lo) };
        assert!(
            mid >= a - 1e-9 && mid <= b + 1e-9,
            "interp {} not in [{}, {}]",
            mid,
            a,
            b
        );
    }

    #[test]
    fn max_abs_diff_zero_for_identical() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let (sa, so, st) = build_sf_axes(&[142.0, 426.0], &[443.0, 2732.0]);
        let a = build_sf_table(&p, &sa, &so, &st);
        let b = build_sf_table(&p, &sa, &so, &st);
        assert_eq!(a.max_abs_diff(&b), Some(0.0));
    }

    #[test]
    fn max_abs_diff_none_on_axis_mismatch() {
        let p = GaussHannPulse::new(0.0102, 200.0);
        let (sa, so, st) = build_sf_axes(&[142.0, 426.0], &[443.0, 2732.0]);
        let a = build_sf_table(&p, &sa, &so, &st);
        let (sa2, so2, st2) = build_sf_axes(&[142.0], &[443.0]);
        let b = build_sf_table(&p, &sa2, &so2, &st2);
        assert_eq!(a.max_abs_diff(&b), None);
    }

    // --- Minimal MAT5 (uncompressed, level-5) writer, used only to build
    // in-test fixtures for load_reference_arrays without depending on a
    // checked-in binary .mat file. ---

    fn align8(buf: &mut Vec<u8>) {
        while !buf.len().is_multiple_of(8) {
            buf.push(0);
        }
    }

    /// Build a top-level "Matrix" data element containing one Double array.
    fn mat_double_array(name: &str, dims: &[i32], data: &[f64]) -> Vec<u8> {
        let mut content = Vec::new();

        // Array flags subelement: tag_type=UInt32(6), tag_len=8, flags_and_class, nzmax.
        content.extend_from_slice(&6u32.to_le_bytes());
        content.extend_from_slice(&8u32.to_le_bytes());
        content.extend_from_slice(&6u32.to_le_bytes()); // class = Double(6), no flags
        content.extend_from_slice(&0u32.to_le_bytes()); // nzmax

        // Dimensions subelement: tag_type=Int32(5), size=4*ndims, values, pad.
        let dims_bytes = (4 * dims.len()) as u32;
        content.extend_from_slice(&5u32.to_le_bytes());
        content.extend_from_slice(&dims_bytes.to_le_bytes());
        for d in dims {
            content.extend_from_slice(&d.to_le_bytes());
        }
        align8(&mut content);

        // Array name subelement: tag_type=Int8(1), size=name.len(), bytes, pad.
        content.extend_from_slice(&1u32.to_le_bytes());
        content.extend_from_slice(&(name.len() as u32).to_le_bytes());
        content.extend_from_slice(name.as_bytes());
        align8(&mut content);

        // Real part: tag_type=Double(9), size=8*n, values, pad.
        let data_bytes = (8 * data.len()) as u32;
        content.extend_from_slice(&9u32.to_le_bytes());
        content.extend_from_slice(&data_bytes.to_le_bytes());
        for v in data {
            content.extend_from_slice(&v.to_le_bytes());
        }
        align8(&mut content);

        // Top-level element tag: type=Matrix(14), size=content.len().
        let mut out = Vec::new();
        out.extend_from_slice(&14u32.to_le_bytes());
        out.extend_from_slice(&(content.len() as u32).to_le_bytes());
        out.extend_from_slice(&content);
        align8(&mut out);
        out
    }

    /// Assemble a minimal, valid, uncompressed MAT5 file from a set of
    /// pre-built top-level data elements.
    fn mat_file_bytes(elements: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        let mut text = b"MATLAB 5.0 MAT-file, qmrust test fixture".to_vec();
        text.resize(116, b' ');
        out.extend_from_slice(&text);
        out.extend_from_slice(&[0u8; 8]); // subsystem data offset (unused)
        out.extend_from_slice(&0x0100u16.to_le_bytes()); // version
        out.extend_from_slice(b"IM"); // little-endian marker
        assert_eq!(out.len(), 128);
        for el in elements {
            out.extend_from_slice(el);
        }
        out
    }

    #[test]
    fn load_reference_arrays_rejects_length_mismatch_without_panic() {
        // angles(2) x offsets(2) x t2f(1) => 4 expected elements, but
        // `values` only carries 3 -- a corrupted/mismatched export. Before
        // the fix this would panic on out-of-bounds indexing; now it must
        // be treated as an absent/invalid reference.
        let angles = mat_double_array("angles", &[2, 1], &[100.0, 200.0]);
        let offsets = mat_double_array("offsets", &[2, 1], &[10.0, 20.0]);
        let t2f = mat_double_array("T2f", &[1, 1], &[0.03]);
        let values = mat_double_array("values", &[3, 1], &[1.0, 2.0, 3.0]);
        let bytes = mat_file_bytes(&[values, angles, offsets, t2f]);

        let dir = std::env::temp_dir();
        let path = dir.join(format!("qmrust_sf_mismatch_{}.mat", std::process::id()));
        std::fs::write(&path, &bytes).expect("write fixture");
        let result = load_reference_arrays(&path);
        let _ = std::fs::remove_file(&path);

        match result {
            Ok(None) => {}
            other => panic!(
                "expected Ok(None) for mismatched lengths, got {:?}",
                other.map(|o| o.is_some())
            ),
        }
    }

    #[test]
    fn load_reference_arrays_loads_well_formed_reference() {
        // angles(2) x offsets(1) x t2f(1) => 2 elements, matching `values`.
        let angles = mat_double_array("angles", &[2, 1], &[100.0, 200.0]);
        let offsets = mat_double_array("offsets", &[1, 1], &[10.0]);
        let t2f = mat_double_array("T2f", &[1, 1], &[0.03]);
        let values = mat_double_array("values", &[2, 1, 1], &[1.5, 2.5]);
        let bytes = mat_file_bytes(&[values, angles, offsets, t2f]);

        let dir = std::env::temp_dir();
        let path = dir.join(format!("qmrust_sf_wellformed_{}.mat", std::process::id()));
        std::fs::write(&path, &bytes).expect("write fixture");
        let result = load_reference_arrays(&path);
        let _ = std::fs::remove_file(&path);

        let table = result
            .expect("parse should succeed")
            .expect("reference should be present");
        assert_eq!(table.angles, vec![100.0, 200.0]);
        assert_eq!(table.offsets, vec![10.0]);
        assert_eq!(table.t2f, vec![0.03]);
        assert_eq!(table.values[[0, 0, 0]], 1.5);
        assert_eq!(table.values[[1, 0, 0]], 2.5);
    }
}
