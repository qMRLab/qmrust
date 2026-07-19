//! CLI command handlers.
//!
//! These are the imperative-shell entry points behind each `qmrust`
//! subcommand. `main.rs` parses arguments and calls straight into here, keeping
//! the binary thin and this logic testable as part of the library.

use anyhow::{bail, Result};
use ndarray::{Array3, Array4};
use nifti::NiftiHeader;
use owo_colors::{OwoColorize, Stream::Stderr};
use std::path::{Path, PathBuf};

use crate::io;
use crate::io::bids_fs::StdFs;
use qmrust_core::core::model::{MeasurementKind, Model, Protocol, VolumeId};
use qmrust_core::engine::AuxMaps;
use qmrust_core::models;
use rust_bids::{Collection, GroupedData};

/// Build per-volume identities for `engine::run` from a model's declared
/// measurement kind and the resolved protocol — dispatch on the measurement
/// shape, never on the model name. Every volume is labeled with a real
/// identity, so the model's `fit` always assembles by value, never by position.
///
/// - `Named { roles }`: volume `i` takes role `roles[i]` (requires exactly
///   `roles.len()` volumes).
/// - `Series { rows }`: prefer externally-resolved per-volume rows
///   (`proto.volumes`, e.g. `.mat` sidecar TIs); otherwise fall back to the
///   model's own canonical identity rows. Both carry populated params — an
///   empty/positional row is never emitted. (The future BIDS shell supplies
///   sidecar-derived rows here.)
fn build_volume_ids(
    kind: MeasurementKind,
    proto: &Protocol,
    n_volumes: usize,
) -> Result<Vec<VolumeId>> {
    match kind {
        MeasurementKind::Named { roles } => {
            if roles.len() != n_volumes {
                bail!(
                    "Data has {} volumes but model expects {} named volumes ({:?})",
                    n_volumes,
                    roles.len(),
                    roles
                );
            }
            Ok(roles.iter().map(|&r| VolumeId::Role(r)).collect())
        }
        MeasurementKind::Series { rows } => {
            let source = if proto.volumes.len() == n_volumes {
                &proto.volumes
            } else {
                &rows
            };
            if source.len() != n_volumes {
                bail!(
                    "Data has {} volumes but the model's series protocol has {} rows",
                    n_volumes,
                    source.len()
                );
            }
            Ok(source.iter().cloned().map(VolumeId::Params).collect())
        }
    }
}

/// Read a config file from disk and parse + validate it, also returning the
/// raw YAML tree so per-model builders can pull their own sub-config. File
/// I/O lives here (in the CLI); `qmrust_core::config::parse_config` is a
/// pure parser with no `std::fs` dependency, keeping the core crate
/// wasm-clean.
fn load_config_raw(
    path: &std::path::Path,
) -> anyhow::Result<(qmrust_core::config::Config, serde_yaml::Value)> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read {:?}: {}", path, e))?;
    qmrust_core::config::parse_config(&contents)
}

/// Print the fully-resolved effective config (defaults applied + validated) as
/// YAML. For qmt_spgr this prints the complete protocol/timing/pulse/fitting
/// block that a short config expands to.
pub fn run_dump_config(config_path: PathBuf) -> Result<()> {
    let (cfg, raw) = load_config_raw(&config_path)?;
    match cfg.model.as_str() {
        "qmt_spgr" => {
            let mut q: qmrust_core::models::qmt_spgr::config::QmtSpgrConfig =
                match raw.get("qmt_spgr") {
                    Some(sub) => serde_yaml::from_value(sub.clone())?,
                    None => Default::default(),
                };
            q.validate()?;
            println!("model: qmt_spgr");
            println!("qmt_spgr:");
            for line in serde_yaml::to_string(&q)?.lines() {
                println!("  {}", line);
            }
        }
        "inversion_recovery" => {
            // Materialize defaults + validate via the typed config (matching the
            // qmt_spgr branch and the command's "fully-resolved" contract),
            // rather than echoing the raw file verbatim.
            let mut ir: qmrust_core::models::inversion_recovery::config::IrConfig =
                serde_yaml::from_value(raw.clone())?;
            ir.validate()?;
            println!("model: inversion_recovery");
            print!("{}", serde_yaml::to_string(&ir)?);
        }
        other => {
            // Unknown models are rejected by validate() before reaching here;
            // fall back to echoing the resolved raw tree (already carries `model:`).
            print!("{}", serde_yaml::to_string(&raw)?);
            let _ = other;
        }
    }
    Ok(())
}

