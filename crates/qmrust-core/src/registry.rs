//! Model registry: the one place that maps a config `model:` name (and a BIDS
//! grouping suffix) to the builder that constructs it. Adding a model means a
//! new module implementing `Model` plus one entry here.

use crate::core::model::{EffectiveConfig, Model, Protocol};
use crate::models;
use anyhow::Result;

pub type Builder = fn(&serde_yaml::Value, &Protocol) -> Result<Box<dyn Model>>;
pub type Describer = fn(&serde_yaml::Value) -> Result<Box<dyn Model>>;
pub type Dumper = fn(&serde_yaml::Value) -> Result<String>;
pub type Effective = fn(&serde_yaml::Value, &Protocol) -> Result<EffectiveConfig>;

/// Method family a model belongs to. Determines its documentation directory,
/// so the generated URL is derived from the registry, never hand-chosen. Add a
/// variant only when a model needs it — an empty category has no page.
///
/// The taxonomy is two levels: every category names a [`family`](Self::family)
/// a reader browses by, and some additionally sit in a
/// [`subgroup`](Self::subgroup) within it. Only the family reaches the URL, so
/// a subdivision can be introduced (or reorganized) without moving any page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    T1Relaxometry,
    T2Relaxometry,
    FieldMapping,
    SemiQuantitativeMt,
    QuantitativeMt,
}

impl Category {
    /// URL/directory slug beneath the documentation's `models/` root. Both
    /// magnetization-transfer categories share one, because the split between
    /// them is a reading aid rather than a different kind of method.
    pub fn slug(&self) -> &'static str {
        match self {
            Category::T1Relaxometry => "t1-relaxometry",
            Category::T2Relaxometry => "t2-relaxometry",
            Category::FieldMapping => "field-mapping",
            Category::SemiQuantitativeMt | Category::QuantitativeMt => "magnetization-transfer",
        }
    }

    /// Top-level heading a reader browses by.
    pub fn family(&self) -> &'static str {
        match self {
            Category::T1Relaxometry => "T1 Relaxometry",
            Category::T2Relaxometry => "T2 Relaxometry",
            Category::FieldMapping => "Field Mapping",
            Category::SemiQuantitativeMt | Category::QuantitativeMt => "Magnetization Transfer",
        }
    }

    /// Heading within the family, for families worth subdividing. `None` when
    /// the family's models sit directly under it.
    pub fn subgroup(&self) -> Option<&'static str> {
        match self {
            Category::SemiQuantitativeMt => Some("Semi-quantitative MT"),
            Category::QuantitativeMt => Some("Quantitative MT"),
            _ => None,
        }
    }

    /// The most specific heading this category carries — its subgroup when it
    /// has one, its family otherwise.
    pub fn title(&self) -> &'static str {
        self.subgroup().unwrap_or_else(|| self.family())
    }

    /// Lucide icon name for this category's family, for surfaces that show one.
    ///
    /// Named here for the same reason the title and the order are: a family is
    /// a registry fact, and a picker that mapped headings to glyphs itself
    /// would need editing every time the taxonomy moved. Categories sharing a
    /// family share its icon.
    pub fn icon(&self) -> &'static str {
        match self {
            Category::T1Relaxometry => "spline-pointer",
            Category::T2Relaxometry => "spline-pointer-mirrored",
            Category::FieldMapping => "circle-dot",
            Category::SemiQuantitativeMt | Category::QuantitativeMt => "waves-arrow-up",
        }
    }

    /// Where this category sits in the order a reader is offered them.
    ///
    /// The registry lists models alphabetically, which is the wrong order to
    /// read a method taxonomy in, and every surface that groups models — the
    /// documentation gallery, the playground's picker — wants the same one.
    /// Sorting by this and then grouping consecutive runs yields the whole
    /// two-level tree, so the order lives here rather than being restated
    /// wherever a list is drawn.
    pub fn order(&self) -> u8 {
        match self {
            Category::T1Relaxometry => 0,
            Category::T2Relaxometry => 1,
            Category::FieldMapping => 2,
            Category::SemiQuantitativeMt => 3,
            Category::QuantitativeMt => 4,
        }
    }
}

/// Canonical example configs for a model, repo-relative.
pub struct Recipes {
    /// Config for the `--bids-dir` path; omits the acquisition arrays, which
    /// come from sidecars.
    pub bids: &'static str,
    /// Config for the `--data`/`--mat-data` path. This one carries the full
    /// acquisition, so it is also what a model is *described* from when
    /// interrogating its measurement axis.
    pub non_bids: &'static str,
    /// Simulation recipe: the non-BIDS acquisition plus a `sim:` block naming
    /// ground-truth parameters. Every model implements a forward signal, so
    /// every model has one.
    pub sim: &'static str,
}

