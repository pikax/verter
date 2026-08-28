//! The single definition of the CSS style-pipeline benchmark universe.
//!
//! `benches/css_bench.rs` and `src/bin/css_latency_gate.rs` both consume this
//! module, so the benchmark-identity universe and the input generators exist
//! exactly once: the identity set a gate compares against is derived from the
//! same data the criterion bench registers, never a hand-maintained copy.
//!
//! Anything else that measures these generator categories should consume this
//! module rather than mirroring the generator bodies.

use std::hint::black_box;

use verter_compiler::style_planner::{
    run_vue_style_cascade, transform_vue_css_modules, transform_vue_scoped_css,
    transform_vue_v_bind, AuthoredStyleInput, PlainCssInput, StyleRewriteOutcome,
};
use verter_css_syntax::CssDialect;

/// The scope id every benchmark and allocation probe uses.
pub const SCOPE_ID: &str = "a4f2eed6";

// =============================================================================
// Input generators — the allocation-category universe.
// =============================================================================

/// Generate CSS with N simple class rules.
pub fn generate_class_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".class-{} {{ color: red; padding: {}px; }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with descendant selectors.
pub fn generate_descendant_selectors(n: usize) -> String {
    (0..n)
        .map(|i| format!(".parent-{} .child-{} {{ color: blue; }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with pseudo-classes.
pub fn generate_pseudo_selectors(n: usize) -> String {
    let pseudos = [":hover", ":focus", ":active", ":first-child", ":last-child"];
    (0..n)
        .map(|i| {
            let pseudo = pseudos[i % pseudos.len()];
            format!(".btn-{}{} {{ color: red; }}", i, pseudo)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with comma-separated selector lists.
pub fn generate_selector_lists(n: usize) -> String {
    (0..n)
        .map(|i| {
            let selectors = (0..3)
                .map(|j| format!(".sel-{}-{}", i, j))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{} {{ margin: {}px; }}", selectors, i)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with v-bind() expressions.
pub fn generate_v_bind_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(".item-{} {{ color: v-bind(color{}); }}", i, i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with quoted v-bind() and dot notation.
pub fn generate_v_bind_dotted(n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                ".item-{} {{ color: v-bind('theme.colors.primary{}'); }}",
                i, i
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :deep() selectors.
pub fn generate_deep_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":deep(.inner-{}) {{ color: red; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :slotted() selectors.
pub fn generate_slotted_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":slotted(.slot-{}) {{ color: red; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with mixed Vue syntax.
pub fn generate_mixed_vue(n: usize) -> String {
    (0..n)
        .map(|i| match i % 3 {
            0 => format!(".item-{} {{ color: v-bind(color{}); }}", i, i),
            1 => format!(":deep(.inner-{}) {{ padding: {}px; }}", i, i),
            _ => format!(":slotted(.slot-{}) {{ margin: {}px; }}", i, i),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with :global() selectors.
pub fn generate_global_rules(n: usize) -> String {
    (0..n)
        .map(|i| format!(":global(.reset-{}) {{ margin: 0; }}", i))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generate CSS with repeated class names (for modules cache hit testing).
pub fn generate_repeated_classes(unique: usize, repeats: usize) -> String {
    let mut rules = Vec::new();
    for r in 0..repeats {
        for i in 0..unique {
            rules.push(format!(".btn-{} {{ padding: {}px; }}", i, r));
        }
    }
    rules.join("\n")
}

/// The per-generator allocation-category universe: one `(category, css)` pair
/// per generator, at the same representative inputs the allocation canaries
/// measure (`n = 50`; `generate_repeated_classes(5, 10)`), so allocation
/// counts stay comparable across probes.
pub fn allocation_category_universe() -> Vec<(&'static str, String)> {
    const N: usize = 50;
    vec![
        ("class_rules", generate_class_rules(N)),
        ("descendant_selectors", generate_descendant_selectors(N)),
        ("pseudo_selectors", generate_pseudo_selectors(N)),
        ("selector_lists", generate_selector_lists(N)),
        ("v_bind_rules", generate_v_bind_rules(N)),
        ("v_bind_dotted", generate_v_bind_dotted(N)),
        ("deep_rules", generate_deep_rules(N)),
        ("slotted_rules", generate_slotted_rules(N)),
        ("mixed_vue", generate_mixed_vue(N)),
        ("global_rules", generate_global_rules(N)),
        ("repeated_classes", generate_repeated_classes(5, 10)),
    ]
}

// =============================================================================
// Measured operations
// =============================================================================

/// The exact measured pipeline call behind one benchmark identity.
///
/// Variant names keep the existing identity strings (`process_style/...`)
/// so the A31 exact-set comparison against the committed legacy baseline
/// stays the same set. Bodies drive `style_planner`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssMeasuredOp {
    /// Full Vue style cascade with the given option axes.
    ProcessStyle { scoped: bool, is_module: bool },
    /// Isolated authored `v-bind()` rewrite.
    Prepass,
    /// Isolated scoped-selector rewrite.
    ApplyScoped,
    /// Isolated CSS-Modules class rewrite.
    ApplyCssModules,
}

fn authored_input(css: &str) -> AuthoredStyleInput<'_> {
    AuthoredStyleInput::new(
        css,
        CssDialect::Css,
        "<style>",
        "standalone:carrier",
        "standalone:carrier-bytes",
    )
    .without_source_map()
}

fn plain_input(css: &str) -> PlainCssInput<'_> {
    PlainCssInput::try_new(
        css,
        CssDialect::Css,
        "<style>",
        "standalone:carrier",
        "standalone:carrier-bytes",
    )
    .expect("plain CSS input")
    .without_source_map()
}

fn observe_rewrite(outcome: StyleRewriteOutcome) {
    match outcome {
        StyleRewriteOutcome::Unchanged { facts } => {
            black_box(&facts.v_bind_vars);
            black_box(&facts.module_classes);
        }
        StyleRewriteOutcome::Rewritten { code, facts, .. } => {
            black_box(&code);
            black_box(&facts.v_bind_vars);
            black_box(&facts.module_classes);
        }
    }
}

impl CssMeasuredOp {
    /// Stable fingerprint of the measured operation and its option axes.
    pub fn fingerprint(self) -> String {
        match self {
            Self::ProcessStyle { scoped, is_module } => {
                format!("process_style:scoped={scoped}:module={is_module}")
            }
            Self::Prepass => "prepass".to_string(),
            Self::ApplyScoped => "apply_scoped".to_string(),
            Self::ApplyCssModules => "apply_css_modules".to_string(),
        }
    }

    /// Perform exactly one measured pipeline call over `css`, black-boxing
    /// the observable outputs the criterion bench black-boxes.
    pub fn run(&self, css: &str) {
        match *self {
            CssMeasuredOp::ProcessStyle { scoped, is_module } => {
                let outcome = run_vue_style_cascade(
                    authored_input(black_box(css)),
                    black_box(SCOPE_ID),
                    is_module,
                    scoped,
                    false,
                );
                black_box(&outcome.code);
                black_box(&outcome.facts.module_classes);
                black_box(&outcome.facts.v_bind_vars);
            }
            CssMeasuredOp::Prepass => {
                let outcome =
                    transform_vue_v_bind(authored_input(black_box(css)), black_box(SCOPE_ID))
                        .expect("v-bind rewrite");
                observe_rewrite(outcome);
            }
            CssMeasuredOp::ApplyScoped => {
                let outcome =
                    transform_vue_scoped_css(plain_input(black_box(css)), black_box(SCOPE_ID))
                        .expect("scoped rewrite");
                observe_rewrite(outcome);
            }
            CssMeasuredOp::ApplyCssModules => {
                let outcome =
                    transform_vue_css_modules(plain_input(black_box(css)), black_box(SCOPE_ID))
                        .expect("modules rewrite");
                observe_rewrite(outcome);
            }
        }
    }
}

// =============================================================================
// Benchmark universe
// =============================================================================

/// One parameterized benchmark instance: a criterion identity plus the input
/// it measures and the pipeline call it performs.
pub struct CssBenchCase {
    /// Criterion benchmark-group name.
    pub group: &'static str,
    /// Criterion function id inside the group (the `BenchmarkId` name, or the
    /// bare `bench_function` name when `param` is `None`).
    pub function_id: &'static str,
    /// The `BenchmarkId` parameter, when the call site is `bench_with_input`.
    pub param: Option<u64>,
    /// The input generator this case's CSS came from (`"inline_single_class"`
    /// for the one literal, non-generator input).
    pub category: &'static str,
    /// The measured pipeline call.
    pub op: CssMeasuredOp,
    /// The generated CSS input.
    pub css: String,
}

impl CssBenchCase {
    /// The full criterion identity string (`group/function_id[/param]`).
    pub fn identity(&self) -> String {
        match self.param {
            Some(p) => format!("{}/{}/{}", self.group, self.function_id, p),
            None => format!("{}/{}", self.group, self.function_id),
        }
    }
}

fn case(
    group: &'static str,
    function_id: &'static str,
    param: Option<u64>,
    category: &'static str,
    op: CssMeasuredOp,
    css: String,
) -> CssBenchCase {
    CssBenchCase {
        group,
        function_id,
        param,
        category,
        op,
        css,
    }
}

/// The complete benchmark universe, in the bench's registration order.
pub fn universe() -> Vec<CssBenchCase> {
    let mut cases = Vec::new();

    // --- group: process_style ------------------------------------------------
    let scoped_only = CssMeasuredOp::ProcessStyle {
        scoped: true,
        is_module: false,
    };
    let modules_only = CssMeasuredOp::ProcessStyle {
        scoped: false,
        is_module: true,
    };
    let scoped_and_modules = CssMeasuredOp::ProcessStyle {
        scoped: true,
        is_module: true,
    };
    let neither = CssMeasuredOp::ProcessStyle {
        scoped: false,
        is_module: false,
    };

    for n in [5u64, 20, 50] {
        cases.push(case(
            "process_style",
            "scoped/classes",
            Some(n),
            "class_rules",
            scoped_only,
            generate_class_rules(n as usize),
        ));
    }
    cases.push(case(
        "process_style",
        "scoped/pseudo",
        Some(20),
        "pseudo_selectors",
        scoped_only,
        generate_pseudo_selectors(20),
    ));
    for n in [5u64, 20, 50] {
        cases.push(case(
            "process_style",
            "modules/classes",
            Some(n),
            "class_rules",
            modules_only,
            generate_class_rules(n as usize),
        ));
    }
    cases.push(case(
        "process_style",
        "scoped+modules",
        Some(20),
        "class_rules",
        scoped_and_modules,
        generate_class_rules(20),
    ));
    for n in [1u64, 5, 20] {
        cases.push(case(
            "process_style",
            "v-bind/simple",
            Some(n),
            "v_bind_rules",
            scoped_only,
            generate_v_bind_rules(n as usize),
        ));
    }
    cases.push(case(
        "process_style",
        "passthrough",
        Some(20),
        "class_rules",
        neither,
        generate_class_rules(20),
    ));

    // --- group: prepass ------------------------------------------------------
    for n in [5u64, 20, 50] {
        cases.push(case(
            "prepass",
            "passthrough",
            Some(n),
            "class_rules",
            CssMeasuredOp::Prepass,
            generate_class_rules(n as usize),
        ));
    }
    for n in [1u64, 5, 20] {
        cases.push(case(
            "prepass",
            "v-bind/simple",
            Some(n),
            "v_bind_rules",
            CssMeasuredOp::Prepass,
            generate_v_bind_rules(n as usize),
        ));
    }
    for n in [1u64, 5, 20] {
        cases.push(case(
            "prepass",
            "v-bind/dotted",
            Some(n),
            "v_bind_dotted",
            CssMeasuredOp::Prepass,
            generate_v_bind_dotted(n as usize),
        ));
    }
    for n in [5u64, 20] {
        cases.push(case(
            "prepass",
            "deep",
            Some(n),
            "deep_rules",
            CssMeasuredOp::Prepass,
            generate_deep_rules(n as usize),
        ));
    }
    for n in [5u64, 20] {
        cases.push(case(
            "prepass",
            "slotted",
            Some(n),
            "slotted_rules",
            CssMeasuredOp::Prepass,
            generate_slotted_rules(n as usize),
        ));
    }
    for n in [6u64, 30] {
        cases.push(case(
            "prepass",
            "mixed",
            Some(n),
            "mixed_vue",
            CssMeasuredOp::Prepass,
            generate_mixed_vue(n as usize),
        ));
    }

    // --- group: scoped -------------------------------------------------------
    cases.push(case(
        "scoped",
        "single_class",
        None,
        "inline_single_class",
        CssMeasuredOp::ApplyScoped,
        ".box { color: red; }".to_string(),
    ));
    for n in [5u64, 20] {
        cases.push(case(
            "scoped",
            "descendant",
            Some(n),
            "descendant_selectors",
            CssMeasuredOp::ApplyScoped,
            generate_descendant_selectors(n as usize),
        ));
    }
    for n in [5u64, 20] {
        cases.push(case(
            "scoped",
            "selector_list",
            Some(n),
            "selector_lists",
            CssMeasuredOp::ApplyScoped,
            generate_selector_lists(n as usize),
        ));
    }
    for n in [5u64, 20] {
        cases.push(case(
            "scoped",
            "pseudo",
            Some(n),
            "pseudo_selectors",
            CssMeasuredOp::ApplyScoped,
            generate_pseudo_selectors(n as usize),
        ));
    }
    for n in [5u64, 20] {
        // After prepass, :global is left as-is.
        cases.push(case(
            "scoped",
            "global",
            Some(n),
            "global_rules",
            CssMeasuredOp::ApplyScoped,
            generate_global_rules(n as usize),
        ));
    }

    // --- group: modules ------------------------------------------------------
    for n in [3u64, 10, 30] {
        cases.push(case(
            "modules",
            "unique_classes",
            Some(n),
            "class_rules",
            CssMeasuredOp::ApplyCssModules,
            generate_class_rules(n as usize),
        ));
    }
    for repeats in [2u64, 5, 10] {
        cases.push(case(
            "modules",
            "repeated_5x",
            Some(repeats),
            "repeated_classes",
            CssMeasuredOp::ApplyCssModules,
            generate_repeated_classes(5, repeats as usize),
        ));
    }

    cases
}

/// The group names, in registration order.
pub const GROUPS: [&str; 4] = ["process_style", "prepass", "scoped", "modules"];

/// The full identity-string universe, sorted.
pub fn identity_universe() -> std::collections::BTreeSet<String> {
    universe().iter().map(CssBenchCase::identity).collect()
}