/// Build the Sf table for a config's protocol and write raw f64 values +
/// axes (to `<output>.axes.txt`) for external validation against qMRLab.
pub fn run_dump_sf(config_path: PathBuf, output: PathBuf) -> Result<()> {
    use models::qmt_spgr::{pulse::GaussHannPulse, sf};
    let (_cfg, raw) = load_config_raw(&config_path)?;
    let q: qmrust_core::models::qmt_spgr::config::QmtSpgrConfig = match raw.get("qmt_spgr") {
        Some(sub) => serde_yaml::from_value(sub.clone())?,
        None => Default::default(),
    };
    let angles: Vec<f64> = q.protocol.mtdata.iter().map(|r| r[0]).collect();
    let offsets: Vec<f64> = q.protocol.mtdata.iter().map(|r| r[1]).collect();
    let pulse = GaussHannPulse::new(q.protocol.timing.tmt, q.pulse.bandwidth);
    let (sa, so, st) = sf::build_sf_axes(&angles, &offsets);
    eprintln!(
        "Building Sf table: {}x{}x{} (trf={}, bw={})...",
        sa.len(),
        so.len(),
        st.len(),
        q.protocol.timing.tmt,
        q.pulse.bandwidth
    );
    let table = sf::build_sf_table(&pulse, &sa, &so, &st);
    // values in C-order [i=angle, j=offset, k=T2f]
    let mut bytes = Vec::with_capacity(table.values.len() * 8);
    for v in table.values.iter() {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(&output, &bytes)?;
    let axes_path = output.with_extension("axes.txt");
    let fmt = |v: &[f64]| {
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    std::fs::write(
        &axes_path,
        format!(
            "dims={},{},{}\nangles={}\noffsets={}\nT2f={}\n",
            sa.len(),
            so.len(),
            st.len(),
            fmt(&sa),
            fmt(&so),
            fmt(&st)
        ),
    )?;
    eprintln!(
        "Wrote {:?} ({} f64) and {:?}",
        output,
        table.values.len(),
        axes_path
    );
    Ok(())
}

// ─── Input loading ──────────────────────────────────────────────────────────

struct InputData {
    data: Array4<f64>,
    mask: Option<Array3<bool>>,
    nifti_header: Option<NiftiHeader>,
    ti_override: Option<Vec<f64>>,
}

fn load_input(
    data_path: Option<&PathBuf>,
    mat_path: Option<&PathBuf>,
    mask_path: Option<&PathBuf>,
) -> Result<InputData> {
    match (data_path, mat_path) {
        (Some(nii_path), None) => {
            eprintln!("Loading NIfTI data from {:?}...", nii_path);
            let (data, header) = io::nifti::read_4d_nifti(nii_path)?;
            let (nx, ny, nz, nt) = data.dim();
            eprintln!("  Volume: {}x{}x{}, {} timepoints", nx, ny, nz, nt);

            let mask = match mask_path {
                Some(p) => {
                    eprintln!("Loading mask from {:?}...", p);
                    Some(load_mask(p)?)
                }
                None => None,
            };

            Ok(InputData {
                data,
                mask,
                nifti_header: Some(header),
                ti_override: None,
            })
        }
        (None, Some(mat_path)) => {
            eprintln!("Loading .mat file from {:?}...", mat_path);
            let mat = io::mat::read_mat_file(mat_path)?;

            let mask = match (mat.mask, mask_path) {
                (Some(m), _) => Some(m),
                (None, Some(p)) => {
                    eprintln!("Loading mask from {:?}...", p);
                    Some(load_mask(p)?)
                }
                (None, None) => None,
            };

            Ok(InputData {
                data: mat.ir_data,
                mask,
                nifti_header: None,
                ti_override: mat.ti,
            })
        }
        (None, None) => bail!("Must provide either --data (NIfTI) or --mat-data (.mat)"),
        (Some(_), Some(_)) => bail!("Cannot provide both --data and --mat-data"),
    }
}

fn load_mask(path: &Path) -> Result<Array3<bool>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "mat" {
        io::mat::read_mask_mat(path)
    } else {
        io::nifti::read_mask_nifti(path)
    }
}

fn load_map(path: &Path) -> Result<Array3<f64>> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == "mat" {
        io::mat::read_map_mat(path)
    } else {
        io::nifti::read_map_nifti(path)
    }
}

