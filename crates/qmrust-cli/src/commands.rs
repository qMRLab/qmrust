//! CLI command handlers.
//!
//! These are the imperative-shell entry points behind each `qmrust`
//! subcommand. `main.rs` parses arguments and calls straight into here, keeping
//! the binary thin and this logic testable as part of the library.

use anyhow::{bail, Context, Result};
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

/// qMRLab `.mat` inversion times are in milliseconds; `qmrust-core` is
/// BIDS-native seconds (see CLAUDE.md "Units — BIDS-native"). This is the
/// ms -> s conversion boundary for `.mat`-supplied TI: everything downstream
/// (`ti_override`, the resolved `Protocol`, and the fitted model) must see
/// seconds, never ms.
pub(crate) fn mat_ti_to_seconds(ti: Option<Vec<f64>>) -> Option<Vec<f64>> {
    ti.map(|ti| ti.iter().map(|t| t / 1000.0).collect())
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
                ti_override: mat_ti_to_seconds(mat.ti),
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
/// `[nx,ny,nz,nt]`, the per-volume sidecar `Protocol` (resolved against
/// `schema`), and the first volume's header for output geometry. `Named`
/// collections (e.g. MTsat-style MTS sets) bail loudly: reordering their
/// role-labeled volumes to a model's `required` axis order is not implemented,
/// and silently mis-assigning volumes would be worse than refusing.
///
/// An empty `schema` (a model that hasn't declared a `protocol_schema()`)
/// resolves to an empty `Protocol` — the model falls back to reading its own
/// `--config` in that case, matching the pre-schema behaviour.
fn load_collection(
    fs: &StdFs,
    c: &Collection,
    schema: &[qmrust_core::core::model::ProtoParam],
    options: &std::collections::BTreeMap<String, f64>,
) -> Result<(Array4<f64>, Protocol, Option<NiftiHeader>)> {
    let vols = match &c.data {
        GroupedData::Sequential(vols) => vols,
        GroupedData::Named(_) => {
            bail!("named-collection fit not yet supported")
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

    // An empty schema must yield an empty `Protocol` (zero volumes), NOT one
    // with N empty per-volume maps: `build_volume_ids` treats a volume count
    // matching the data as authoritative identities, so N empty rows would
    // suppress the model's canonical `rows` fallback and break identity
    // matching. This branch is load-bearing for correctness, not an optimization.
    let proto = if schema.is_empty() {
        Protocol::default()
    } else {
        rust_bids::resolve_protocol(fs, c, schema, options)?
    };
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

/// Run the engine over one (data, protocol) volume, honoring the model's
/// `FitStrategy`/measurement kind. Shared by `fit_and_write` (flat `run_fit`
/// output) and `run_fit_bids` (BIDS-derivatives output) so both write from
/// the identical `FitResults`.
fn run_model_fit(
    model: &dyn Model,
    data: &Array4<f64>,
    proto: &Protocol,
    mask: Option<&Array3<bool>>,
    aux: &AuxMaps,
) -> Result<qmrust_core::fitting::FitResults> {
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
    Ok(results)
}

/// Run the engine over one (data, protocol) volume and write the result maps
/// as NIfTI into `output_dir` — the flat layout `run_fit` (NIfTI/.mat input)
/// uses (`<output_dir>/<name>.nii.gz`, e.g. `out_ir/T1.nii.gz`).
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
    let results = run_model_fit(model, data, proto, mask, aux)?;

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

/// Compact JSON view of a resolved `Protocol`, faithful to what the fit
/// actually used: per-volume params grouped into arrays keyed by param name
/// (e.g. `"InversionTime": [0.35, 0.5, ...]`, one entry per volume, in
/// acquisition order) plus any collection-wide scalars from `proto.global`.
fn protocol_to_json(proto: &Protocol) -> serde_json::Value {
    let mut by_param: std::collections::BTreeMap<&str, Vec<f64>> =
        std::collections::BTreeMap::new();
    for volume in &proto.volumes {
        for (key, value) in volume {
            by_param.entry(key.as_str()).or_default().push(*value);
        }
    }
    let mut out = serde_json::Map::new();
    for (key, values) in by_param {
        out.insert(key.to_string(), serde_json::json!(values));
    }
    for (key, value) in &proto.global {
        out.insert(key.clone(), serde_json::json!(value));
    }
    serde_json::Value::Object(out)
}

/// The input volumes a collection's fit read from, as dataset-relative BIDS
/// URIs (`bids::<path relative to the dataset root>`). `VolumeRef.nii` is
/// already dataset-relative under `StdFs`; the `bids_dir` strip keeps the URI
/// relative if the path is ever absolute.
fn collection_sources(c: &Collection, bids_dir: &Path) -> Vec<String> {
    let volumes: Vec<&str> = match &c.data {
        GroupedData::Sequential(v) => v.iter().map(|r| r.nii.as_str()).collect(),
        GroupedData::Named(m) => m.values().map(|r| r.nii.as_str()).collect(),
    };
    volumes
        .into_iter()
        .map(|nii| {
            let rel = Path::new(nii)
                .strip_prefix(bids_dir)
                .unwrap_or(Path::new(nii));
            format!("bids::{}", rel.display())
        })
        .collect()
}

/// Write `model`'s declared BIDS output maps (`Model::bids_outputs()`) from a
/// fitted `results` into a BIDS-derivatives tree rooted at `deriv_root`:
/// `deriv_root/qmrust/<subject>[/<session>]/anat/<subject>[_<session>]_<suffix>.nii.gz`
/// plus a full provenance JSON sidecar (`prov.sidecar(units)`) next to each.
/// Outputs `bids_outputs()` doesn't declare (diagnostics like
/// `res`/`idx`/`kf`/`resnorm`) are never written —
/// only real BIDS maps get exported to the derivatives layout. Uses the same
/// writer `fit_and_write`'s flat output uses (`write_map_nifti` for
/// `.mat`-sourced data, `write_3d_nifti` otherwise), so map values are
/// byte-identical between the flat and derivatives layouts. Also ensures a
/// `deriv_root/qmrust/dataset_description.json` exists (created once, never
/// overwritten on subsequent subjects/sessions).
// One parameter per independent piece of write context (map data, model,
// BIDS entities, output root, header/source-format, provenance) — a params
// struct would just relocate the same fields without reducing them.
#[allow(clippy::too_many_arguments)]
fn write_derivatives(
    results: &qmrust_core::fitting::FitResults,
    model: &dyn Model,
    subject: &str,
    session: Option<&str>,
    deriv_root: &Path,
    header: &NiftiHeader,
    from_mat: bool,
    prov: &crate::provenance::FitProvenance,
) -> Result<()> {
    let qmrust_root = deriv_root.join("qmrust");
    std::fs::create_dir_all(&qmrust_root)?;
    let dd_path = qmrust_root.join("dataset_description.json");
    if !dd_path.exists() {
        let dd = serde_json::json!({
            "Name": "qmrust derivatives",
            "BIDSVersion": "1.8.0",
            "GeneratedBy": [crate::provenance::generated_by()],
            "DatasetType": "derivative",
        });
        std::fs::write(&dd_path, serde_json::to_string_pretty(&dd)?)?;
    }

    let subject_dir = match session {
        Some(ses) => qmrust_root.join(subject).join(ses),
        None => qmrust_root.join(subject),
    };
    let anat_dir = subject_dir.join("anat");
    std::fs::create_dir_all(&anat_dir)?;

    let entity_stem = match session {
        Some(ses) => format!("{subject}_{ses}"),
        None => subject.to_string(),
    };

    for (output_name, suffix, units) in model.bids_outputs() {
        let Some(map) = results.get(output_name) else {
            continue;
        };
        let base = format!("{entity_stem}_{suffix}");
        let nii_path = anat_dir.join(format!("{base}.nii.gz"));
        if from_mat {
            io::nifti::write_map_nifti(map, header, &nii_path)?;
        } else {
            io::nifti::write_3d_nifti(map, header, &nii_path)?;
        }
        let json_path = anat_dir.join(format!("{base}.json"));
        std::fs::write(
            &json_path,
            serde_json::to_string_pretty(&prov.sidecar(units))?,
        )?;
    }
    Ok(())
}

/// Fit every collection of a BIDS dataset matching the config's model,
/// writing each subject's (and session's, if present) result maps under
/// `output_dir/qmrust/<subject>[/<session>]/anat/` in the BIDS-derivatives
/// layout (see `write_derivatives`). Each model's declared auxiliary inputs
/// (B1/B0/R1) and a brain mask are resolved from the dataset's file table by
/// BIDS suffix — found wherever they live, raw tree or any `derivatives/`
/// pipeline — and fed to the fit; a `required` input that is absent is a hard
/// error, an optional one simply falls back to the model's default.
pub fn run_fit_bids(
    bids_dir: PathBuf,
    config_path: PathBuf,
    output_dir: PathBuf,
    threads: Option<usize>,
    grouping: Option<PathBuf>,
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

    // Probe the model's shape (declared aux, series identity keys) against an
    // empty protocol — these are structural and don't depend on any one
    // collection's resolved sidecar values.
    let probe = (entry.build)(&raw, &Protocol::default())?;
    // Declarative BIDS metadata -> protocol mapping. Empty for a model that
    // declares no `protocol_schema()`; `load_collection` then resolves into an
    // empty `Protocol` and the model reads its protocol from `--config`.
    let schema = probe.protocol_schema();
    // `Source::Option` protocol params fall back to `--config`; the BIDS path
    // resolves protocol from sidecars, so no options are supplied.
    let options: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();

    let fs = StdFs {
        root: bids_dir.clone(),
    };
    let bids_cfg = match &grouping {
        Some(path) => {
            let yaml = std::fs::read_to_string(path)
                .with_context(|| format!("reading grouping manifest {:?}", path))?;
            rust_bids::parse_config(&yaml)
                .with_context(|| format!("parsing grouping manifest {:?}", path))?
        }
        None => rust_bids::default_config(),
    };
    let vocab = rust_bids::Vocabulary::from_config(&bids_cfg);

    // Optional `mask:` config: which mask to apply, disambiguated by entity
    // constraints. Its entity keys are normalized (e.g. `desc` -> `description`)
    // to match how the table stores them.
    let mask_spec: Option<MaskSpec> = raw
        .get("mask")
        .map(|v| serde_yaml::from_value::<MaskSpec>(v.clone()))
        .transpose()
        .map_err(|e| anyhow::anyhow!("invalid `mask:` config: {e}"))?
        .map(|spec| MaskSpec {
            suffix: spec.suffix,
            entities: spec
                .entities
                .into_iter()
                .map(|(k, v)| (vocab.normalize_entity_key(&k), v))
                .collect(),
        });
    let suffix = entry.bids_suffix;
    // The flat file table spans the whole dataset — raw tree and every
    // `derivatives/` pipeline — and is the single source from which each
    // collection's auxiliary inputs (B1/B0/R1, mask) are located by suffix.
    let table = rust_bids::parse_to_table(&fs, &vocab)?;
    for unknown in table
        .iter()
        .map(|r| r.suffix.as_str())
        .filter(|s| !vocab.is_known_suffix(s))
        .collect::<std::collections::BTreeSet<_>>()
    {
        eprintln!("warning (vocabulary): unrecognized suffix '{unknown}' — not a canonical BIDS suffix, registered model, or declared custom suffix");
    }
    let collections = rust_bids::collections_for(&fs, &bids_cfg, suffix)?;

    if collections.is_empty() {
        bail!("no {} collections found in {:?}", suffix, bids_dir);
    }
    eprintln!(
        "Found {} {} collection(s) in {:?}",
        collections.len(),
        suffix,
        bids_dir
    );

    let mut fit_count = 0usize;
    let mut skipped = 0usize;
    for c in &collections {
        for w in &c.warnings {
            eprintln!("  warning ({}): {}", c.subject, w.message);
        }

        let label = match &c.session {
            Some(ses) => format!("{}/{}", c.subject, ses),
            None => c.subject.clone(),
        };

        // Only the expected, structural case is a skip: `Named` collections
        // (e.g. MTsat-style MTS sets) aren't reorderable to a model's axis order
        // yet (see `load_collection`). Anything else `load_collection` (or the
        // model build / run_model_fit / write_derivatives below) reports — a
        // corrupt NIfTI, a spatial-dims mismatch, a broken sidecar — is a real
        // failure and must propagate loudly (`?`), never be logged-and-skipped
        // alongside it.
        if matches!(c.data, GroupedData::Named(_)) {
            eprintln!(
                "  skipping {}: named-collection fit not yet supported",
                label
            );
            skipped += 1;
            continue;
        }

        eprintln!("Fitting {}...", label);
        let (data, proto, header) = load_collection(&fs, c, &schema, &options)?;

        let model = (entry.build)(&raw, &proto)?;
        eprintln!("  Model: {}, {} volumes", cfg.model, data.dim().3);

        let from_mat = header.is_none();
        let (nx, ny, nz, _) = data.dim();
        let header = header.unwrap_or_else(|| make_minimal_header(nx, ny, nz));

        let (aux, mask, aux_sources) = resolve_aux_and_mask(
            &table,
            model.as_ref(),
            &c.entities,
            mask_spec.as_ref(),
            &bids_dir,
        )?;
        for src in &aux_sources {
            eprintln!("  input: {src}");
        }

        let executed_at_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let fit_started = std::time::Instant::now();
        let results = run_model_fit(model.as_ref(), &data, &proto, mask.as_ref(), &aux)?;
        let duration_s = fit_started.elapsed().as_secs_f64();

        let mut sources = collection_sources(c, &bids_dir);
        sources.extend(aux_sources);
        let prov = crate::provenance::FitProvenance {
            model: cfg.model.clone(),
            config_json: serde_json::to_value(&raw)?,
            protocol_json: protocol_to_json(&proto),
            sources,
            executed_at_unix,
            duration_s,
        };
        // `output_dir` is treated as the derivatives root: BIDS output lands
        // at `output_dir/qmrust/<subject>[/<session>]/anat/...` (see
        // `write_derivatives`), not the flat layout `run_fit` uses.
        write_derivatives(
            &results,
            model.as_ref(),
            &c.subject,
            c.session.as_deref(),
            &output_dir,
            &header,
            from_mat,
            &prov,
        )?;
        fit_count += 1;
    }

    eprintln!("Fit {} subject(s), skipped {}", fit_count, skipped);
    if fit_count == 0 {
        bail!(
            "no {} collections were fit in {:?} ({} skipped)",
            suffix,
            bids_dir,
            skipped
        );
    }

    Ok(())
}

/// Locate a single input file for a collection by matching its full grouping
/// `identity` (every entity the dataset groups by) plus `extra` constraints
/// (the declared BIDS suffix, and any entity the model says indexes the input)
/// against the whole dataset `table` — raw tree and every `derivatives/`
/// pipeline alike. `None` when nothing matches; an error when several do, so an
/// ambiguous input is surfaced rather than silently chosen. Nothing here is
/// model- or dataset-specific: the model supplies `extra`, the collection
/// supplies `identity`, and `table_filter` does the rest.
/// How to locate the brain mask for a fit, declared in `--config` under a
/// `mask:` key. `suffix` is the BIDS suffix (default `mask`); every other field
/// is an entity constraint (`desc: brain`) that disambiguates which mask to use
/// when a dataset holds several. Absent `mask:` means no masking — a mask is
/// never guessed, because a dataset can carry many (brain, tissue, lesion, …)
/// and auto-picking one is ill-posed.
#[derive(Debug, Clone, serde::Deserialize)]
struct MaskSpec {
    #[serde(default = "default_mask_suffix")]
    suffix: String,
    #[serde(flatten)]
    entities: std::collections::BTreeMap<String, String>,
}

fn default_mask_suffix() -> String {
    "mask".to_string()
}

fn find_input<'a>(
    table: &'a [rust_bids::BidsRow],
    identity: &std::collections::BTreeMap<String, String>,
    extra: &[(&str, &str)],
) -> Result<Option<&'a rust_bids::BidsRow>> {
    let mut constraints: Vec<(&str, &str)> = identity
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    constraints.extend_from_slice(extra);
    let hits = rust_bids::table_filter(table, &constraints);
    match hits.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => bail!(
            "ambiguous input for {:?}: {} candidates ({})",
            constraints,
            many.len(),
            many.iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Resolve a model's declared BIDS auxiliary inputs and a brain mask for one
/// collection from the dataset `table`. Each input is located by the
/// collection's full entity `identity` + its declared BIDS suffix (+ any
/// `BidsMap.entity` the model declares), so it is matched on whatever entities
/// identify the collection — subject/session/run/… alike — not a fixed pair.
/// A `required` input that is absent is a hard error; an optional one becomes a
/// `None` entry (the model uses its default). A brain mask is applied only when
/// `mask_spec` is given (from the config's `mask:` key): it is located by the
/// collection identity + the spec's suffix and entity constraints, so a dataset
/// with several masks is disambiguated by the config rather than guessed.
/// Returns the aux bundle, the optional mask, and the dataset-relative paths of
/// every input loaded (for provenance `Sources`).
fn resolve_aux_and_mask(
    table: &[rust_bids::BidsRow],
    model: &dyn Model,
    identity: &std::collections::BTreeMap<String, String>,
    mask_spec: Option<&MaskSpec>,
    bids_dir: &Path,
) -> Result<(AuxMaps, Option<Array3<bool>>, Vec<String>)> {
    let mut maps: Vec<(String, Option<Array3<f64>>)> = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    for spec in model.required_inputs() {
        let Some(bids) = spec.bids.as_ref() else {
            // Not BIDS-locatable: leave absent for the model's own default.
            maps.push((spec.name.to_string(), None));
            continue;
        };
        let mut extra = vec![("suffix", bids.suffix)];
        if let Some(entity) = bids.entity {
            // The model declares an entity that indexes this input; match it
            // against the identity value the collection carries for it.
            if let Some(val) = identity.get(entity) {
                extra.push((entity, val.as_str()));
            }
        }
        match find_input(table, identity, &extra)? {
            Some(row) => {
                let map = io::nifti::read_map_nifti(&bids_dir.join(&row.path))?;
                maps.push((spec.name.to_string(), Some(map)));
                sources.push(row.path.clone());
            }
            None if spec.required => bail!(
                "required input '{}' (BIDS suffix '{}') not found for {:?} in {:?}",
                spec.name,
                bids.suffix,
                identity,
                bids_dir
            ),
            None => maps.push((spec.name.to_string(), None)),
        }
    }
    let mask = match mask_spec {
        Some(spec) => {
            let mut extra: Vec<(&str, &str)> = vec![("suffix", spec.suffix.as_str())];
            extra.extend(spec.entities.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            match find_input(table, identity, &extra)? {
                Some(row) => {
                    sources.push(row.path.clone());
                    Some(io::nifti::read_mask_nifti(&bids_dir.join(&row.path))?)
                }
                None => None,
            }
        }
        None => None,
    };
    Ok((AuxMaps::new(maps), mask, sources))
}

/// Header for outputs whose input carried no spatial reference (.mat inputs),
/// matching qMRLab's `make_nii`/`save_nii_v2` convention so Rust maps overlay
/// exactly on qMRLab's FitResults: no qform, an sform identity with the origin
/// at voxel (1,1,1), and float64 datatype.
pub(crate) fn make_minimal_header(nx: usize, ny: usize, nz: usize) -> NiftiHeader {
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
    use qmrust_core::core::model::{ProtoParam, Scope, Source};

    /// The IR model's declared schema, for tests exercising `load_collection`
    /// directly (outside `run_fit_bids`, which pulls this from the model).
    fn ir_schema() -> Vec<ProtoParam> {
        vec![ProtoParam {
            name: "InversionTime",
            source: Source::Field("InversionTime"),
            scope: Scope::PerVolume,
        }]
    }

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

    /// A `.mat`-shaped ms TI vector (qMRLab convention) must reach the
    /// fitting core as seconds — the ms -> s boundary conversion invariant
    /// (CLAUDE.md "Units — BIDS-native").
    #[test]
    fn mat_ti_to_seconds_converts_ms_to_seconds() {
        let ti_ms = vec![350.0_f64, 500.0, 900.0, 1250.0];
        let ti_s = mat_ti_to_seconds(Some(ti_ms)).unwrap();
        assert_eq!(ti_s, vec![0.35, 0.5, 0.9, 1.25]);

        // Feeding the converted TIs through the same `ProtocolSource::Mat`
        // path `load_input`/`run_fit` use must yield the seconds values, not
        // the original ms ones.
        let proto = qmrust_core::protocol::resolve(qmrust_core::protocol::ProtocolSource::Mat {
            inversion_times: Some(ti_s.clone()),
        });
        let got: Vec<f64> = proto
            .volumes
            .iter()
            .map(|v| *v.get("InversionTime").unwrap())
            .collect();
        assert_eq!(got, ti_s);

        assert!(mat_ti_to_seconds(None).is_none());
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
            // A stray, non-declared field alongside InversionTime: the schema
            // only names `InversionTime`, so `FlipAngle` must not leak into
            // the resolved `Protocol` below.
            std::fs::write(
                &json_path,
                format!(r#"{{"InversionTime": {ti}, "FlipAngle": 9}}"#),
            )
            .unwrap();
        }

        let fs = StdFs {
            root: tmp.0.clone(),
        };
        let cfg = rust_bids::default_config();
        let cols = rust_bids::collections_for(&fs, &cfg, "IRT1").unwrap();
        assert_eq!(cols.len(), 1, "one IRT1 collection for sub-01");

        let (data, proto, header) = load_collection(
            &fs,
            &cols[0],
            &ir_schema(),
            &std::collections::BTreeMap::new(),
        )
        .unwrap();

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
        for vol in &proto.volumes {
            assert_eq!(
                vol.get("FlipAngle"),
                None,
                "FlipAngle isn't in the IR schema and must not be captured"
            );
        }
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
    ///
    /// Builds a synthetic single-voxel IRT1 BIDS dataset (T1 = 0.9 s, 7
    /// inversion times) plus its `--config` and an empty output dir, returning
    /// `(bids_dir, config_path, out_dir, tmp)` — `tmp` must be kept alive for
    /// the duration of the test (it removes the fixture tree on drop).
    fn synthetic_ir_dataset() -> (PathBuf, PathBuf, PathBuf, TempDir) {
        let tmp = TempDir::new("fit-bids");
        let bids_dir = tmp.0.join("dataset");
        let anat_dir = bids_dir.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        // BIDS-native seconds throughout: T1 = 0.9 s, TIs 0.35..1.25 s
        // (see CLAUDE.md "Units — BIDS-native").
        let t1 = 0.9_f64;
        let a = 500.0_f64;
        let b = -1000.0_f64;
        let tis = [0.35_f64, 0.50, 0.65, 0.80, 0.95, 1.10, 1.25];
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
            "model: inversion_recovery\nmethod: complex\ninversion_times: [0.35, 0.50, 0.65, 0.80, 0.95, 1.10, 1.25]\n",
        )
        .unwrap();

        let out_dir = tmp.0.join("out");
        (bids_dir, config_path, out_dir, tmp)
    }

    #[test]
    fn run_fit_bids_recovers_t1_from_a_synthetic_dataset() {
        let t1 = 0.9_f64;
        let (bids_dir, config_path, out_dir, _tmp) = synthetic_ir_dataset();
        run_fit_bids(bids_dir, config_path, out_dir.clone(), None, None).unwrap();

        // BIDS-derivatives layout: output_dir is the derivatives root, so the
        // T1 map lands at qmrust/sub-01/anat/sub-01_T1map.nii.gz (mapped from
        // IR's `T1` output via `bids_outputs()`), not a flat `T1.nii.gz`.
        let t1_path = out_dir
            .join("qmrust")
            .join("sub-01")
            .join("anat")
            .join("sub-01_T1map.nii.gz");
        assert!(t1_path.exists(), "expected {:?} to exist", t1_path);
        let t1_map = io::nifti::read_map_nifti(&t1_path).unwrap();
        assert!(
            (t1_map[[0, 0, 0]] - t1).abs() < 0.001,
            "T1: {} (expected ~{})",
            t1_map[[0, 0, 0]],
            t1
        );

        let sidecar = out_dir
            .join("qmrust")
            .join("sub-01")
            .join("anat")
            .join("sub-01_T1map.json");
        assert!(sidecar.exists(), "expected a JSON sidecar next to the map");

        let dataset_description = out_dir.join("qmrust").join("dataset_description.json");
        assert!(
            dataset_description.exists(),
            "expected qmrust/dataset_description.json to exist"
        );
    }

    /// A `--grouping` manifest overriding the built-in default is honored:
    /// the same synthetic IRT1 dataset still resolves into one collection
    /// and fits to the known T1, proving `run_fit_bids` loads and applies
    /// the custom manifest rather than always falling back to
    /// `rust_bids::default_config()`.
    #[test]
    fn run_fit_bids_accepts_custom_grouping_file() {
        let (bids_dir, config_path, out_dir, tmp) = synthetic_ir_dataset();
        let grouping = tmp.0.join("grouping.yaml");
        std::fs::write(
            &grouping,
            "loop_over: [sub, ses, run, task]\nIRT1:\n  sequential_set:\n    by: [inv]\n",
        )
        .unwrap();

        run_fit_bids(bids_dir, config_path, out_dir.clone(), None, Some(grouping)).unwrap();

        let t1_path = out_dir
            .join("qmrust")
            .join("sub-01")
            .join("anat")
            .join("sub-01_T1map.nii.gz");
        assert!(t1_path.exists(), "expected {:?} to exist", t1_path);
    }

    /// `write_derivatives` in isolation: given a fitted `FitResults` with a
    /// declared map (`T1`) and an undeclared diagnostic (`res`), it writes
    /// only the declared map (as `T1map`, IR's `bids_outputs()` mapping) plus
    /// its JSON sidecar, ensures `dataset_description.json`, and leaves `res`
    /// out of the derivatives tree entirely.
    #[test]
    fn write_derivatives_writes_only_declared_bids_outputs() {
        let tmp = TempDir::new("write-derivatives");
        let deriv_root = tmp.0.join("derivatives");

        let config_path = tmp.0.join("ir.yaml");
        std::fs::write(
            &config_path,
            "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800]\n",
        )
        .unwrap();
        let (_cfg, raw) = load_config_raw(&config_path).unwrap();
        let entry = qmrust_core::registry::by_name("inversion_recovery").unwrap();
        let model = (entry.build)(&raw, &Protocol::default()).unwrap();

        let mut results = qmrust_core::fitting::FitResults::new();
        let t1_map = Array3::from_elem((1, 1, 1), 900.0_f64);
        results.insert("T1".to_string(), t1_map.clone());
        // A diagnostic output not in `bids_outputs()` — must not be written.
        results.insert("res".to_string(), Array3::from_elem((1, 1, 1), 0.001));

        let header = make_minimal_header(1, 1, 1);
        let prov = crate::provenance::FitProvenance {
            model: "inversion_recovery".to_string(),
            config_json: serde_json::to_value(&raw).unwrap(),
            protocol_json: serde_json::json!({"InversionTime": [0.35, 0.5, 0.65, 0.8]}),
            sources: vec!["bids::sub-01/anat/sub-01_inv-1_IRT1.nii.gz".to_string()],
            executed_at_unix: 1_600_000_000,
            duration_s: 0.5,
        };
        write_derivatives(
            &results,
            model.as_ref(),
            "sub-01",
            None,
            &deriv_root,
            &header,
            true,
            &prov,
        )
        .unwrap();

        let anat_dir = deriv_root.join("qmrust").join("sub-01").join("anat");
        let t1_path = anat_dir.join("sub-01_T1map.nii.gz");
        assert!(t1_path.exists(), "expected {:?} to exist", t1_path);
        let sidecar_path = anat_dir.join("sub-01_T1map.json");
        assert!(
            sidecar_path.exists(),
            "expected a JSON sidecar next to the T1 map"
        );
        let sidecar: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&sidecar_path).unwrap()).unwrap();
        assert_eq!(sidecar["GeneratedBy"][0]["Name"], "qmrust");
        assert_eq!(sidecar["Model"], "inversion_recovery");
        assert_eq!(sidecar["Units"], "s");
        assert!(
            !anat_dir.join("sub-01_res.nii.gz").exists(),
            "the undeclared diagnostic 'res' output must not be written"
        );
        assert!(
            deriv_root
                .join("qmrust")
                .join("dataset_description.json")
                .exists(),
            "expected qmrust/dataset_description.json to exist"
        );

        let read_back = io::nifti::read_map_nifti(&t1_path).unwrap();
        assert_eq!(
            read_back[[0, 0, 0]],
            t1_map[[0, 0, 0]],
            "map values must be byte-identical to the input FitResults"
        );
    }

    /// A real per-volume failure (here: a spatial-dims mismatch that
    /// `load_collection` detects) must fail the whole run loudly — not be
    /// logged as a "skip" alongside the deliberate Named-collection skip.
    #[test]
    fn run_fit_bids_propagates_a_real_load_error_instead_of_skipping() {
        let tmp = TempDir::new("fit-bids-corrupt");
        let bids_dir = tmp.0.join("dataset");
        let anat_dir = bids_dir.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        let t1 = 900.0_f64;
        let a = 500.0_f64;
        let b = -1000.0_f64;
        let tis = [350.0_f64, 500.0, 650.0, 800.0, 950.0, 1100.0, 1250.0];
        let header = make_minimal_header(1, 1, 1);
        // A second header with mismatched spatial dims (2x1x1 vs 1x1x1), used
        // for one volume so `load_collection` hits its dims-mismatch bail.
        let bad_header = make_minimal_header(2, 1, 1);
        for (i, ti) in tis.iter().enumerate() {
            let signal = ir_signal(*ti, t1, a, b);
            let nii_path = anat_dir.join(format!("sub-01_inv-{:02}_IRT1.nii.gz", i + 1));
            if i == 2 {
                let data = Array3::from_elem((2, 1, 1), signal);
                io::nifti::write_3d_nifti(&data, &bad_header, &nii_path).unwrap();
            } else {
                let data = Array3::from_elem((1, 1, 1), signal);
                io::nifti::write_3d_nifti(&data, &header, &nii_path).unwrap();
            }
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
        let err = match run_fit_bids(bids_dir, config_path, out_dir, None, None) {
            Ok(()) => panic!("a spatial-dims mismatch must fail the run, not be skipped"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("spatial dims"),
            "expected a dims-mismatch error, got: {err}"
        );
    }

    /// A qmt_spgr-targeted dataset must exit non-zero, not silently report
    /// success having fit zero subjects. `QMTSPGR` has a `rust-bids` sequential
    /// set definition, so this 3-volume dataset resolves into a real collection
    /// — but it doesn't carry the model's expected 10-row series protocol, so
    /// `validate_against_protocol` inside the model's `build` bails (wrapped in
    /// a "protocol inconsistent" context). Check the full error chain (`{:?}`),
    /// not just the outer `Display` message, for the underlying counts.
    #[test]
    fn run_fit_bids_bails_when_every_collection_is_skipped() {
        let tmp = TempDir::new("fit-bids-all-named");
        let bids_dir = tmp.0.join("dataset");
        let anat_dir = bids_dir.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        let header = make_minimal_header(1, 1, 1);
        let data = Array3::from_elem((1, 1, 1), 1.0_f64);
        // Give each volume a valid Angle/Offset sidecar (qmt_spgr's
        // `protocol_schema()` reads these) but only 3 of the model's 10
        // expected mtdata rows, so the mismatch is caught by
        // `validate_against_protocol`'s count check, not a missing-field error.
        for (fname, angle, offset) in [
            ("sub-01_flip-1_mt-off_QMTSPGR.nii.gz", 142.0, 443.0),
            ("sub-01_flip-1_mt-on_QMTSPGR.nii.gz", 426.0, 443.0),
            ("sub-01_flip-2_mt-off_QMTSPGR.nii.gz", 142.0, 1088.0),
        ] {
            let path = anat_dir.join(fname);
            io::nifti::write_3d_nifti(&data, &header, &path).unwrap();
            let json_path = anat_dir.join(fname.replace(".nii.gz", ".json"));
            std::fs::write(
                &json_path,
                format!(r#"{{"Angle": {angle}, "Offset": {offset}}}"#),
            )
            .unwrap();
        }

        let config_path = tmp.0.join("qmt.yaml");
        std::fs::write(
            &config_path,
            "model: qmt_spgr\nqmt_spgr:\n  model: Ramani\n",
        )
        .unwrap();

        let out_dir = tmp.0.join("out");
        let err = match run_fit_bids(bids_dir, config_path, out_dir, None, None) {
            Ok(()) => panic!("a protocol-mismatched QMTSPGR dataset must bail, not exit Ok"),
            Err(e) => e,
        };
        let chain = format!("{err:?}");
        assert!(
            chain.contains("expected 10 volumes") && chain.contains("supplies 3"),
            "expected a volumes-vs-protocol-rows mismatch bail message, got: {chain}"
        );
    }

    /// A dataset with zero matching collections for the config's model (e.g.
    /// an inversion_recovery config pointed at an MTS-only dataset — wrong
    /// config, typo, whatever) must exit non-zero, not silently succeed
    /// having fit nothing.
    #[test]
    fn run_fit_bids_bails_when_no_collections_are_found() {
        let tmp = TempDir::new("fit-bids-no-collections");
        let bids_dir = tmp.0.join("dataset");
        let anat_dir = bids_dir.join("sub-01/anat");
        std::fs::create_dir_all(&anat_dir).unwrap();

        // An MTS-only dataset; no IRT1 files anywhere.
        let header = make_minimal_header(1, 1, 1);
        let data = Array3::from_elem((1, 1, 1), 1.0_f64);
        for fname in [
            "sub-01_flip-1_mt-off_MTS.nii.gz",
            "sub-01_flip-1_mt-on_MTS.nii.gz",
            "sub-01_flip-2_mt-off_MTS.nii.gz",
        ] {
            io::nifti::write_3d_nifti(&data, &header, &anat_dir.join(fname)).unwrap();
        }

        let config_path = tmp.0.join("ir.yaml");
        std::fs::write(
            &config_path,
            "model: inversion_recovery\nmethod: complex\ninversion_times: [350, 500, 650, 800]\n",
        )
        .unwrap();

        let out_dir = tmp.0.join("out");
        let err = match run_fit_bids(bids_dir, config_path, out_dir, None, None) {
            Ok(()) => panic!("zero matching collections must bail, not exit Ok"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("no IRT1 collections found"),
            "expected the no-collections-found bail message, got: {err}"
        );
    }

    #[test]
    fn load_collection_rejects_named_collections_for_now() {
        let c = Collection {
            subject: "sub-01".into(),
            session: None,
            run: None,
            task: None,
            entities: std::collections::BTreeMap::new(),
            suffix: "MTS".into(),
            data: GroupedData::Named(Default::default()),
            warnings: vec![],
        };
        let fs = StdFs {
            root: std::env::temp_dir(),
        };
        match load_collection(&fs, &c, &ir_schema(), &std::collections::BTreeMap::new()) {
            Ok(_) => panic!("named collections must be rejected, not silently loaded"),
            Err(e) => assert!(e
                .to_string()
                .contains("named-collection fit not yet supported")),
        }
    }

    /// Input resolution is driven by a collection's *full* entity identity, not
    /// a fixed subject/session pair: the same suffix present once per session
    /// must resolve to the file matching the collection's own session, and an
    /// underspecified identity that matches several files is a hard error (no
    /// silent pick). This is what lets the backbone scale past two models —
    /// any entity a dataset groups by participates in locating inputs.
    #[test]
    fn find_input_matches_full_identity_and_flags_ambiguity() {
        use std::collections::BTreeMap;
        let row = |path: &str, ses: &str| rust_bids::BidsRow {
            path: path.into(),
            derivatives: Some("preprocessed".into()),
            datatype: Some("fmap".into()),
            suffix: "TB1map".into(),
            extension: ".nii.gz".into(),
            entities: BTreeMap::from([
                ("subject".to_string(), "01".to_string()),
                ("session".to_string(), ses.to_string()),
            ]),
            sidecar_path: None,
        };
        let rows = vec![
            row(
                "derivatives/preprocessed/sub-01/ses-1/fmap/sub-01_ses-1_TB1map.nii.gz",
                "1",
            ),
            row(
                "derivatives/preprocessed/sub-01/ses-2/fmap/sub-01_ses-2_TB1map.nii.gz",
                "2",
            ),
        ];

        let identity = |ses: Option<&str>| {
            let mut m = BTreeMap::from([("subject".to_string(), "01".to_string())]);
            if let Some(s) = ses {
                m.insert("session".to_string(), s.to_string());
            }
            m
        };

        // Full identity (subject + session) → the matching session's file.
        let hit = find_input(&rows, &identity(Some("2")), &[("suffix", "TB1map")])
            .unwrap()
            .unwrap();
        assert!(hit.path.contains("ses-2"));
        // Underspecified identity (subject only) matches both → ambiguity error.
        assert!(find_input(&rows, &identity(None), &[("suffix", "TB1map")]).is_err());
        // No match → None (an optional input the fit does without).
        assert!(
            find_input(&rows, &identity(Some("9")), &[("suffix", "TB1map")])
                .unwrap()
                .is_none()
        );
    }

    /// A `mask:` config disambiguates which mask to use by entity constraint,
    /// and its short entity keys must be matched against the table's full names.
    /// A dataset holding several masks resolves to the configured one; a spec
    /// too loose to be unique is a hard error rather than a silent pick.
    #[test]
    fn mask_spec_disambiguates_and_flags_ambiguity() {
        use std::collections::BTreeMap;
        // `desc: brain` parses (suffix defaults to `mask`); `desc` is the short
        // key the config author writes.
        let spec: MaskSpec = serde_yaml::from_str("desc: brain\n").unwrap();
        assert_eq!(spec.suffix, "mask");
        assert_eq!(spec.entities.get("desc").map(String::as_str), Some("brain"));

        // Two masks for one subject; the table stores the entity as its full
        // name `description` (as `parse_to_table` would), so the config key is
        // normalized to match.
        let mask_row = |desc: &str| rust_bids::BidsRow {
            path: format!("derivatives/preprocessed/sub-01/anat/sub-01_desc-{desc}_mask.nii.gz"),
            derivatives: Some("preprocessed".into()),
            datatype: Some("anat".into()),
            suffix: "mask".into(),
            extension: ".nii.gz".into(),
            entities: BTreeMap::from([
                ("subject".to_string(), "01".to_string()),
                ("description".to_string(), desc.to_string()),
            ]),
            sidecar_path: None,
        };
        let rows = vec![mask_row("brain"), mask_row("tumor")];
        let identity = BTreeMap::from([("subject".to_string(), "01".to_string())]);

        // Config `desc` -> full `description`, matching the table.
        let extra = [("suffix", "mask"), ("description", "brain")];
        let hit = find_input(&rows, &identity, &extra).unwrap().unwrap();
        assert!(hit.path.contains("desc-brain"));

        // Suffix alone is too loose — both masks match, so it errors.
        assert!(find_input(&rows, &identity, &[("suffix", "mask")]).is_err());
    }

    /// End-to-end validation: the BIDS fit path (`bidsify` +
    /// `run_fit_bids`) must reproduce the `.mat` fit path (`run_fit
    /// --mat-data`) exactly, since `bidsify` writes byte-identical voxel data
    /// (see `bidsify.rs`'s round-trip tests). Needs a *real* qMRLab OSF IR
    /// dataset (`IRData.mat`/`Mask.mat`) supplied via env vars — no network
    /// access here, so it's `#[ignore]`d by default and skipped by a plain
    /// `cargo test`. Run explicitly with:
    ///
    /// ```text
    /// QMRUST_IR_MAT=<path>/IRData.mat QMRUST_IR_MASK=<path>/Mask.mat \
    ///   cargo test -p qmrust-cli --release bids_fit_matches_mat_fit -- --ignored --nocapture
    /// ```
    ///
    /// `scripts/make_bids_examples.sh` fetches such a dataset from OSF.
    ///
    /// Both paths apply the same brain mask: the `.mat` path via `--mask`, the
    /// BIDS path by resolving the mask `bidsify` writes to
    /// `derivatives/preprocessed/` (`run_fit_bids` resolves a declared mask by
    /// its BIDS suffix). The two fits therefore mask identically and must be
    /// exactly equal on every masked voxel of the same byte-identical input;
    /// the loop below compares inside the mask (both leave it unfit outside).
    ///
    /// Both `t1_mat` and `t1_bids` below are our own two pipelines (not
    /// qMRLab's), so they're compared directly in seconds — no unit
    /// reconciliation needed between them. `scripts/make_bids_examples.sh`
    /// separately prints qMRLab's `FitResults/T1.nii.gz` path for manual
    /// comparison; that reference is in **milliseconds**, so any numeric
    /// comparison against it must scale ours by `* 1000.0` first (our T1 is
    /// seconds; see CLAUDE.md "Units — BIDS-native").
    #[test]
    #[ignore]
    fn bids_fit_matches_mat_fit() {
        let ir_mat = PathBuf::from(
            std::env::var("QMRUST_IR_MAT").expect("set QMRUST_IR_MAT=<path>/IRData.mat"),
        );
        let ir_mask = PathBuf::from(
            std::env::var("QMRUST_IR_MASK").expect("set QMRUST_IR_MASK=<path>/Mask.mat"),
        );
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let config = repo_root.join("prots").join("irt1_config.yaml");

        let tmp = TempDir::new("bids-matches-mat");
        let out_mat = tmp.0.join("out_mat");
        let bids_dir = tmp.0.join("ds-qmrust");
        // `output_dir` is the *derivatives root*: `run_fit_bids`/`write_derivatives`
        // append `qmrust/<subject>/anat/...` themselves (mirrors
        // `scripts/make_bids_examples.sh`'s `--output-dir ds-qmrust/derivatives`).
        let deriv_dir = bids_dir.join("derivatives");

        // (a) Fit via the .mat path, in-process.
        run_fit(
            None,
            Some(ir_mat.clone()),
            config.clone(),
            Some(ir_mask.clone()),
            out_mat.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("mat-path fit failed");
        let mat_mask = io::mat::read_mask_mat(&ir_mask).expect("read Mask.mat");
        let t1_mat = io::nifti::read_map_nifti(&out_mat.join("T1.nii.gz")).expect("read T1.mat");

        // (b) bidsify (in-process) + fit via the BIDS path, in-process.
        crate::bidsify::run_bidsify(crate::bidsify::BidsifyArgs {
            model: "inversion_recovery".to_string(),
            mat_data: Some(ir_mat),
            mat_dir: None,
            mask: Some(ir_mask),
            config: config.clone(),
            subject: "01".to_string(),
            out: bids_dir.clone(),
        })
        .expect("bidsify failed");
        run_fit_bids(bids_dir, config, deriv_dir.clone(), None, None)
            .expect("bids-path fit failed");
        let t1_bids = io::nifti::read_map_nifti(
            &deriv_dir
                .join("qmrust")
                .join("sub-01")
                .join("anat")
                .join("sub-01_T1map.nii.gz"),
        )
        .expect("read T1map (bids)");

        assert_eq!(t1_mat.dim(), t1_bids.dim(), "T1 map shapes must match");

        let mut n_in_mask = 0usize;
        let mut n_mismatch = 0usize;
        for ((x, y, z), &in_mask) in mat_mask.indexed_iter() {
            if !in_mask {
                continue;
            }
            n_in_mask += 1;
            let a = t1_mat[[x, y, z]];
            let b = t1_bids[[x, y, z]];
            // Both NaN (fit failed identically on identical input) counts as
            // agreement; otherwise require exact equality (byte-identical
            // input -> byte-identical fit, no tolerance).
            let equal = (a.is_nan() && b.is_nan()) || a == b;
            if !equal {
                n_mismatch += 1;
                eprintln!("  mismatch at ({x},{y},{z}): mat={a} bids={b}");
            }
        }
        eprintln!("in-mask voxels: {n_in_mask}, mismatches: {n_mismatch}");
        assert_eq!(
            n_mismatch, 0,
            "BIDS-path T1 must exactly match .mat-path T1 for every masked voxel \
             (byte-identical input must produce byte-identical fit)"
        );
    }

    /// End-to-end validation: the QMTSPGR BIDS fit path (bidsify
    /// plus run_fit_bids) must reproduce the .mat fit path (run_fit against
    /// mat_data) exactly, mirroring `bids_fit_matches_mat_fit` above for qMT.
    /// `run_fit_bids` doesn't yet resolve BIDS aux maps (see its doc
    /// comment), so this test disables aux on both sides: run_fit is called
    /// with no r1map/b1map/b0map and no mat_dir (so no aux .mat convenience
    /// loading either), and run_fit_bids never resolves BIDS aux at all. Both
    /// sides therefore fit with default aux (B1=1, B0=0, no R1), making this
    /// an apples-to-apples comparison on byte-identical input.
    ///
    /// Needs a *real* qMRLab OSF qMT dataset (`MTdata.mat`) supplied via an
    /// env var — no network access here, so it's `#[ignore]`d by default.
    /// Run explicitly with:
    ///
    /// ```text
    /// QMRUST_QMT_MAT=<path>/MTdata.mat \
    ///   cargo test -p qmrust-cli --release qmtspgr_bids_fit_matches_mat_fit -- --ignored --nocapture
    /// ```
    ///
    /// `scripts/make_bids_examples.sh` fetches such a dataset from OSF.
    #[test]
    #[ignore]
    fn qmtspgr_bids_fit_matches_mat_fit() {
        let qmt_mat = PathBuf::from(
            std::env::var("QMRUST_QMT_MAT").expect("set QMRUST_QMT_MAT=<path>/MTdata.mat"),
        );
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..");
        let config = repo_root.join("prots").join("qmt_config_ramani.yaml");

        let tmp = TempDir::new("qmt-bids-matches-mat");
        let out_mat = tmp.0.join("out_mat");
        let bids_dir = tmp.0.join("ds-qmrust");
        let deriv_dir = bids_dir.join("derivatives");

        // (a) Fit via the .mat path, in-process, with NO aux (no
        // --r1map/--b1map/--b0map, no --mat-dir so no Mask.mat/aux .mat
        // convenience-loading either) — matching the BIDS side's no-aux fit.
        run_fit(
            None,
            Some(qmt_mat.clone()),
            config.clone(),
            None,
            out_mat.clone(),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("mat-path fit failed");
        let f_mat = io::nifti::read_map_nifti(&out_mat.join("F.nii.gz")).expect("read F (mat)");
        let kr_mat = io::nifti::read_map_nifti(&out_mat.join("kr.nii.gz")).expect("read kr (mat)");

        // (b) bidsify (in-process, no mask/aux) + fit via the BIDS path.
        crate::bidsify::run_bidsify(crate::bidsify::BidsifyArgs {
            model: "qmt_spgr".to_string(),
            mat_data: Some(qmt_mat),
            mat_dir: None,
            mask: None,
            config: config.clone(),
            subject: "02".to_string(),
            out: bids_dir.clone(),
        })
        .expect("bidsify failed");
        run_fit_bids(bids_dir, config, deriv_dir.clone(), None, None)
            .expect("bids-path fit failed");

        let anat = deriv_dir.join("qmrust").join("sub-02").join("anat");
        let f_bids =
            io::nifti::read_map_nifti(&anat.join("sub-02_Fmap.nii.gz")).expect("read Fmap (bids)");
        let kr_bids = io::nifti::read_map_nifti(&anat.join("sub-02_kRmap.nii.gz"))
            .expect("read kRmap (bids)");

        assert_eq!(f_mat.dim(), f_bids.dim(), "F map shapes must match");
        assert_eq!(kr_mat.dim(), kr_bids.dim(), "kr map shapes must match");

        let mut n_voxels = 0usize;
        let mut n_mismatch = 0usize;
        for ((x, y, z), &a) in f_mat.indexed_iter() {
            n_voxels += 1;
            let b = f_bids[[x, y, z]];
            let ar = kr_mat[[x, y, z]];
            let br = kr_bids[[x, y, z]];
            let f_equal = (a.is_nan() && b.is_nan()) || a == b;
            let kr_equal = (ar.is_nan() && br.is_nan()) || ar == br;
            if !f_equal || !kr_equal {
                n_mismatch += 1;
                eprintln!("  mismatch at ({x},{y},{z}): F mat={a} bids={b}, kr mat={ar} bids={br}");
            }
        }
        eprintln!("voxels: {n_voxels}, mismatches: {n_mismatch}");
        assert_eq!(
            n_mismatch, 0,
            "BIDS-path F/kr must exactly match .mat-path F/kr for every voxel \
             (byte-identical input, identical no-aux fit, must produce byte-identical output)"
        );
    }
}
