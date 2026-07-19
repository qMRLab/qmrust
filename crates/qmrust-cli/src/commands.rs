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
use qmrust_core::core::model::{MeasurementKind, Protocol, VolumeId};
use qmrust_core::models;
use std::collections::BTreeMap;

/// Build per-volume identities for `engine::run` from a model's declared
/// measurement kind and the resolved protocol — dispatch on the measurement
/// shape, never on the model name.
///
/// - `Named { roles }`: volume `i` takes role `roles[i]` (requires exactly
///   `roles.len()` volumes).
/// - `Series`: each volume carries its protocol row from `proto.volumes` when
///   the resolver supplied one per volume; otherwise an empty row, and the
///   model reads values in acquisition order until the BIDS shell supplies
///   full per-volume sidecar identities.
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
        MeasurementKind::Series { .. } => {
            if proto.volumes.len() == n_volumes {
                Ok(proto
                    .volumes
                    .iter()
                    .cloned()
                    .map(VolumeId::Params)
                    .collect())
            } else {
                Ok((0..n_volumes)
                    .map(|_| VolumeId::Params(BTreeMap::new()))
                    .collect())
            }
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
    // Build the per-volume identities from the model's declared measurement
    // kind and the resolved protocol — no per-model branching. `Named` maps
    // each volume to its role; `Series` tags each volume with its protocol
    // row (from the resolved sidecar/.mat rows when available).
    let volume_ids = build_volume_ids(model.measurement(), &proto, n_volumes)?;

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
    let aux = qmrust_core::engine::AuxMaps::new(aux_pairs);

    let start = std::time::Instant::now();
    let (nx, ny, nz, _) = input.data.dim();
    let (pb, mut cb) = crate::progress::voxel_bar(nx * ny * nz);
    let results = qmrust_core::engine::run(
        model.as_ref(),
        &input.data,
        &volume_ids,
        input.mask.as_ref(),
        &aux,
        &mut cb,
    )?;
    pb.finish_and_clear();
    let elapsed = start.elapsed().as_secs_f64();
    let done_msg = format!("Fitting complete in {:.2}s", elapsed);
    eprintln!(
        "{}",
        done_msg.if_supports_color(Stderr, |t| t.green().bold().to_string())
    );

    // Write outputs. NIfTI inputs carry a real spatial header → preserve it
    // (write_3d_nifti). .mat inputs have none → emit a make_nii-compatible
    // header (write_map_nifti: 2D when z=1, sform origin at voxel (1,1,1)) so
    // the maps overlay/subtract cleanly against qMRLab's FitResults.
    std::fs::create_dir_all(&output_dir)?;
    let from_mat = input.nifti_header.is_none();
    let header = input.nifti_header.unwrap_or_else(|| {
        let (nx, ny, nz, _) = input.data.dim();
        make_minimal_header(nx, ny, nz)
    });

    eprintln!("Writing results to {:?}...", output_dir);
    for (name, map) in &results {
        let path = output_dir.join(format!("{}.nii.gz", name));
        if from_mat {
            io::nifti::write_map_nifti(map, &header, &path)?;
        } else {
            io::nifti::write_3d_nifti(map, &header, &path)?;
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