/// Documentation metadata: the facts about a model that code cannot introspect.
/// Everything else on a generated documentation page comes from the `Model`
/// trait itself.
pub struct ModelDoc {
    /// Display name, e.g. "qMT-SPGR".
    pub title: &'static str,
    pub category: Category,
    /// One paragraph: what the model measures, and how.
    pub summary: &'static str,
    /// Signal model as LaTeX, without `$` delimiters.
    pub equation: &'static str,
    /// `(param_name, meaning, unit)`. Each `param_name` must appear in the
    /// model's `param_names()`; unitless quantities use `""`.
    pub symbols: &'static [(&'static str, &'static str, &'static str)],
    /// BibTeX keys, resolved against `docs/references.bib`.
    pub citations: &'static [&'static str],
    /// Repo-relative module directory, for source links.
    pub source_dir: &'static str,
    pub recipes: Recipes,
    /// Config keys whose value is one of a fixed set, for rendering a
    /// dropdown instead of free text. `key` supports a dotted path into a
    /// nested config group (e.g. `"pulse.shape"`). Each set here must be
    /// accepted by the model's own `validate_options` — enforced by
    /// `qmrust-cli`'s `catalog::tests::declared_enums_match_validate_options`.
    pub enums: &'static [(&'static str, &'static [&'static str])],
    /// `(output_name, min, max)` — the physically sensible display window for
    /// an output map, in that output's own unit.
    ///
    /// A window derived from the data alone is only as good as the data: an
    /// unmasked fit puts background noise and failed voxels in the same
    /// histogram as tissue, which stretches the scale until the anatomy is
    /// flat grey. These are the ranges a reader of *this* quantity expects,
    /// so a map is legible before anyone touches a slider. Each name must be
    /// one of the model's `output_names()` — enforced by `qmrust-cli`'s
    /// `catalog::tests::declared_display_ranges_name_real_outputs`. An output
    /// with no entry falls back to a percentile window over its own values.
    pub display_ranges: &'static [(&'static str, f64, f64)],
}

pub struct ModelEntry {
    pub name: &'static str,
    pub bids_suffix: &'static str,
    pub build: Builder,
    pub describe: Describer,
    pub dump: Dumper,
    pub effective: Effective,
    pub doc: ModelDoc,
}