/// Load one resolved BIDS `Collection` into the `(data, protocol, header)`
/// triple the engine needs: the volumes stacked in the collection's order as
/// `[nx,ny,nz,nt]`, the per-volume sidecar `Protocol` (for the given keys),
/// and the first volume's header for output geometry. `Named` collections
/// (e.g. qMT-style MTS sets) are a later increment — reordering them to a
/// model's `required` axis order is not yet implemented, so they bail loudly
/// rather than silently mis-assign volumes.
fn load_collection(
    fs: &StdFs,
    c: &Collection,
    keys: &[&str],
) -> Result<(Array4<f64>, Protocol, Option<NiftiHeader>)> {
    let vols = match &c.data {
        GroupedData::Sequential(vols) => vols,
        GroupedData::Named(_) => {
            bail!("named-collection fit not yet supported (see fitting-integration follow-ups)")
        }
    };
    if vols.is_empty() {
        bail!("collection for '{}' has no volumes", c.suffix);
    }

    let mut header = None;
    let mut dims: Option<(usize, usize, usize)> = None;
    let mut slices: Vec<Array3<f64>> = Vec::with_capacity(vols.len());
    for v in vols {
        let path = fs.root.join(&v.nii);
        let (data, h) = io::nifti::read_map_nifti_with_header(&path)?;
        let d = data.dim();
        match dims {
            None => dims = Some(d),
            Some(expected) => {
                if expected != d {
                    bail!(
                        "volume {:?} has spatial dims {:?}, expected {:?} (from the first volume)",
                        path,
                        d,
                        expected
                    );
                }
            }
        }
        if header.is_none() {
            header = Some(h);
        }
        slices.push(data);
    }

    let (nx, ny, nz) = dims.expect("checked non-empty above");
    let nt = slices.len();
    let mut out = Array4::<f64>::zeros((nx, ny, nz, nt));
    for (t, slice) in slices.iter().enumerate() {
        out.index_axis_mut(ndarray::Axis(3), t).assign(slice);
    }

    let proto = rust_bids::protocol_for(fs, c, keys)?;
    Ok((out, proto, header))
}

