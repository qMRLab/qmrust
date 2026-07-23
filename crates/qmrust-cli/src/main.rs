//! Thin CLI entry point. Parses arguments and dispatches into
//! [`qmrust::commands`] / [`qmrust::sim`]; all behaviour lives in the library.

use anyhow::Result;
use clap::{Parser, Subcommand};
use qmrust_core::sim;
use std::path::PathBuf;

mod bidsify;
mod commands;
mod io;
mod progress;
mod provenance;

#[derive(Parser)]
#[command(name = "qmrust", version, about = "Quantitative MRI fitting in Rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fit quantitative MRI data
    Fit {
        /// Path to 4D NIfTI data (.nii or .nii.gz)
        #[arg(long, group = "input")]
        data: Option<PathBuf>,

        /// Path to MATLAB .mat file containing data, mask, and protocol
        #[arg(long, group = "input")]
        mat_data: Option<PathBuf>,

        /// Path to a qMRI-BIDS dataset root; fits every collection matching
        /// the config's model, one subject/session at a time
        #[arg(long, group = "input")]
        bids_dir: Option<PathBuf>,

        /// Path to YAML configuration file
        #[arg(long)]
        config: PathBuf,

        /// Path to 3D NIfTI or .mat binary mask (optional)
        #[arg(long)]
        mask: Option<PathBuf>,

        /// Output directory for result maps
        #[arg(long, default_value = "./FitResults")]
        output_dir: PathBuf,

        /// Number of threads (default: all cores)
        #[arg(long)]
        threads: Option<usize>,

        /// Directory containing MTdata.mat/R1map.mat/B1map.mat/B0map.mat/Mask.mat
        #[arg(long)]
        mat_dir: Option<PathBuf>,

        /// R1 map (NIfTI or .mat)
        #[arg(long)]
        r1map: Option<PathBuf>,

        /// B1 map (NIfTI or .mat)
        #[arg(long)]
        b1map: Option<PathBuf>,

        /// B0 map (NIfTI or .mat)
        #[arg(long)]
        b0map: Option<PathBuf>,

        /// Custom BIDS grouping manifest (YAML); overrides the built-in default.
        /// Only valid with --bids-dir.
        #[arg(long)]
        grouping: Option<PathBuf>,
    },

    /// Build the qmt_spgr Sf saturation table for a config's protocol and write
    /// it as raw little-endian f64 (C-order [angle, offset, T2f]) for validation.
    DumpSf {
        /// Path to YAML configuration file (qmt_spgr)
        #[arg(long)]
        config: PathBuf,

        /// Output path for the raw f64 table values
        #[arg(long)]
        output: PathBuf,
    },

    /// Print the fully-resolved effective config (all defaults materialized,
    /// validation applied) as YAML, for reproducibility/auditing.
    DumpConfig {
        /// Path to YAML configuration file
        #[arg(long)]
        config: PathBuf,
    },

    /// Simulate signal / sim→fit round-trips (qMRLab-style Sim_*).
    Sim {
        #[command(subcommand)]
        mode: SimMode,
    },

    /// Convert a qMRLab .mat dataset into a byte-identical BIDS layout
    /// ("inversion_recovery" or "qmt_spgr").
    Bidsify {
        /// Model name ("inversion_recovery" or "qmt_spgr")
        #[arg(long)]
        model: String,

        /// Path to the .mat file containing the IR/MT data (+ optional Mask/TI)
        #[arg(long)]
        mat_data: Option<PathBuf>,

        /// Directory containing MTdata.mat + optional R1map.mat/B1map.mat/
        /// B0map.mat/Mask.mat (qmt_spgr convenience, mirrors `fit --mat-dir`)
        #[arg(long)]
        mat_dir: Option<PathBuf>,

        /// Path to a 4D NIfTI measurement (echoes/TIs in the 4th axis), for
        /// datasets that ship as NIfTI rather than qMLab .mat. Mutually
        /// exclusive with --mat-data/--mat-dir.
        #[arg(long)]
        nii_data: Option<PathBuf>,

        /// Directory of per-role NIfTIs (`<role>.nii.gz`, e.g. MTS's
        /// MTw/PDw/T1w) for a Named model. Mutually exclusive with the other
        /// source flags.
        #[arg(long)]
        nii_dir: Option<PathBuf>,

        /// Path to a NIfTI mask, paired with --nii-data/--nii-dir
        #[arg(long)]
        nii_mask: Option<PathBuf>,

        /// Path to a separate .mat mask file (overrides one embedded in mat_data
        /// or found in --mat-dir)
        #[arg(long)]
        mask: Option<PathBuf>,

        /// Path to the model's YAML config (for inversion_times/qmt_spgr
        /// protocol fallback)
        #[arg(long)]
        config: PathBuf,

        /// Subject label without the "sub-" prefix (e.g. "01")
        #[arg(long)]
        subject: String,

        /// BIDS dataset root to create/append to
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum SimMode {
    /// Forward signal only (noise-free)
    Signal {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        plot: Option<PathBuf>,
    },
    /// Simulate one voxel (optionally many noisy trials) and fit back
    SingleVoxel {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        plot: Option<PathBuf>,
    },
    /// Sweep one parameter; report bias/std per point
    Sensitivity {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        plot: Option<PathBuf>,
    },
    /// Draw parameters from distributions; report error statistics
    Montecarlo {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        plot: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fit {
            data,
            mat_data,
            bids_dir,
            config,
            mask,
            output_dir,
            threads,
            mat_dir,
            r1map,
            b1map,
            b0map,
            grouping,
        } => {
            if let Some(bids_dir) = bids_dir {
                commands::run_fit_bids(bids_dir, config, output_dir, threads, grouping)
            } else {
                if grouping.is_some() {
                    anyhow::bail!("--grouping is only valid with --bids-dir");
                }
                commands::run_fit(
                    data, mat_data, config, mask, output_dir, threads, mat_dir, r1map, b1map, b0map,
                )
            }
        }
        Commands::DumpSf { config, output } => commands::run_dump_sf(config, output),
        Commands::DumpConfig { config } => commands::run_dump_config(config),
        Commands::Sim { mode } => {
            let (name, config, output, plot) = match mode {
                SimMode::Signal {
                    config,
                    output,
                    plot,
                } => ("signal", config, output, plot),
                SimMode::SingleVoxel {
                    config,
                    output,
                    plot,
                } => ("single-voxel", config, output, plot),
                SimMode::Sensitivity {
                    config,
                    output,
                    plot,
                } => ("sensitivity", config, output, plot),
                SimMode::Montecarlo {
                    config,
                    output,
                    plot,
                } => ("montecarlo", config, output, plot),
            };
            sim::run_sim(name, config, output, plot)
        }
        Commands::Bidsify {
            model,
            mat_data,
            mat_dir,
            nii_data,
            nii_dir,
            nii_mask,
            mask,
            config,
            subject,
            out,
        } => bidsify::run_bidsify(bidsify::BidsifyArgs {
            model,
            mat_data,
            mat_dir,
            nii_data,
            nii_dir,
            nii_mask,
            mask,
            config,
            subject,
            out,
        }),
    }
}