pub fn all() -> &'static [ModelEntry] {
    &[
        ModelEntry {
            name: "b1_afi",
            bids_suffix: "TB1AFI",
            build: models::b1_afi::build,
            describe: models::b1_afi::describe,
            dump: models::b1_afi::dump,
            effective: models::b1_afi::effective,
            doc: ModelDoc {
                title: "Actual Flip Angle B1+",
                category: Category::FieldMapping,
                summary: "Maps the transmit (B1+) field from a single spoiled \
                    gradient-echo sequence that interleaves two excitation \
                    repetition times at one nominal flip angle. In the pulsed \
                    steady state the ratio of the two signals depends on the \
                    achieved flip angle alone, so proton density and receive \
                    sensitivity cancel. The result is dimensionless — the \
                    achieved flip angle as a fraction of the nominal one — and \
                    feeds the B1 correction of models such as MTsat and VFA. \
                    The closed form assumes the repetition times are short \
                    against T1, and underestimates B1 as that assumption \
                    weakens.",
                equation: r"\cos\theta = \frac{r n - 1}{n - r},\quad r = \left|\frac{S_{\mathrm{TR}_2}}{S_{\mathrm{TR}_1}}\right|,\quad n = \frac{\mathrm{TR}_2}{\mathrm{TR}_1},\quad B_1^{+} = \frac{\theta}{\theta_\mathrm{nom}}",
                symbols: &[
                    ("B1", "Relative transmit field", ""),
                    ("T1", "Longitudinal relaxation time", "s"),
                ],
                citations: &["yarnykh2007"],
                source_dir: "crates/qmrust-core/src/models/b1_afi",
                recipes: Recipes {
                    bids: "recipes/bids/b1_afi_config.yaml",
                    non_bids: "recipes/non-bids/b1_afi_config.yaml",
                    sim: "recipes/sim/b1_afi_sim.yaml",
                },
                enums: &[],
                display_ranges: &[
                    // A well-shimmed transmit coil sits within a few tens of
                    // percent of nominal; this window keeps 1.0 mid-scale.
                    ("B1", 0.5, 1.5),
                ],
            },
        },
        ModelEntry {
            name: "b1_dam",
            bids_suffix: "TB1DAM",
            build: models::b1_dam::build,
            describe: models::b1_dam::describe,
            dump: models::b1_dam::dump,
            effective: models::b1_dam::effective,
            doc: ModelDoc {
                title: "Double Angle B1+",
                category: Category::FieldMapping,
                summary: "Maps the transmit (B1+) field from two spoiled \
                    gradient-echo volumes acquired at flip angles alpha and \
                    twice alpha. The ratio of the two signals depends only on \
                    the achieved flip angle, so proton density and receive \
                    sensitivity cancel and the map needs no calibration. The \
                    result is dimensionless — the achieved flip angle as a \
                    fraction of the nominal one — and feeds the B1 correction \
                    of models such as MTsat and VFA.",
                equation: r"B_1^{+} = \frac{\left|\arccos\left(\frac{S_{2\alpha}}{2 S_{\alpha}}\right)\right|}{\alpha}",
                symbols: &[
                    ("B1", "Relative transmit field", ""),
                    ("A", "Signal amplitude", ""),
                ],
                citations: &["insko1993"],
                source_dir: "crates/qmrust-core/src/models/b1_dam",
                recipes: Recipes {
                    bids: "recipes/bids/b1_dam_config.yaml",
                    non_bids: "recipes/non-bids/b1_dam_config.yaml",
                    sim: "recipes/sim/b1_dam_sim.yaml",
                },
                enums: &[],
                display_ranges: &[
                    // A well-shimmed transmit coil sits within a few tens of
                    // percent of nominal; this window keeps 1.0 mid-scale.
                    ("B1", 0.5, 1.5),
                ],
            },
        },
        ModelEntry {
            name: "mt_ratio",
            bids_suffix: "MTR",
            build: models::mt_ratio::build,
            describe: models::mt_ratio::describe,
            dump: models::mt_ratio::dump,
            effective: models::mt_ratio::effective,
            doc: ModelDoc {
                title: "MT Ratio",
                category: Category::SemiQuantitativeMt,
                summary: "Computes the magnetization transfer ratio from two \
                    images: one acquired with an off-resonance saturation pulse \
                    and one without. MTR is a semi-quantitative percentage that \
                    reflects the pool of macromolecule-bound protons, but it \
                    depends on the saturation pulse and sequence timing, so \
                    values are not comparable across protocols.",
                equation: r"\mathrm{MTR} = 100 \times \frac{S_\mathrm{off} - S_\mathrm{on}}{S_\mathrm{off}}",
                symbols: &[("MTR", "Magnetization transfer ratio", "%")],
                citations: &["wolff1989"],
                source_dir: "crates/qmrust-core/src/models/mt_ratio",
                recipes: Recipes {
                    bids: "recipes/bids/mt_ratio_config.yaml",
                    non_bids: "recipes/non-bids/mt_ratio_config.yaml",
                    sim: "recipes/sim/mt_ratio_sim.yaml",
                },
                enums: &[],
                display_ranges: &[
                    // MTR in healthy white matter sits near 40-50%; the window
                    // spans grey matter to white without clipping either.
                    ("MTR", 0.0, 60.0),
                ],
            },
        },
        ModelEntry {
            name: "mt_sat",
            bids_suffix: "MTS",
            build: models::mt_sat::build,
            describe: models::mt_sat::describe,
            dump: models::mt_sat::dump,
            effective: models::mt_sat::effective,
            doc: ModelDoc {
                title: "MT Saturation",
                category: Category::SemiQuantitativeMt,
                summary: "Derives the MT saturation parameter from three spoiled \
                    gradient-echo volumes — MT-weighted, PD-weighted and \
                    T1-weighted. Unlike MTR, MTsat removes the leading-order \
                    dependence on T1 and on transmit-field inhomogeneity, making \
                    it far more comparable across sites; supplying a B1 map \
                    applies the residual correction. T1 (and optionally MTR) \
                    fall out of the same three volumes.",
                equation: r"\delta = \left(\frac{A\,\alpha_\mathrm{MT}}{S_\mathrm{MT}} - 1\right)\frac{\mathrm{TR}_\mathrm{MT}}{T_1} - \frac{\alpha_\mathrm{MT}^{2}}{2}",
                symbols: &[
                    ("A", "Apparent signal amplitude", ""),
                    ("T1", "Longitudinal relaxation time", "s"),
                    ("MTSAT", "MT saturation", "%"),
                ],
                citations: &["helms2008"],
                source_dir: "crates/qmrust-core/src/models/mt_sat",
                recipes: Recipes {
                    bids: "recipes/bids/mt_sat_config.yaml",
                    non_bids: "recipes/non-bids/mt_sat_config.yaml",
                    sim: "recipes/sim/mt_sat_sim.yaml",
                },
                enums: &[],
                display_ranges: &[
                    // MTsat is a few percent: ~5% in white matter, ~2% in grey.
                    ("MTSAT", 0.0, 6.0),
                    // T1 at 3T: ~0.8 s white matter, ~1.4 s grey, ~4 s CSF.
                    ("T1", 0.0, 3.0),
                    ("MTR", 0.0, 60.0),
                ],
            },
        },
        ModelEntry {
            name: "mono_t2",
            bids_suffix: "MESE",
            build: models::mono_t2::build,
            describe: models::mono_t2::describe,
            dump: models::mono_t2::dump,
            effective: models::mono_t2::effective,
            doc: ModelDoc {
                title: "Monoexp T2",
                category: Category::T2Relaxometry,
                summary: "Fits the transverse relaxation time T2 from a \
                    multi-echo spin-echo series as a mono-exponential decay with \
                    a free amplitude. The fit runs either as a log-linear \
                    least-squares solve or as a bounded non-linear exponential \
                    fit, optionally dropping the first echo — commonly \
                    contaminated by stimulated-echo effects — or adding a \
                    constant offset term to absorb noise-floor bias.",
                equation: r"S(\mathrm{TE}) = M_0\,\exp\!\left(-\mathrm{TE}/T_2\right)",
                symbols: &[
                    ("T2", "Transverse relaxation time", "s"),
                    ("M0", "Signal amplitude at TE = 0 (arbitrary units)", ""),
                ],
                citations: &["milford2015"],
                source_dir: "crates/qmrust-core/src/models/mono_t2",
                recipes: Recipes {
                    bids: "recipes/bids/mono_t2_config.yaml",
                    non_bids: "recipes/non-bids/mono_t2_config.yaml",
                    sim: "recipes/sim/mono_t2_sim.yaml",
                },
                enums: &[("fit_type", &["exponential", "linear"])],
                display_ranges: &[
                    // T2 at 3T: ~0.07 s white matter, ~0.09 s grey; CSF far
                    // longer, and left to clip rather than flatten the tissue.
                    ("T2", 0.0, 0.15),
                    // M0 is an arbitrary-unit amplitude, so its scale belongs
                    // to the data and no fixed window can be right.
                ],
            },
        },
        ModelEntry {
            name: "inversion_recovery",
            bids_suffix: "IRT1",
            build: models::inversion_recovery::build,
            describe: models::inversion_recovery::describe,
            dump: models::inversion_recovery::dump,
            effective: models::inversion_recovery::effective,
            doc: ModelDoc {
                title: "Inversion Recovery",
                category: Category::T1Relaxometry,
                summary: "Fits the longitudinal relaxation time T1 from a series \
                    of inversion-recovery images acquired at different inversion \
                    times. The magnitude signal is modelled as an exponential \
                    recovery with a free amplitude and offset, which together \
                    absorb imperfect inversion efficiency, so no assumption \
                    about a perfect 180° pulse is needed. T1 is recovered by a \
                    grid search over the configured range followed by a local \
                    zoom refinement.",
                equation: r"S(\mathrm{TI}) = a\,\exp\!\left(-\mathrm{TI}/T_1\right) + b",
                symbols: &[
                    ("T1", "Longitudinal relaxation time", "s"),
                    ("a", "Recovery amplitude (absorbs inversion efficiency)", ""),
                    ("b", "Signal offset", ""),
                ],
                citations: &["barral2010"],
                source_dir: "crates/qmrust-core/src/models/inversion_recovery",
                recipes: Recipes {
                    bids: "recipes/bids/irt1_config.yaml",
                    non_bids: "recipes/non-bids/irt1_config.yaml",
                    sim: "recipes/sim/irt1_sim.yaml",
                },
                enums: &[("method", &["magnitude", "complex"])],
                display_ranges: &[("T1", 0.0, 3.0)],
            },
        },
        ModelEntry {
            name: "vfa_t1",
            bids_suffix: "VFA",
            build: models::vfa_t1::build,
            describe: models::vfa_t1::describe,
            dump: models::vfa_t1::dump,
            effective: models::vfa_t1::effective,
            doc: ModelDoc {
                title: "Variable Flip Angle",
                category: Category::T1Relaxometry,
                summary: "Fits the longitudinal relaxation time T1 from spoiled \
                    gradient-echo images acquired at two or more excitation flip \
                    angles and a single repetition time. Dividing the \
                    steady-state signal by the sine of the flip angle \
                    linearizes the model, so T1 and the equilibrium \
                    magnetization follow from an ordinary least-squares line \
                    through the transformed data — a closed-form solve, with no \
                    iteration. That transform also divides the noise, which \
                    biases the estimate at low SNR, so a nonlinear fit_type is \
                    available that minimises residuals on the signal equation \
                    itself. A transmit field map, when supplied, scales the \
                    nominal flip angles to the actual ones before the fit.",
                equation: r"S(\alpha) = M_0 \sin(\alpha)\,\frac{1 - e^{-T_R/T_1}}{1 - \cos(\alpha)\,e^{-T_R/T_1}}",
                symbols: &[
                    ("M0", "Equilibrium magnetization", ""),
                    ("T1", "Longitudinal relaxation time", "s"),
                ],
                citations: &["fram1987"],
                source_dir: "crates/qmrust-core/src/models/vfa_t1",
                recipes: Recipes {
                    bids: "recipes/bids/vfa_t1_config.yaml",
                    non_bids: "recipes/non-bids/vfa_t1_config.yaml",
                    sim: "recipes/sim/vfa_t1_sim.yaml",
                },
                enums: &[("fit_type", &["linear", "nonlinear"])],
                display_ranges: &[("T1", 0.0, 3.0)],
            },
        },
        ModelEntry {
            name: "qmt_spgr",
            bids_suffix: "QMTSPGR",
            build: models::qmt_spgr::build,
            describe: models::qmt_spgr::describe,
            dump: models::qmt_spgr::dump,
            effective: models::qmt_spgr::effective,
            doc: ModelDoc {
                title: "qMT-SPGR",
                category: Category::QuantitativeMt,
                summary: "Two-pool quantitative magnetization transfer from a \
                    spoiled gradient-echo sequence with off-resonance saturation \
                    sampled across a grid of saturation flip angles and \
                    frequency offsets. A free-water pool exchanges magnetization \
                    with a restricted macromolecular pool; fitting the sampled \
                    Z-spectrum recovers the bound-pool fraction and the exchange \
                    rate. Two steady-state solutions are available — Ramani's \
                    closed form and the Sled–Pike rectangular-pulse \
                    approximation — and B1, B0 and R1 maps constrain the fit \
                    when supplied.",
                equation: r"\begin{aligned}
\frac{dM_f}{dt} &= R_{1f}\left(M_{0f} - M_f\right) - k_f M_f + k_r M_r - W_f(\Delta, \alpha)\,M_f \\
\frac{dM_r}{dt} &= R_{1r}\left(M_{0r} - M_r\right) + k_f M_f - k_r M_r - W_r(\Delta, \alpha)\,M_r \\
F &= M_{0r}/M_{0f}, \qquad k_f F = k_r
\end{aligned}",
                symbols: &[
                    ("F", "Bound-pool fraction M0r/M0f", ""),
                    ("kr", "Exchange rate, restricted to free pool", "1/s"),
                    ("R1f", "Free-pool longitudinal relaxation rate", "1/s"),
                    ("R1r", "Restricted-pool longitudinal relaxation rate", "1/s"),
                    ("T2f", "Free-pool transverse relaxation time", "s"),
                    ("T2r", "Restricted-pool transverse relaxation time", "s"),
                ],
                citations: &["ramani2002", "sled2001", "cabana2015"],
                source_dir: "crates/qmrust-core/src/models/qmt_spgr",
                recipes: Recipes {
                    bids: "recipes/bids/qmt_config_ramani.yaml",
                    non_bids: "recipes/non-bids/qmt_config_ramani.yaml",
                    sim: "recipes/sim/qmt_sim_ramani.yaml",
                },
                enums: &[
                    ("qmt_spgr.model", &["Ramani", "SledPikeRP"]),
                    ("qmt_spgr.lineshape", &["SuperLorentzian"]),
                    ("qmt_spgr.pulse.shape", &["gausshann"]),
                ],
                display_ranges: &[
                    // Bound pool fraction, a fraction not a percent.
                    ("F", 0.0, 0.25),
                    // Exchange rate from bound to free pool.
                    ("kr", 0.0, 60.0),
                    ("R1f", 0.0, 2.0),
                    ("R1r", 0.0, 2.0),
                    ("T2f", 0.0, 0.1),
                    // The bound pool's T2 is on the order of microseconds.
                    ("T2r", 0.0, 2e-5),
                ],
            },
        },
    ]
}