// ─── Fitting dispatch ───────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn run_fit(
    data_path: Option<PathBuf>,
    mat_path: Option<PathBuf>,
    config_path: PathBuf,
    mask_path: Option<PathBuf>,
    output_dir: PathBuf,
    threads: Option<usize>,
    mat_dir: Option<PathBuf>,
    r1map: Option<PathBuf>,
    b1map: Option<PathBuf>,
    b0map: Option<PathBuf>,
) -> Result<()> {
    if let Some(n) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    // Resolve mat-dir convenience paths (individual flags win).
    let (data_path, mat_path, r1map, b1map, b0map, mask_path) = if let Some(dir) = mat_dir.as_ref()
    {
        let pick = |explicit: Option<PathBuf>, name: &str| -> Option<PathBuf> {
            explicit.or_else(|| {
                let p = dir.join(name);
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
            })
        };
        (
            data_path,
            mat_path.or_else(|| {
                let p = dir.join("MTdata.mat");
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
            }),
            pick(r1map, "R1map.mat"),
            pick(b1map, "B1map.mat"),
            pick(b0map, "B0map.mat"),
            pick(mask_path, "Mask.mat"),
        )
    } else {
        (data_path, mat_path, r1map, b1map, b0map, mask_path)
    };

    let (mut cfg, raw) = load_config_raw(&config_path)?;
    let input = load_input(data_path.as_ref(), mat_path.as_ref(), mask_path.as_ref())?;

    // .mat may supply IR TI values as a protocol override.
    let proto = qmrust_core::protocol::resolve(qmrust_core::protocol::ProtocolSource::Mat {
        inversion_times: input.ti_override.clone(),
    });
    // Keep cfg.inversion_times in sync for the dump/eprintln summary below.
    if let Some(ti) = input.ti_override.clone() {
        cfg.inversion_times = ti;
        cfg.inversion_times
            .sort_by(|a, b| a.partial_cmp(b).unwrap());
        eprintln!(
            "  Using TI from .mat file ({} values)",
            cfg.inversion_times.len()
        );
    }

    let entry = qmrust_core::registry::by_name(&cfg.model).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown model: '{}'. Available: {}",
            cfg.model,
            qmrust_core::registry::all()
                .iter()
                .map(|e| e.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let model = (entry.build)(&raw, &proto)?;

    let n_volumes = input.data.dim().3;
    eprintln!("Model: {}, {} volumes", cfg.model, n_volumes);

    // Load the aux maps this model declares (by logical name).
    let mut aux_pairs: Vec<(String, Option<Array3<f64>>)> = Vec::new();
    for spec in model.required_inputs() {
        let path = match spec.name {
            "R1map" => r1map.as_ref(),
            "B1map" => b1map.as_ref(),
            "B0map" => b0map.as_ref(),
            _ => None,
        };
        let map = match path {
            Some(p) => Some(load_map(p)?),
            None => None,
        };
        aux_pairs.push((spec.name.to_string(), map));
    }
    let aux = AuxMaps::new(aux_pairs);

    // NIfTI inputs carry a real spatial header → preserve it (write_3d_nifti).
    // .mat inputs have none → emit a make_nii-compatible header
    // (write_map_nifti: 2D when z=1, sform origin at voxel (1,1,1)) so the
    // maps overlay/subtract cleanly against qMRLab's FitResults.
    let from_mat = input.nifti_header.is_none();
    let header = input.nifti_header.unwrap_or_else(|| {
        let (nx, ny, nz, _) = input.data.dim();
        make_minimal_header(nx, ny, nz)
    });

    fit_and_write(
        model.as_ref(),
        &input.data,
        &proto,
        input.mask.as_ref(),
        &aux,
        &header,
        from_mat,
        &output_dir,
    )
}

/// Run the engine over one (data, protocol) volume and write the result maps
/// as NIfTI into `output_dir` — the shared tail of both `run_fit` (NIfTI/.mat
/// input) and `run_fit_bids` (one call per resolved BIDS collection).
#[allow(clippy::too_many_arguments)]
fn fit_and_write(
    model: &dyn Model,
    data: &Array4<f64>,
    proto: &Protocol,
    mask: Option<&Array3<bool>>,
    aux: &AuxMaps,
    header: &NiftiHeader,
    from_mat: bool,
    output_dir: &Path,
) -> Result<()> {
    let n_volumes = data.dim().3;
    // Build the per-volume identities from the model's declared measurement
    // kind and the resolved protocol — no per-model branching. `Named` maps
    // each volume to its role; `Series` tags each volume with its protocol
    // row (from the resolved sidecar/.mat rows when available).
    let volume_ids = build_volume_ids(model.measurement(), proto, n_volumes)?;

    let start = std::time::Instant::now();
    let (nx, ny, nz, _) = data.dim();
    let (pb, mut cb) = crate::progress::voxel_bar(nx * ny * nz);
    let results = qmrust_core::engine::run(model, data, &volume_ids, mask, aux, &mut cb)?;
    pb.finish_and_clear();
    let elapsed = start.elapsed().as_secs_f64();
    let done_msg = format!("Fitting complete in {:.2}s", elapsed);
    eprintln!(
        "{}",
        done_msg.if_supports_color(Stderr, |t| t.green().bold().to_string())
    );

    std::fs::create_dir_all(output_dir)?;
    eprintln!("Writing results to {:?}...", output_dir);
    for (name, map) in &results {
        let path = output_dir.join(format!("{}.nii.gz", name));
        if from_mat {
            io::nifti::write_map_nifti(map, header, &path)?;
        } else {
            io::nifti::write_3d_nifti(map, header, &path)?;
        }
        let fname = format!("{}.nii.gz", name);
        eprintln!(
            "  {}",
            fname.if_supports_color(Stderr, |t| t.dimmed().to_string())
        );
    }

    eprintln!(
        "{}",
        "Done.".if_supports_color(Stderr, |t| t.green().bold().to_string())
    );
    Ok(())
}

/// Every distinct key used across a model's `Series` identity rows, sorted —
/// the axis keys `load_collection` resolves per-volume sidecar values for.
/// `Named` models are not BIDS-collection-loadable yet (see `load_collection`),
/// so they resolve no keys.
fn measurement_keys(kind: MeasurementKind) -> Vec<String> {
    match kind {
        MeasurementKind::Series { rows } => {
            let mut keys: Vec<String> = rows.into_iter().flat_map(|r| r.into_keys()).collect();
            keys.sort();
            keys.dedup();
            keys
        }
        MeasurementKind::Named { .. } => vec![],
    }
}

/// Fit every collection of a BIDS dataset matching the config's model,
/// writing each subject's (and session's, if present) result maps under
/// `output_dir/<subject>[/<session>]/`. v1 targets no-aux models (e.g. IRT1):
/// BIDS-side B1/B0/R1 map resolution is a follow-up, so a model that
/// `required`-declares any aux input is rejected up front rather than fit
/// with a silently-missing map.
pub fn run_fit_bids(
    bids_dir: PathBuf,
    config_path: PathBuf,
    output_dir: PathBuf,
    threads: Option<usize>,
) -> Result<()> {
    if let Some(n) = threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .ok();
    }

    let (cfg, raw) = load_config_raw(&config_path)?;
    let entry = qmrust_core::registry::by_name(&cfg.model).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown model: '{}'. Available: {}",
            cfg.model,
            qmrust_core::registry::all()
                .iter()
                .map(|e| e.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    // Probe the model's shape (required aux, series identity keys) against an
    // empty protocol — these are structural and don't depend on any one
    // collection's resolved sidecar values.
    let probe = (entry.build)(&raw, &Protocol::default())?;
    let required: Vec<&'static str> = probe
        .required_inputs()
        .into_iter()
        .filter(|s| s.required)
        .map(|s| s.name)
        .collect();
    if !required.is_empty() {
        bail!(
            "qmrust fit --bids-dir does not yet resolve BIDS auxiliary maps; model '{}' \
             requires {:?}. This is a tracked follow-up — for now, fit it via --mat-dir/--data \
             with explicit --r1map/--b1map/--b0map.",
            cfg.model,
            required
        );
    }
    let keys = measurement_keys(probe.measurement());
    let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();

    let fs = StdFs {
        root: bids_dir.clone(),
    };
    let bids_cfg = rust_bids::default_config();
    let suffix = entry.bids_suffix;
    let collections = rust_bids::collections_for(&fs, &bids_cfg, suffix)?;

    if collections.is_empty() {
        eprintln!("No {} collections found in {:?}", suffix, bids_dir);
        return Ok(());
    }
    eprintln!(
        "Found {} {} collection(s) in {:?}",
        collections.len(),
        suffix,
        bids_dir
    );

    for c in &collections {
        for w in &c.warnings {
            eprintln!("  warning ({}): {}", c.subject, w.message);
        }

        let label = match &c.session {
            Some(ses) => format!("{}/{}", c.subject, ses),
            None => c.subject.clone(),
        };
        eprintln!("Fitting {}...", label);

        let (data, proto, header) = match load_collection(&fs, c, &key_refs) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  skipping {}: {}", label, e);
                continue;
            }
        };

        let model = (entry.build)(&raw, &proto)?;
        eprintln!("  Model: {}, {} volumes", cfg.model, data.dim().3);

        let from_mat = header.is_none();
        let (nx, ny, nz, _) = data.dim();
        let header = header.unwrap_or_else(|| make_minimal_header(nx, ny, nz));

        let subject_dir = match &c.session {
            Some(ses) => output_dir.join(&c.subject).join(ses),
            None => output_dir.join(&c.subject),
        };

        fit_and_write(
            model.as_ref(),
            &data,
            &proto,
            None,
            &AuxMaps::empty(),
            &header,
            from_mat,
            &subject_dir,
        )?;
    }

    Ok(())
}

