//! indicatif-backed progress callback for the CLI (kept out of qmrust-core so
//! the core stays wasm-clean).

use indicatif::{ProgressBar, ProgressStyle};

/// A progress bar over `total` voxels; returns a closure the engine calls with
/// the number of voxels completed. Drawn to stderr; auto-hidden when stderr is
/// not a terminal. Because the engine reports completion in one call, the bar
/// fills on finish.
pub fn voxel_bar(total: usize) -> (ProgressBar, impl FnMut(u64)) {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::with_template(
            "  {spinner:.cyan} [{elapsed_precise}] [{bar:38.cyan/blue}] {human_pos}/{human_len} voxels ({per_sec}, ETA {eta})",
        )
        .expect("valid template")
        .progress_chars("=>-"),
    );
    let pb2 = pb.clone();
    (pb, move |n| pb2.inc(n))
}