pub fn by_name(name: &str) -> Option<&'static ModelEntry> {
    all().iter().find(|e| e.name == name)
}

pub fn by_bids_suffix(suffix: &str) -> Option<&'static ModelEntry> {
    all().iter().find(|e| e.bids_suffix == suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_by_name() {
        assert!(by_name("inversion_recovery").is_some());
        assert!(by_name("qmt_spgr").is_some());
        assert!(by_name("nope").is_none());
    }

    #[test]
    fn lookup_by_bids_suffix() {
        assert_eq!(by_bids_suffix("IRT1").unwrap().name, "inversion_recovery");
        assert_eq!(by_bids_suffix("QMTSPGR").unwrap().name, "qmt_spgr");
    }

    #[test]
    fn builds_via_registry() {
        let v: serde_yaml::Value = serde_yaml::from_str("model: qmt_spgr\n").unwrap();
        let entry = by_name("qmt_spgr").unwrap();
        let m = (entry.build)(&v, &crate::core::model::Protocol::default()).unwrap();
        assert_eq!(m.output_names().len(), 8);
    }

    #[test]
    fn every_model_declares_documentation_metadata() {
        for e in all() {
            let d = &e.doc;
            assert!(!d.title.is_empty(), "{}: empty doc title", e.name);
            assert!(!d.summary.is_empty(), "{}: empty doc summary", e.name);
            assert!(!d.equation.is_empty(), "{}: empty doc equation", e.name);
            assert!(!d.symbols.is_empty(), "{}: no doc symbols", e.name);
            assert!(!d.citations.is_empty(), "{}: no doc citations", e.name);
            assert!(
                d.source_dir.starts_with("crates/qmrust-core/src/models/"),
                "{}: source_dir must be repo-relative, got '{}'",
                e.name,
                d.source_dir
            );
            assert!(
                d.recipes.bids.starts_with("recipes/bids/"),
                "{}: bids recipe must live under recipes/bids/, got '{}'",
                e.name,
                d.recipes.bids
            );
            assert!(
                d.recipes.non_bids.starts_with("recipes/non-bids/"),
                "{}: non-BIDS recipe must live under recipes/non-bids/, got '{}'",
                e.name,
                d.recipes.non_bids
            );
        }
    }

    #[test]
    fn category_slugs_are_url_safe_and_stable() {
        for e in all() {
            let slug = e.doc.category.slug();
            assert!(
                slug.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{}: category slug '{}' is not URL-safe",
                e.name,
                slug
            );
            assert!(!e.doc.category.title().is_empty());
            assert!(!e.doc.category.family().is_empty());
        }
    }

    /// Several categories may share a documentation directory — the two
    /// magnetization-transfer ones do — but only within a single family. Two
    /// families writing into one directory would interleave their pages under
    /// a heading naming just one of them, and the gallery links (built from the
    /// slug) would point into the wrong section.
    #[test]
    fn categories_sharing_a_slug_belong_to_one_family() {
        let mut family_of: std::collections::BTreeMap<&str, &str> = Default::default();
        for e in all() {
            let (slug, family) = (e.doc.category.slug(), e.doc.category.family());
            if let Some(seen) = family_of.insert(slug, family) {
                assert_eq!(
                    seen, family,
                    "slug '{slug}' is claimed by both '{seen}' and '{family}'"
                );
            }
        }
    }

    /// Reading order must be a total order over the categories in play: two
    /// categories at the same rank would group and sort unpredictably in both
    /// the gallery and the picker, which sort by exactly this.
    #[test]
    fn category_order_is_unambiguous() {
        let mut by_order: std::collections::BTreeMap<u8, &str> = Default::default();
        for e in all() {
            let c = e.doc.category;
            if let Some(seen) = by_order.insert(c.order(), c.title()) {
                assert_eq!(
                    seen,
                    c.title(),
                    "order {} is shared by '{seen}' and '{}'",
                    c.order(),
                    c.title()
                );
            }
        }
    }
}