/// Header for outputs whose input carried no spatial reference (.mat inputs),
/// matching qMRLab's `make_nii`/`save_nii_v2` convention so Rust maps overlay
/// exactly on qMRLab's FitResults: no qform, an sform identity with the origin
/// at voxel (1,1,1), and float64 datatype.
fn make_minimal_header(nx: usize, ny: usize, nz: usize) -> NiftiHeader {
    let mut h = NiftiHeader::default();
    h.dim[0] = 3;
    h.dim[1] = nx as u16;
    h.dim[2] = ny as u16;
    h.dim[3] = nz as u16;
    h.pixdim[0] = 0.0;
    h.pixdim[1] = 1.0;
    h.pixdim[2] = 1.0;
    h.pixdim[3] = 1.0;
    h.datatype = 64;
    h.bitpix = 64;
    // Match make_nii: use the sform (not qform) with origin at voxel (1,1,1).
    h.qform_code = 0;
    h.sform_code = 1;
    h.srow_x = [1.0, 0.0, 0.0, 1.0];
    h.srow_y = [0.0, 1.0, 0.0, 1.0];
    h.srow_z = [0.0, 0.0, 1.0, 1.0];
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique tempdir under `std::env::temp_dir()`, removed on drop so
    /// repeated test runs don't accumulate stale fixture directories.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "qmrust-cli-test-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Clean IR signal `a + b*exp(-ti/t1)`, matching the fixture the fitting
    /// engine round-trips against elsewhere in the workspace.
    fn ir_signal(ti: f64, t1: f64, a: f64, b: f64) -> f64 {
        a + b * (-ti / t1).exp()
    }

    #[test]
    fn load_collection_stacks_sequential_irt1_series_in_ti_order() {
        let tmp = TempDir::new("load-collection");
        let anat_dir = tmp.0.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        let t1 = 900.0_f64;
        let a = 500.0_f64;
        let b = -1000.0_f64;
        let tis = [350.0_f64, 500.0, 650.0, 800.0];
        let header = make_minimal_header(1, 1, 1);
        for (i, ti) in tis.iter().enumerate() {
            let signal = ir_signal(*ti, t1, a, b);
            let data = Array3::from_elem((1, 1, 1), signal);
            let nii_path = anat_dir.join(format!("sub-01_inv-{:02}_IRT1.nii.gz", i + 1));
            io::nifti::write_3d_nifti(&data, &header, &nii_path).unwrap();
            let json_path = anat_dir.join(format!("sub-01_inv-{:02}_IRT1.json", i + 1));
            std::fs::write(&json_path, format!(r#"{{"InversionTime": {ti}}}"#)).unwrap();
        }

        let fs = StdFs {
            root: tmp.0.clone(),
        };
        let cfg = rust_bids::default_config();
        let cols = rust_bids::collections_for(&fs, &cfg, "IRT1").unwrap();
        assert_eq!(cols.len(), 1, "one IRT1 collection for sub-01");

        let (data, proto, header) = load_collection(&fs, &cols[0], &["InversionTime"]).unwrap();

        assert_eq!(data.dim(), (1, 1, 1, 4));
        assert!(header.is_some());
        for (i, ti) in tis.iter().enumerate() {
            let expected = ir_signal(*ti, t1, a, b);
            assert!(
                (data[[0, 0, 0, i]] - expected).abs() < 1e-6,
                "volume {i} (TI={ti}) should carry the clean IR signal in TI order"
            );
        }

        assert_eq!(proto.volumes.len(), 4);
        for (i, ti) in tis.iter().enumerate() {
            assert_eq!(
                proto.volumes[i].get("InversionTime"),
                Some(ti),
                "protocol volume {i} should carry the matching InversionTime"
            );
        }
    }

    /// `qmrust fit --bids-dir` end to end: a synthetic sub-01 IRT1 series on
    /// disk (same layout `load_collection`'s test builds) fitted through
    /// `run_fit_bids`, writing `T1.nii.gz` whose single voxel recovers the
    /// known T1.
    #[test]
    fn run_fit_bids_recovers_t1_from_a_synthetic_dataset() {
        let tmp = TempDir::new("fit-bids");
        let bids_dir = tmp.0.join("dataset");
        let anat_dir = bids_dir.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        let t1 = 900.0_f64;
        let a = 500.0_f64;
        let b = -1000.0_f64;
        let tis = [350.0_f64, 500.0, 650.0, 800.0, 950.0, 1100.0, 1250.0];
        let header = make_minimal_header(1, 1, 1);
        for (i, ti) in tis.iter().enumerate() {
            let signal = ir_signal(*ti, t1, a, b);
            let data = Array3::from_elem((1, 1, 1), signal);
            let nii_path = anat_dir.join(format!("sub-01_inv-{:02}_IRT1.nii.gz", i + 1));
            io::nifti::write_3d_nifti(&data, &header, &nii_path).unwrap();
            let json_path = anat_dir.join(format!("sub-01_inv-{:02}_IRT1.json", i + 1));
            std::fs::write(&json_path, format!(r#"{{"InversionTime": {ti}}}"#)).unwrap();
        }

        let config_path = tmp.0.join("ir.yaml");
        std::fs::write(
            &config_path,
            "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800, 950, 1100, 1250]\n",
        )
        .unwrap();

        let out_dir = tmp.0.join("out");
        run_fit_bids(bids_dir, config_path, out_dir.clone(), None).unwrap();

        let t1_path = out_dir.join("sub-01").join("T1.nii.gz");
        assert!(t1_path.exists(), "expected {:?} to exist", t1_path);
        let t1_map = io::nifti::read_map_nifti(&t1_path).unwrap();
        assert!(
            (t1_map[[0, 0, 0]] - t1).abs() < 1.0,
            "T1: {} (expected ~{})",
            t1_map[[0, 0, 0]],
            t1
        );
    }

    #[test]
    fn load_collection_rejects_named_collections_for_now() {
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            suffix: "MTS".into(),
            data: GroupedData::Named(Default::default()),
            warnings: vec![],
        };
        let fs = StdFs {
            root: std::env::temp_dir(),
        };
        match load_collection(&fs, &c, &["FlipAngle"]) {
            Ok(_) => panic!("named collections must be rejected, not silently loaded"),
            Err(e) => assert!(e
                .to_string()
                .contains("named-collection fit not yet supported")),
        }
    }
}
