//! Static SVG plots for simulation results via plotters.

use anyhow::Result;
use plotters::prelude::*;
use std::path::Path;

use crate::sim::report::SensitivityReport;

/// Overlay clean, noisy, and fitted-curve signals vs protocol index.
pub fn plot_single_voxel(
    clean: &[f64],
    noisy: &[f64],
    fitted_curve: &[f64],
    path: &Path,
) -> Result<()> {
    let root = SVGBackend::new(path, (800, 500)).into_drawing_area();
    root.fill(&WHITE)?;
    let n = clean.len().max(noisy.len()).max(fitted_curve.len());
    let ymin = clean
        .iter()
        .chain(noisy)
        .chain(fitted_curve)
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let ymax = clean
        .iter()
        .chain(noisy)
        .chain(fitted_curve)
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let pad = 0.05 * (ymax - ymin).abs().max(1e-6);

    let mut chart = ChartBuilder::on(&root)
        .caption("Single-voxel signal", ("sans-serif", 22))
        .margin(15)
        .x_label_area_size(40)
        .y_label_area_size(55)
        .build_cartesian_2d(0f64..(n as f64 - 1.0).max(1.0), (ymin - pad)..(ymax + pad))?;
    chart
        .configure_mesh()
        .x_desc("volume")
        .y_desc("signal")
        .draw()?;

    chart
        .draw_series(LineSeries::new(
            clean.iter().enumerate().map(|(i, &y)| (i as f64, y)),
            BLUE,
        ))?
        .label("truth")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLUE));
    chart
        .draw_series(
            noisy
                .iter()
                .enumerate()
                .map(|(i, &y)| Circle::new((i as f64, y), 3, RED.filled())),
        )?
        .label("noisy")
        .legend(|(x, y)| Circle::new((x + 10, y), 3, RED.filled()));
    chart
        .draw_series(LineSeries::new(
            fitted_curve.iter().enumerate().map(|(i, &y)| (i as f64, y)),
            GREEN,
        ))?
        .label("fit")
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], GREEN));
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;
    root.present()?;
    Ok(())
}

/// Plot bias±std vs the swept parameter, one line per estimated parameter.
pub fn plot_sensitivity(report: &SensitivityReport, path: &Path) -> Result<()> {
    let root = SVGBackend::new(path, (800, 500)).into_drawing_area();
    root.fill(&WHITE)?;
    if report.points.is_empty() {
        root.present()?;
        return Ok(());
    }
    let xs: Vec<f64> = report.points.iter().map(|p| p.value).collect();
    let (mut xmin, mut xmax) = (xs[0], *xs.last().unwrap());
    if xmax < xmin {
        std::mem::swap(&mut xmin, &mut xmax);
    }
    if (xmax - xmin).abs() < 1e-12 {
        xmin -= 0.5;
        xmax += 0.5;
    }
    let biases: Vec<f64> = report
        .points
        .iter()
        .flat_map(|p| p.stats.iter().map(|s| s.bias))
        .collect();
    let ymin = biases.iter().cloned().fold(0.0_f64, f64::min);
    let ymax = biases.iter().cloned().fold(0.0_f64, f64::max);
    let pad = 0.1 * (ymax - ymin).abs().max(1e-6);

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!("Sensitivity to {}", report.swept_param),
            ("sans-serif", 22),
        )
        .margin(15)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(xmin..xmax, (ymin - pad)..(ymax + pad))?;
    chart
        .configure_mesh()
        .x_desc(&report.swept_param)
        .y_desc("bias")
        .draw()?;

    // One series per estimated param name (taken from the first point).
    let names: Vec<String> = report.points[0]
        .stats
        .iter()
        .map(|s| s.name.clone())
        .collect();
    let palette = [&BLUE, &RED, &GREEN, &BLACK, &BLACK, &BLACK];
    for (si, name) in names.iter().enumerate() {
        let color = palette[si % palette.len()];
        let series: Vec<(f64, f64)> = report
            .points
            .iter()
            .filter_map(|p| {
                p.stats
                    .iter()
                    .find(|s| &s.name == name)
                    .map(|s| (p.value, s.bias))
            })
            .collect();
        chart
            .draw_series(LineSeries::new(series, color))?
            .label(name.clone())
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
    }
    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;
    root.present()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_voxel_svg_written() {
        let dir = std::env::temp_dir();
        let path = dir.join("qmrust_sim_sv_test.svg");
        plot_single_voxel(
            &[0.9, 0.8, 0.7],
            &[0.92, 0.78, 0.71],
            &[0.9, 0.8, 0.7],
            &path,
        )
        .unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("<svg"), "not an svg");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn sensitivity_svg_written_and_degenerate_safe() {
        use crate::sim::report::{ParamStat, SweepPoint};
        let mk = |v: f64, bias: f64| SweepPoint {
            value: v,
            stats: vec![ParamStat {
                name: "F".into(),
                truth: 0.1,
                mean: 0.1 + bias,
                std: 0.01,
                bias,
                rmse: bias.abs(),
            }],
        };
        // normal range
        let r = SensitivityReport {
            mode: "sensitivity".into(),
            model: "qmt_spgr".into(),
            swept_param: "F".into(),
            points: vec![mk(0.1, 0.0), mk(0.2, 0.01)],
        };
        let p = std::env::temp_dir().join("qmrust_sim_sens_test.svg");
        plot_sensitivity(&r, &p).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("<svg"));
        std::fs::remove_file(&p).ok();
        // degenerate range (all equal x) must not panic and still writes an SVG
        let rd = SensitivityReport {
            mode: "sensitivity".into(),
            model: "qmt_spgr".into(),
            swept_param: "F".into(),
            points: vec![mk(0.15, 0.0), mk(0.15, 0.0)],
        };
        let pd = std::env::temp_dir().join("qmrust_sim_sens_degen.svg");
        plot_sensitivity(&rd, &pd).unwrap();
        assert!(std::fs::read_to_string(&pd).unwrap().contains("<svg"));
        std::fs::remove_file(&pd).ok();
    }
}
