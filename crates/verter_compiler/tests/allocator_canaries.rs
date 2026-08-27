//! Allocation canaries — the single counting-allocator integration binary
//! for `verter_compiler`.
//!
//! A `#[global_allocator]` is process-global and exactly one may be
//! installed per binary, so every allocation-counting test for this crate
//! co-resides here behind one counting allocator. Counts are thread-local:
//! the Rust test harness may allocate on sibling worker threads outside any
//! test-body lock, and those allocations must not corrupt another thread's
//! measurement window. Every measured path in this binary is synchronous
//! and remains on its harness thread.
//!
//! This binary is allocator-ONLY: it carries no non-allocation tests. The
//! rest of the integration suite lives in the `main` binary.
//!
//! # Style-pipeline allocation instruments
//!
//! This binary records live allocations through both the lightningcss
//! `css::process_style` entry point and `style_planner::run_vue_style_cascade`,
//! one count per `crates/verter_bench/benches/css_bench.rs` generator category
//! (the same input generators `verter_bench::css_identities` registers). The
//! numbers this binary prints (`eprintln!` markers, `cargo test -- --nocapture`)
//! are the live instruments. The 1.2x ceiling lives in
//! `converged_style_pipeline_allocation_within_ratified_ceiling`, which
//! compares each category against the recaptured legacy counts committed in
//! this file.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;

thread_local! {
    static ALLOC_COUNTER: Cell<u64> = const { Cell::new(0) };
    static ALLOC_BYTES: Cell<u64> = const { Cell::new(0) };
}

fn increment_alloc_counter(bytes: usize) {
    // Allocation can occur while a thread is tearing down TLS. Do not turn
    // an otherwise valid allocation into a panic if this key is no longer
    // accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
    let _ = ALLOC_BYTES.try_with(|total| total.set(total.get().wrapping_add(bytes as u64)));
}

fn reset_alloc_counter() {
    ALLOC_COUNTER.with(|counter| counter.set(0));
    ALLOC_BYTES.with(|total| total.set(0));
}

fn alloc_count() -> u64 {
    ALLOC_COUNTER.with(Cell::get)
}

fn alloc_bytes() -> u64 {
    ALLOC_BYTES.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        increment_alloc_counter(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// Input generators mirrored 1:1 from `verter_bench::css_identities`.
/// `verter_compiler` tests cannot depend on `verter_bench`; byte-identity of
/// the generated CSS is the contract recorded in
/// `docs/arch/refactor/rev11/evidence/J1/generator-mirror-equivalence.md`.
mod style_planner_gen {
    pub fn generate_class_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(".class-{i} {{ color: red; padding: {i}px; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_descendant_selectors(n: usize) -> String {
        (0..n)
            .map(|i| format!(".parent-{i} .child-{i} {{ color: blue; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_pseudo_selectors(n: usize) -> String {
        let pseudos = [":hover", ":focus", ":active", ":first-child", ":last-child"];
        (0..n)
            .map(|i| {
                let pseudo = pseudos[i % pseudos.len()];
                format!(".btn-{i}{pseudo} {{ color: red; }}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_selector_lists(n: usize) -> String {
        (0..n)
            .map(|i| {
                let selectors = (0..3)
                    .map(|j| format!(".sel-{i}-{j}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{selectors} {{ margin: {i}px; }}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_v_bind_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(".item-{i} {{ color: v-bind(color{i}); }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_v_bind_dotted(n: usize) -> String {
        (0..n)
            .map(|i| format!(".item-{i} {{ color: v-bind('theme.colors.primary{i}'); }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_deep_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":deep(.inner-{i}) {{ color: red; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_slotted_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":slotted(.slot-{i}) {{ color: red; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_mixed_vue(n: usize) -> String {
        (0..n)
            .map(|i| match i % 3 {
                0 => format!(".item-{i} {{ color: v-bind(color{i}); }}"),
                1 => format!(":deep(.inner-{i}) {{ padding: {i}px; }}"),
                _ => format!(":slotted(.slot-{i}) {{ margin: {i}px; }}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_global_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":global(.reset-{i}) {{ margin: 0; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn generate_repeated_classes(unique: usize, repeats: usize) -> String {
        let mut rules = Vec::new();
        for r in 0..repeats {
            for i in 0..unique {
                rules.push(format!(".btn-{i} {{ padding: {r}px; }}"));
            }
        }
        rules.join("\n")
    }

    pub const N: usize = 50;

    pub fn all_categories() -> [(&'static str, String); 11] {
        [
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
}

mod legacy_process_style_allocation_baseline {
    //! One canary per `css_bench.rs` generator category, each driving the
    //! generated CSS through the `css::process_style` entry point with a
    //! fixed representative option set (`scoped: true`, `is_module: false`)
    //! so counts are comparable across categories and, later, against the
    //! `style_planner` pipeline. `scoped: true` is used uniformly (rather
    //! than mirroring each generator's own bench group's options) because
    //! the ratified bound compares ALLOCATION COUNT PER CATEGORY across the
    //! two pipelines, not per exact option permutation — a fixed option set
    //! removes that as a confound.

    use verter_compiler::css::{process_style, ProcessStyleOptions};

    use super::style_planner_gen::*;
    use super::{alloc_count, reset_alloc_counter};

    pub(super) fn measure(css: &str) -> u64 {
        let options = ProcessStyleOptions {
            scope_id: "a4f2eed6",
            scoped: true,
            is_module: false,
            module_name: None,
            filename: None,
            sourcemap: false,
        };
        // Warm any one-time lazy initialisation before the measured call.
        let _ = process_style(css, &options).unwrap();
        reset_alloc_counter();
        let result = process_style(css, &options).unwrap();
        let count = alloc_count();
        std::hint::black_box(&result);
        count
    }

    macro_rules! canary {
        ($name:ident, $marker:literal, $css:expr) => {
            #[test]
            fn $name() {
                let css = $css;
                let count = measure(&css);
                eprintln!("J1_LEGACY_ALLOC[{}] = {count}", $marker);
                assert!(
                    count > 0,
                    "baseline sanity: `{}` must observe non-zero allocations \
                     through legacy css::process_style",
                    $marker
                );
            }
        };
    }

    canary!(class_rules, "class_rules", generate_class_rules(N));
    canary!(
        descendant_selectors,
        "descendant_selectors",
        generate_descendant_selectors(N)
    );
    canary!(
        pseudo_selectors,
        "pseudo_selectors",
        generate_pseudo_selectors(N)
    );
    canary!(selector_lists, "selector_lists", generate_selector_lists(N));
    canary!(v_bind_rules, "v_bind_rules", generate_v_bind_rules(N));
    canary!(v_bind_dotted, "v_bind_dotted", generate_v_bind_dotted(N));
    canary!(deep_rules, "deep_rules", generate_deep_rules(N));
    canary!(slotted_rules, "slotted_rules", generate_slotted_rules(N));
    canary!(mixed_vue, "mixed_vue", generate_mixed_vue(N));
    canary!(global_rules, "global_rules", generate_global_rules(N));
    canary!(
        repeated_classes,
        "repeated_classes",
        generate_repeated_classes(5, 10)
    );
}

mod style_planner_allocation_baseline {
    //! Counterpart to `legacy_process_style_allocation_baseline`.
    //!
    //! Same generated CSS inputs (the shared `super::style_planner_gen`
    //! module), driven through `style_planner::run_vue_style_cascade` — the
    //! same cascade entry `compile/mod.rs` and `vue_bridge.rs` call — instead
    //! of chaining the per-stage transform functions by hand.
    //!
    //! Calling the cascade entry (rather than `transform_vue_v_bind` then
    //! `transform_vue_scoped_css` as two independent calls) matters for
    //! allocation-count fidelity: `run_vue_style_cascade` hands the same
    //! already-parsed `StyleSyntaxIr` forward when a stage returns
    //! `Unchanged`, so a real invocation pays `1 + K` `parse_style_ir` calls
    //! (K = stages that actually change bytes), never a flat 2 regardless of
    //! whether v-bind touched anything. `module: false, scoped: true` mirrors
    //! the lightningcss canary's fixed `is_module: false, scoped: true`.
    //!
    //! Per-category non-zero sanity. The 1.2x ratio is
    //! `converged_style_pipeline_allocation_within_ratified_ceiling`.

    use verter_compiler::style_planner::{run_vue_style_cascade, AuthoredStyleInput};
    use verter_css_syntax::CssDialect;

    use super::style_planner_gen::*;
    use super::{alloc_count, reset_alloc_counter};

    const SCOPE_ID: &str = "a4f2eed6";

    fn run_pipeline(css: &str) {
        let input = AuthoredStyleInput::new(
            css,
            CssDialect::Css,
            "<style>",
            "standalone:carrier",
            "standalone:carrier-bytes",
        );
        let outcome = run_vue_style_cascade(input, SCOPE_ID, false, true, false);
        std::hint::black_box(&outcome.code);
    }

    pub(super) fn measure(css: &str) -> u64 {
        run_pipeline(css);
        reset_alloc_counter();
        run_pipeline(css);
        alloc_count()
    }

    macro_rules! canary {
        ($name:ident, $marker:literal, $css:expr) => {
            #[test]
            fn $name() {
                let css = $css;
                let count = measure(&css);
                eprintln!("J1_STYLE_PLANNER_ALLOC[{}] = {count}", $marker);
                assert!(
                    count > 0,
                    "baseline sanity: `{}` must observe non-zero allocations \
                     through style_planner::run_vue_style_cascade",
                    $marker
                );
            }
        };
    }

    canary!(class_rules, "class_rules", generate_class_rules(N));
    canary!(
        descendant_selectors,
        "descendant_selectors",
        generate_descendant_selectors(N)
    );
    canary!(
        pseudo_selectors,
        "pseudo_selectors",
        generate_pseudo_selectors(N)
    );
    canary!(selector_lists, "selector_lists", generate_selector_lists(N));
    canary!(v_bind_rules, "v_bind_rules", generate_v_bind_rules(N));
    canary!(v_bind_dotted, "v_bind_dotted", generate_v_bind_dotted(N));
    canary!(deep_rules, "deep_rules", generate_deep_rules(N));
    canary!(slotted_rules, "slotted_rules", generate_slotted_rules(N));
    canary!(mixed_vue, "mixed_vue", generate_mixed_vue(N));
    canary!(global_rules, "global_rules", generate_global_rules(N));
    canary!(
        repeated_classes,
        "repeated_classes",
        generate_repeated_classes(5, 10)
    );

    pub(super) fn measure_with_source_map(css: &str, want_source_map: bool) -> u64 {
        let run = |css: &str| {
            let input = AuthoredStyleInput::new(
                css,
                CssDialect::Css,
                "<style>",
                "standalone:carrier",
                "standalone:carrier-bytes",
            );
            let outcome = run_vue_style_cascade(input, SCOPE_ID, false, true, want_source_map);
            std::hint::black_box(&outcome.code);
        };
        run(css);
        reset_alloc_counter();
        run(css);
        alloc_count()
    }
}

/// Dual-pipeline instrument: both pipelines measured in one process against
/// the same generated CSS. Asserts each half is non-zero. The 1.2x ratio is
/// `converged_style_pipeline_allocation_within_ratified_ceiling`.
mod dual_pipeline_allocation_instrument {
    use super::legacy_process_style_allocation_baseline as lightningcss;
    use super::style_planner_allocation_baseline as planner;
    use super::style_planner_gen::all_categories;

    #[test]
    fn each_category_observes_both_pipelines() {
        for (name, css) in all_categories() {
            let lightningcss_count = lightningcss::measure(&css);
            let planner_count = planner::measure(&css);
            eprintln!(
                "J1_ALLOC_BOTH[{name}] lightningcss={lightningcss_count} planner={planner_count}"
            );
            assert!(
                lightningcss_count > 0,
                "{name}: lightningcss pipeline must allocate"
            );
            assert!(
                planner_count > 0,
                "{name}: style_planner pipeline must allocate"
            );
        }
    }

    #[test]
    fn source_map_off_allocates_strictly_less_than_on() {
        let css = super::style_planner_gen::generate_class_rules(super::style_planner_gen::N);
        let off = planner::measure_with_source_map(&css, false);
        let on = planner::measure_with_source_map(&css, true);
        eprintln!("J1_SOURCE_MAP_ALLOC off={off} on={on}");
        assert!(off > 0 && on > 0, "both source-map modes must allocate");
        assert!(
            off < on,
            "a caller that does not want a source map must not pay \
             generate_map/to_json_string (off={off}, on={on})"
        );
    }
}

/// 1.2x allocation ceiling: converged `style_planner` counts versus the
/// recaptured legacy `css::process_style` counts for the same generator
/// category. Category universe is `style_planner_gen::all_categories` (the
/// same 11 generators `css_identities::allocation_category_universe`
/// registers). Comparison runs only after the retained set, the measured
/// set, and that universe are the same set.
mod allocation_ceiling {
    use std::collections::{BTreeMap, BTreeSet};

    use super::legacy_process_style_allocation_baseline as legacy;
    use super::style_planner_allocation_baseline as converged;
    use super::style_planner_gen::all_categories;

    /// Wall-clock / allocation ceiling: candidate <= 1.2x baseline.
    const CEILING_NUM: u128 = 12;
    const CEILING_DEN: u128 = 10;

    /// Legacy `css::process_style` allocation counts recaptured on tree
    /// `1548e5b23d199fd9c761d952f50b4ecb4d5888bb` with `--test-threads=1`
    /// through `dual_pipeline_allocation_instrument::each_category_observes_both_pipelines`.
    /// Live counts from that tree, not a copied historical table.
    const RETAINED_LEGACY_ALLOC: &[(&str, u64)] = &[
        ("class_rules", 422),
        ("descendant_selectors", 371),
        ("pseudo_selectors", 371),
        ("selector_lists", 822),
        ("v_bind_rules", 929),
        ("v_bind_dotted", 929),
        ("deep_rules", 522),
        ("slotted_rules", 472),
        ("mixed_vue", 648),
        ("global_rules", 370),
        ("repeated_classes", 371),
    ];

    fn check_allocation_ceiling(
        universe: &[&str],
        retained_legacy: &BTreeMap<&str, u64>,
        measured_converged: &BTreeMap<&str, u64>,
    ) -> Result<(), Vec<String>> {
        let mut failures = Vec::new();
        let universe_set: BTreeSet<&str> = universe.iter().copied().collect();
        let retained_set: BTreeSet<&str> = retained_legacy.keys().copied().collect();
        let measured_set: BTreeSet<&str> = measured_converged.keys().copied().collect();

        if universe_set.len() != universe.len() {
            failures.push("universe contains duplicate categories".to_string());
        }
        if retained_set.len() != retained_legacy.len() {
            failures.push("retained legacy table contains duplicate categories".to_string());
        }
        if measured_set.len() != measured_converged.len() {
            failures.push("measured set contains duplicate categories".to_string());
        }

        for missing in universe_set.difference(&retained_set) {
            failures.push(format!(
                "retained legacy table is missing category {missing:?} from the universe"
            ));
        }
        for extra in retained_set.difference(&universe_set) {
            failures.push(format!(
                "retained legacy table has extra category {extra:?} not in the universe"
            ));
        }
        for missing in universe_set.difference(&measured_set) {
            failures.push(format!(
                "measured set is missing category {missing:?} from the universe"
            ));
        }
        for extra in measured_set.difference(&universe_set) {
            failures.push(format!(
                "measured set has extra category {extra:?} not in the universe"
            ));
        }
        if !failures.is_empty() {
            return Err(failures);
        }

        for name in universe {
            let base = u128::from(*retained_legacy.get(name).expect("set-equal"));
            let cand = u128::from(*measured_converged.get(name).expect("set-equal"));
            if cand * CEILING_DEN > base * CEILING_NUM {
                failures.push(format!(
                    "category {name:?} exceeds the 1.2x allocation ceiling: converged \
                     {cand} vs legacy {base} ({:.3}x)",
                    cand as f64 / base.max(1) as f64,
                ));
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures)
        }
    }

    fn universe_names() -> Vec<&'static str> {
        all_categories().into_iter().map(|(name, _)| name).collect()
    }

    fn retained_map() -> BTreeMap<&'static str, u64> {
        RETAINED_LEGACY_ALLOC.iter().copied().collect()
    }

    #[test]
    fn allocation_ceiling_passes_when_sets_match_and_ratios_under_bound() {
        let universe = universe_names();
        let retained = retained_map();
        let measured: BTreeMap<&str, u64> = retained
            .iter()
            .map(|(k, v)| (*k, v.saturating_add(v / 10))) // 1.1x
            .collect();
        check_allocation_ceiling(&universe, &retained, &measured)
            .expect("1.1x of retained counts is under the ceiling");
    }

    #[test]
    fn allocation_ceiling_fails_on_missing_category() {
        let universe = universe_names();
        let retained = retained_map();
        let mut measured = retained.clone();
        measured.remove("class_rules");
        let err = check_allocation_ceiling(&universe, &retained, &measured)
            .expect_err("missing category must refuse");
        assert!(
            err.iter()
                .any(|e| e.contains("missing") && e.contains("class_rules")),
            "refusal names the missing category: {err:?}"
        );
    }

    #[test]
    fn allocation_ceiling_fails_on_extra_category() {
        let universe = universe_names();
        let retained = retained_map();
        let mut measured = retained.clone();
        measured.insert("fabricated", 1);
        let err = check_allocation_ceiling(&universe, &retained, &measured)
            .expect_err("extra category must refuse");
        assert!(
            err.iter()
                .any(|e| e.contains("extra") && e.contains("fabricated")),
            "refusal names the extra category: {err:?}"
        );
    }

    #[test]
    fn allocation_ceiling_fails_when_exactly_one_category_exceeds_naming_only_it() {
        let universe = universe_names();
        let retained = retained_map();
        let mut measured = retained.clone();
        let base = retained["class_rules"];
        measured.insert("class_rules", (base * 12) / 10 + 1); // just over 1.2x
        let err = check_allocation_ceiling(&universe, &retained, &measured)
            .expect_err("one category over the ceiling must fail");
        let ceiling: Vec<&String> = err.iter().filter(|e| e.contains("ceiling")).collect();
        assert_eq!(
            ceiling.len(),
            1,
            "exactly the one exceeding category reddens: {err:?}"
        );
        assert!(
            ceiling[0].contains("class_rules"),
            "the exceeding category is named: {err:?}"
        );
    }

    #[test]
    fn allocation_ceiling_passes_at_exactly_the_ceiling() {
        let universe = universe_names();
        let retained = retained_map();
        let mut measured = retained.clone();
        let base = retained["class_rules"];
        measured.insert("class_rules", (base * 12) / 10); // exactly 1.2x
        check_allocation_ceiling(&universe, &retained, &measured)
            .expect("exactly 1.2x is within the ceiling");
    }

    #[test]
    fn retained_legacy_allocation_matches_live_legacy_pipeline() {
        let retained = retained_map();
        let mut live = BTreeMap::new();
        for (name, css) in all_categories() {
            live.insert(name, legacy::measure(&css));
        }
        assert_eq!(
            live, retained,
            "live legacy counts must equal the recapture committed in this file \
             — a copied donor table that drifted from this tree fails here"
        );
    }

    #[test]
    fn converged_style_pipeline_allocation_within_ratified_ceiling() {
        let universe = universe_names();
        let retained = retained_map();
        let mut measured = BTreeMap::new();
        for (name, css) in all_categories() {
            let count = converged::measure(&css);
            eprintln!(
                "J1_ALLOC_RATIO[{name}] = converged {count} / legacy {} = {:.3}x",
                retained[name],
                count as f64 / retained[name].max(1) as f64
            );
            measured.insert(name, count);
        }
        check_allocation_ceiling(&universe, &retained, &measured).unwrap_or_else(|failures| {
            panic!(
                "converged style pipeline exceeds the recaptured 1.2x allocation ceiling:\n  {}",
                failures.join("\n  ")
            );
        });
    }
}

/// Copy A of the CSS generator mirror: `style_planner_gen` must produce
/// byte-identical output to the pinned digest table (Copy B is
/// `verter_bench::css_identities`).
mod generator_mirror {
    use std::collections::BTreeSet;

    use sha2::{Digest, Sha256};

    use super::style_planner_gen::{
        generate_class_rules, generate_deep_rules, generate_descendant_selectors,
        generate_global_rules, generate_mixed_vue, generate_pseudo_selectors,
        generate_repeated_classes, generate_selector_lists, generate_slotted_rules,
        generate_v_bind_dotted, generate_v_bind_rules,
    };

    const MIRROR_TABLE_JSON: &str =
        include_str!("../../../docs/arch/refactor/rev11/evidence/J1/generator-mirror-digests.json");

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    fn copy_a_digest_table() -> std::collections::BTreeMap<String, String> {
        let mut table = std::collections::BTreeMap::new();
        const SIZES: [usize; 7] = [1, 5, 8, 20, 40, 50, 100];
        const AXES: [usize; 6] = [1, 5, 10, 20, 50, 100];
        type OneArgGenerator = fn(usize) -> String;
        let ones: [(&str, OneArgGenerator); 10] = [
            ("generate_class_rules", generate_class_rules),
            (
                "generate_descendant_selectors",
                generate_descendant_selectors,
            ),
            ("generate_pseudo_selectors", generate_pseudo_selectors),
            ("generate_selector_lists", generate_selector_lists),
            ("generate_v_bind_rules", generate_v_bind_rules),
            ("generate_v_bind_dotted", generate_v_bind_dotted),
            ("generate_deep_rules", generate_deep_rules),
            ("generate_slotted_rules", generate_slotted_rules),
            ("generate_mixed_vue", generate_mixed_vue),
            ("generate_global_rules", generate_global_rules),
        ];
        for (name, gen) in ones {
            for n in SIZES {
                table.insert(format!("{name}:{n}"), sha256_hex(gen(n).as_bytes()));
            }
        }
        for unique in AXES {
            for repeats in AXES {
                table.insert(
                    format!("generate_repeated_classes:{unique}x{repeats}"),
                    sha256_hex(generate_repeated_classes(unique, repeats).as_bytes()),
                );
            }
        }
        table
    }

    #[test]
    fn allocator_canary_generators_match_pinned_mirror_digests() {
        let pinned: serde_json::Value =
            serde_json::from_str(MIRROR_TABLE_JSON).expect("mirror table parses");
        let expected = pinned["digests"]
            .as_object()
            .expect("digests object")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().expect("hex digest").to_string()))
            .collect::<std::collections::BTreeMap<_, _>>();
        let actual = copy_a_digest_table();
        let expected_keys: BTreeSet<&str> = expected.keys().map(String::as_str).collect();
        let actual_keys: BTreeSet<&str> = actual.keys().map(String::as_str).collect();
        assert_eq!(
            actual_keys, expected_keys,
            "Copy A generator digest keys must be exactly the pinned set"
        );
        for (key, digest) in &expected {
            assert_eq!(
                actual.get(key).map(String::as_str),
                Some(digest.as_str()),
                "Copy A digest mismatch at {key}"
            );
        }
    }

    #[test]
    fn generator_mirror_control_class_rules_differs_from_deep_rules() {
        let left = generate_class_rules(1);
        let right = generate_deep_rules(1);
        assert_ne!(left.as_bytes(), right.as_bytes());
        let offset = left
            .as_bytes()
            .iter()
            .zip(right.as_bytes())
            .position(|(a, b)| a != b)
            .unwrap_or(left.len().min(right.len()));
        assert_eq!(offset, 0, "control must differ at byte 0, got {offset}");
        assert!(left.starts_with(".class-0 { color: red; padding: 0px; }"));
        assert!(right.starts_with(":deep(.inner-0) { color: red; }"));
    }
}

mod svelte_css_analysis_fact_reread_allocation_probe {
    //! The shared `verter_css_syntax` parser decides a compound's grammar-gap
    //! classification and an at-rule's prelude text ONCE, at parse time, and
    //! stores each as a plain struct field; every later reader
    //! (`match_relsel`/`render`) reads that field instead of re-deriving it
    //! from raw source bytes. This canary proves the observable half of that
    //! claim directly: re-reading the SAME facts a second time, through the
    //! exact accessors production code uses, allocates nothing.

    use verter_compiler::svelte::runtime::{
        analyze_style_body_for_alloc_probe, reread_cached_css_facts_for_alloc_probe,
    };
    use verter_span::Span;

    use super::{alloc_count, reset_alloc_counter};

    fn style_body_span(source: &str) -> Span {
        let start = source.find("<style>").expect("open tag") + "<style>".len();
        let end = source.rfind("</style>").expect("close tag");
        Span::new(start as u32, end as u32)
    }

    // SCOPE NOTE: this canary measures HEAP ALLOCATIONS only. It proves the
    // absence of a reconstruction that ALLOCATES (re-decoding into a new
    // `String`, rebuilding a `Vec`, re-parsing into a fresh tree) — it
    // cannot see a hidden reconstruction that re-scans or re-derives a fact
    // from `source: &str` WITHOUT allocating (slicing an existing `&str` by
    // span, or a byte/codepoint comparison loop, touch no allocator). A
    // structural "read the parser's stored field, don't rescan raw bytes"
    // guarantee is a DIFFERENT property than "the reread allocates nothing",
    // and this test only proves the second.
    #[test]
    fn cached_compound_tail_and_prelude_text_reread_without_allocating() {
        // Exercises every fact shape at once: a plain compound (`.card`), an
        // unclaimed-tail compound that is unused but structurally ordinary
        // (`.dead`), and two `@keyframes` at-rules whose prelude text is the
        // parser's own decoded reconstruction.
        let source = "<div class=\"card\"><p class=\"title\">x</p></div>\n<style>\
            @keyframes spin { from { opacity: 0; } to { opacity: 1; } }\n\
            @keyframes spin-two { from { opacity: 0; } }\n\
            .card, .dead, .title { color: red; }\
            </style>";
        let analyzed = analyze_style_body_for_alloc_probe(source, style_body_span(source));

        // Warm-up: the FIRST re-read, right after analysis, still exercises
        // whatever the accessors do (a plain struct-field read) — recorded
        // but not asserted on, since the interesting claim is that a SECOND
        // re-read does not allocate MORE than the first.
        reset_alloc_counter();
        reread_cached_css_facts_for_alloc_probe(source, &analyzed);
        let first_reread = alloc_count();
        eprintln!("J1_ALLOC[css_fact_reread_first] = {first_reread}");

        reset_alloc_counter();
        reread_cached_css_facts_for_alloc_probe(source, &analyzed);
        let second_reread = alloc_count();
        eprintln!("J1_ALLOC[css_fact_reread_second] = {second_reread}");

        assert_eq!(
            second_reread, 0,
            "re-reading a compound-tail / at-rule-prelude fact must allocate nothing \
             — an ALLOCATING hidden reconstruction pass (re-decoding the prelude text \
             into a new String, rebuilding a Vec, or re-parsing) would allocate here; \
             an allocation-free rescan of `source` would NOT be caught by this canary \
             (see the scope note above)"
        );
        assert_eq!(
            first_reread, 0,
            "the FIRST re-read (the facts were already decided at parse time, before \
             analysis even ran) must also allocate nothing — proving the parser's own \
             stored fields, not lazy-on-first-read memoization, back these accessors"
        );
    }
}

/// Intra-parser attribution for `parse_style_ir` / `StyleSyntaxIr` construction.
///
/// Splits the `parse:initial` bucket into the caller's `Arc::from(code)` admission
/// copy, a no-op event sink (lexer + parser only), selector-list clone, and the
/// remaining IR-sink work. Prints one row per `css_bench.rs` generator category
/// so the numbers compose with the sibling arena-lifecycle block. Assertions
/// pin the *shape* of the split (admission cannot explain hundreds of calls);
/// they do not freeze a ratio ceiling.
mod intra_parser_attribution {
    use std::cell::{Cell, RefCell};
    use std::sync::Arc;

    use verter_css_syntax::{
        parse_style_ir, parse_with_sink, set_style_ir_parse_phase_probe, CssDialect, CssEntryPoint,
        CssParseMode, CssSource, CssStructureTooLarge, ParseEvent, ParseEventSink, SelectorList,
    };

    /// Restores whatever probe was installed before, even if the measured parse panics — a
    /// leaked function pointer would silently attribute a LATER parse on this thread.
    struct PhaseProbeGuard(Option<fn(&'static str)>);

    impl PhaseProbeGuard {
        fn install(probe: fn(&'static str)) -> Self {
            Self(set_style_ir_parse_phase_probe(Some(probe)))
        }
    }

    impl Drop for PhaseProbeGuard {
        fn drop(&mut self) {
            set_style_ir_parse_phase_probe(self.0);
        }
    }

    use super::{alloc_bytes, alloc_count, reset_alloc_counter};
    // One record per probe marker. Both selector-list ownership transfers (a rule's own list
    // and every functional pseudo's argument list) mark, so a 200-rule generator marks ~400
    // times. Overflow only truncates `parse_emit` attribution — the clone bucket is a Cell —
    // but size for the largest generator anyway.
    const PHASE_CAP: usize = 2048;

    struct NoopSink;

    impl ParseEventSink for NoopSink {
        fn event(&mut self, _event: ParseEvent) -> Result<(), CssStructureTooLarge> {
            Ok(())
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct Totals {
        calls: u64,
        bytes: u64,
    }

    impl Totals {
        fn capture() -> Self {
            Self {
                calls: alloc_count(),
                bytes: alloc_bytes(),
            }
        }
    }

    struct PhaseLog {
        names: [&'static str; PHASE_CAP],
        calls: [u64; PHASE_CAP],
        bytes: [u64; PHASE_CAP],
        len: usize,
        last: Totals,
    }

    impl PhaseLog {
        const fn new() -> Self {
            Self {
                names: [""; PHASE_CAP],
                calls: [0; PHASE_CAP],
                bytes: [0; PHASE_CAP],
                len: 0,
                last: Totals { calls: 0, bytes: 0 },
            }
        }

        fn reset(&mut self, start: Totals) {
            self.len = 0;
            self.last = start;
        }

        fn record(&mut self, phase: &'static str, now: Totals) {
            assert!(
                self.len < PHASE_CAP,
                "phase log overflowed PHASE_CAP={PHASE_CAP}: attribution columns would be \
                 silently truncated while the printed totals still looked plausible"
            );
            self.names[self.len] = phase;
            self.calls[self.len] = now.calls.saturating_sub(self.last.calls);
            self.bytes[self.len] = now.bytes.saturating_sub(self.last.bytes);
            self.last = now;
            self.len += 1;
        }

        fn delta(&self, name: &'static str) -> Totals {
            let mut calls: u64 = 0;
            let mut bytes: u64 = 0;
            for i in 0..self.len {
                if self.names[i] == name {
                    calls = calls.saturating_add(self.calls[i]);
                    bytes = bytes.saturating_add(self.bytes[i]);
                }
            }
            Totals { calls, bytes }
        }
    }

    thread_local! {
        static PHASE_LOG: RefCell<PhaseLog> = const { RefCell::new(PhaseLog::new()) };
        static CLONE_MARK: Cell<Totals> = const { Cell::new(Totals { calls: 0, bytes: 0 }) };
        static CLONE_TOTAL: Cell<Totals> = const { Cell::new(Totals { calls: 0, bytes: 0 }) };
    }

    fn on_phase(phase: &'static str) {
        let now = Totals::capture();
        if phase == "selector_clone_enter" {
            // Fold parse work since the last marker into parse_emit so the
            // clone sandwich does not drop those allocations from the log.
            PHASE_LOG.with(|log| log.borrow_mut().record("after_parse_emit", now));
            CLONE_MARK.with(|mark| mark.set(now));
            return;
        }
        if phase == "selector_clone_exit" {
            let before = CLONE_MARK.with(Cell::get);
            CLONE_TOTAL.with(|total| {
                let so_far = total.get();
                total.set(Totals {
                    calls: so_far
                        .calls
                        .saturating_add(now.calls.saturating_sub(before.calls)),
                    bytes: so_far
                        .bytes
                        .saturating_add(now.bytes.saturating_sub(before.bytes)),
                });
            });
            PHASE_LOG.with(|log| log.borrow_mut().last = now);
            return;
        }
        PHASE_LOG.with(|log| log.borrow_mut().record(phase, now));
    }

    fn measure_admission(css: &str) -> Totals {
        reset_alloc_counter();
        let owned = Arc::<str>::from(css);
        std::hint::black_box(&owned);
        Totals::capture()
    }

    fn measure_source_wrap(css: &str) -> Totals {
        let owned = Arc::<str>::from(css);
        reset_alloc_counter();
        let source = CssSource::new(owned, 0).unwrap();
        std::hint::black_box(&source);
        Totals::capture()
    }

    fn measure_parser_noop(css: &str) -> Totals {
        let source = CssSource::new(Arc::from(css), 0).unwrap();
        let mut sink = NoopSink;
        reset_alloc_counter();
        parse_with_sink(
            &source,
            CssDialect::Css,
            CssEntryPoint::Stylesheet,
            CssParseMode::Recover,
            &mut sink,
        )
        .unwrap();
        std::hint::black_box(&sink);
        Totals::capture()
    }

    struct IrSplit {
        total: Totals,
        sink_new: Totals,
        parse_emit: Totals,
        selector_clone: Totals,
        finish: Totals,
    }

    fn measure_parse_style_ir(css: &str) -> IrSplit {
        let source = CssSource::new(Arc::from(css), 0).unwrap();
        let guard = PhaseProbeGuard::install(on_phase);
        reset_alloc_counter();
        CLONE_TOTAL.with(|total| total.set(Totals { calls: 0, bytes: 0 }));
        PHASE_LOG.with(|log| log.borrow_mut().reset(Totals::capture()));
        let ir = parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap();
        drop(guard);
        std::hint::black_box(&ir);
        let selector_clone = CLONE_TOTAL.with(Cell::get);
        PHASE_LOG.with(|log| {
            let log = log.borrow();
            IrSplit {
                total: Totals::capture(),
                sink_new: log.delta("after_sink_new"),
                parse_emit: log.delta("after_parse_emit"),
                selector_clone,
                finish: log.delta("after_finish"),
            }
        })
    }

    fn attribute(css: &str) -> (Totals, Totals, Totals, IrSplit) {
        let source = CssSource::new(Arc::from(css), 0).unwrap();
        let _ = parse_style_ir(source, CssDialect::Css, CssParseMode::Recover).unwrap();
        (
            measure_admission(css),
            measure_source_wrap(css),
            measure_parser_noop(css),
            measure_parse_style_ir(css),
        )
    }

    use super::style_planner_gen::{
        all_categories, generate_class_rules, generate_deep_rules, generate_selector_lists, N,
    };

    #[test]
    fn admission_copy_cannot_explain_parse_initial() {
        for (name, css) in all_categories() {
            let (admission, source_wrap, parser_noop, ir) = attribute(&css);
            eprintln!(
                "PARSE_ALLOC[{name}] admission={}/{} source_wrap={}/{} parser_noop={}/{} \
                 ir_total={}/{} sink_new={}/{} parse_emit={}/{} selector_clone={}/{} \
                 finish={}/{} source_len={}",
                admission.calls,
                admission.bytes,
                source_wrap.calls,
                source_wrap.bytes,
                parser_noop.calls,
                parser_noop.bytes,
                ir.total.calls,
                ir.total.bytes,
                ir.sink_new.calls,
                ir.sink_new.bytes,
                ir.parse_emit.calls,
                ir.parse_emit.bytes,
                ir.selector_clone.calls,
                ir.selector_clone.bytes,
                ir.finish.calls,
                ir.finish.bytes,
                css.len()
            );
            // CONTROL. Both bound `Arc::<str>::from(&str)` — a std behaviour, not a verter
            // one. No change to this crate or `verter_css_syntax` can move them; they exist so
            // the admission column below is read as a measured fact rather than an assumption.
            assert!(
                admission.calls <= 4,
                "{name}: Arc::from admission must be a handful of calls, got {}",
                admission.calls
            );
            assert!(
                admission.bytes as usize >= css.len(),
                "{name}: admission must copy the source bytes ({} < {})",
                admission.bytes,
                css.len()
            );
            assert_eq!(
                source_wrap.calls, 0,
                "{name}: CssSource::new must not heap-allocate after Arc ownership is taken"
            );
            assert!(
                ir.total.calls > 50,
                "{name}: parse_style_ir of the 50-rule generator must be tens/hundreds of calls, got {}",
                ir.total.calls
            );
            // CONTROL. `admission.calls <= 4` and `ir.total.calls > 50` above already imply
            // this; it cannot fail while they pass. Kept because it states the split's headline
            // claim in the form the ruling asked the question — one admission copy cannot
            // explain hundreds of parse allocations — not because it discriminates.
            assert!(
                ir.total.calls > admission.calls.saturating_mul(10),
                "{name}: IR construction ({}) must dwarf the admission copy ({})",
                ir.total.calls,
                admission.calls
            );
            // CONTROL. `parser_noop` measures zero in every category — the lexer and
            // recursive-descent parser allocate nothing — so this reduces to `> 0`, already
            // implied by the `> 50` bound above. It is retained to state the split's premise
            // (all parse heap is IR construction), not because it discriminates.
            assert!(
                ir.total.calls > parser_noop.calls,
                "{name}: StyleSyntaxIr construction ({}) must allocate beyond lexer+parser ({})",
                ir.total.calls,
                parser_noop.calls
            );
            // Covers BOTH bracketed ownership transfers: a rule's own selector list (a plain
            // move, zero allocations) and every functional pseudo's nested argument list (one
            // `Box<SelectorList>`, nothing else). Asserting the bucket's EXACT cost rather than
            // a zero call count is what makes a clone anywhere inside either bracket visible: a
            // clone allocates the list's interior `Vec`s, whose sizes are not the box's.
            let admissible_bytes = ir
                .selector_clone
                .calls
                .saturating_mul(std::mem::size_of::<SelectorList>() as u64);
            assert_eq!(
                ir.selector_clone.bytes,
                admissible_bytes,
                "{name}: a selector-list transfer may allocate only its own box \
                 (got {} calls / {} bytes; {} boxes would be {} bytes)",
                ir.selector_clone.calls,
                ir.selector_clone.bytes,
                ir.selector_clone.calls,
                admissible_bytes
            );
            // Phase columns must account for the whole parse. Without this the harness can
            // under-report a phase (a dropped marker, a mis-folded bracket) while `ir_total`
            // still prints a plausible number, and the evidence table would be quietly wrong.
            let attributed = ir
                .sink_new
                .calls
                .saturating_add(ir.parse_emit.calls)
                .saturating_add(ir.selector_clone.calls)
                .saturating_add(ir.finish.calls);
            assert_eq!(
                attributed,
                ir.total.calls,
                "{name}: phase columns must sum to the parse total \
                 (sink_new {} + parse_emit {} + selector_clone {} + finish {} != {})",
                ir.sink_new.calls,
                ir.parse_emit.calls,
                ir.selector_clone.calls,
                ir.finish.calls,
                ir.total.calls
            );
        }
    }

    /// Per-rule buffers must not be sized from the WHOLE stylesheet.
    ///
    /// Each generator emits N structurally identical rules, so correct per-rule cost is a
    /// CONSTANT and total requested bytes are linear in N. A buffer sized from the whole source
    /// makes per-rule cost grow with N. The bound is therefore on per-rule cost holding steady,
    /// not on a total-growth ratio: a growth ratio only bounds the pre-size CONSTANT, so a
    /// small enough divisor slips under it while still being whole-source sizing —
    /// `Vec::with_capacity(source.len() / 32)` grows totals 5.5x on 4x source and passes a 6x
    /// bound. Comparing per-rule cost across an 8x spread removes that escape: any whole-source
    /// term is 8x larger per rule at the top, whatever its divisor.
    #[test]
    fn per_rule_buffers_are_not_sized_from_the_whole_source() {
        for (name, small_n, large_n, generate) in [
            (
                "class_rules",
                N,
                N * 8,
                generate_class_rules as fn(usize) -> String,
            ),
            ("deep_rules", N, N * 8, generate_deep_rules),
            ("selector_lists", N, N * 8, generate_selector_lists),
        ] {
            let small = generate(small_n);
            let large = generate(large_n);

            // Warm any one-time lazy state so it is not charged to the first measurement.
            let warm = CssSource::new(Arc::from(small.as_str()), 0).unwrap();
            let _ = parse_style_ir(warm, CssDialect::Css, CssParseMode::Recover).unwrap();

            let small_bytes = measure_parse_style_ir(&small).total.bytes;
            let large_bytes = measure_parse_style_ir(&large).total.bytes;
            let small_per_rule = small_bytes as f64 / small_n as f64;
            let large_per_rule = large_bytes as f64 / large_n as f64;
            let per_rule_growth = large_per_rule / small_per_rule;
            eprintln!(
                "PARSE_SCALE[{name}] small={small_bytes}B/{}rules={small_per_rule:.1} \
                 large={large_bytes}B/{large_n}rules={large_per_rule:.1} \
                 per_rule_growth={per_rule_growth:.3}",
                small_n
            );
            assert!(
                large.len() > small.len() * 7,
                "{name}: the large generator must actually be ~8x the source \
                 ({} vs {})",
                large.len(),
                small.len()
            );
            assert!(
                per_rule_growth < 1.10,
                "{name}: per-rule requested bytes must not grow with stylesheet size \
                 ({small_per_rule:.1} at {small_n} rules -> {large_per_rule:.1} at {large_n} \
                 rules, {per_rule_growth:.3}x)"
            );
        }
    }
}

mod slotted_occurrence_arena_bytes {
    //! Requested-byte / scaling regression for the per-`:slotted()` bump-arena
    //! floor. Bytes are the measured quantity, not calls: one
    //! `Allocator::new()` + first `alloc_str` is a handful of calls wrapping a
    //! ~16KiB first chunk, so the call-count delta is a proxy whose size
    //! depends on how many occurrences an input carries, while the byte
    //! counter measures the imposed per-occurrence cost directly.
    //!
    //! Isolation: `:deep(...)` only renders its argument's raw source slice
    //! and never collects argument edits, so subtracting `:deep` bytes from
    //! `:slotted` bytes at the same N attributes the extra to the slotted
    //! rewrite plus a small parser-side residual — the two generators differ
    //! in source length and token content, so parsing does NOT subtract out
    //! exactly (see `parser_only_residual_between_deep_and_slotted_generators`
    //! for the quantified residual). The extra must scale far below one
    //! bump-chunk per additional occurrence.

    use verter_compiler::style_planner::{run_vue_style_cascade, AuthoredStyleInput};
    use verter_css_syntax::CssDialect;

    use super::style_planner_gen::{
        generate_deep_rules, generate_mixed_vue, generate_slotted_rules,
    };
    use super::{alloc_bytes, alloc_count, reset_alloc_counter};

    const SCOPE_ID: &str = "a4f2eed6";
    const SMALL_N: usize = 8;
    const LARGE_N: usize = 40;
    /// One oxc bump first-chunk is 16KiB, so an occurrence-local arena costs
    /// ~22.7KB of requested bytes per occurrence (chunk + two 64-entry chunk
    /// Vecs). The measured per-occurrence extra of the outer-edit rewrite is
    /// 628 bytes (single argument edit); 2KiB gives ~3x measurement headroom
    /// while sitting ~11x below the arena floor, so a reintroduced
    /// per-occurrence arena cannot pass.
    const MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE: u64 = 2048;
    /// Multi-edit cap: an `:is()` two-arm argument contributes two inserts
    /// plus the prefix/suffix deletes per occurrence; measured extra is
    /// 1,971 bytes per occurrence. 4KiB is ~2x headroom and ~5.5x below the
    /// ~22.7KB occurrence-local arena floor.
    const MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE: u64 = 4096;
    /// Three-edit cap: an `:is()` three-arm argument contributes three
    /// inserts plus the prefix/suffix deletes per occurrence; measured extra
    /// is 2,589 bytes per occurrence. 6KiB is ~2.4x headroom and ~3.7x below
    /// the ~22.7KB occurrence-local arena floor, so a regression gated on
    /// more than two argument edits cannot pass.
    const MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE: u64 = 6144;
    const ATTRIBUTION_N: usize = 50;

    /// Multi-edit variant of the css_bench slotted generator: every
    /// occurrence's argument is an `:is()` with two arms, so scoping fans out
    /// to TWO absolute-offset inserts per occurrence. A regression that
    /// minted an occurrence-local arena only when an argument carries more
    /// than one edit is invisible to the single-edit generators above; this
    /// shape catches it.
    fn generate_multi_edit_slotted_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":slotted(:is(.a-{i}, .b-{i})) {{ color: red; }}"))
            .collect()
    }

    /// `:deep()` baseline with byte-for-byte the same argument shape as
    /// [`generate_multi_edit_slotted_rules`]: `:deep(...)` only reads its
    /// argument's raw slice and collects no argument edits, so subtracting it
    /// isolates the slotted rewrite from parsing the `:is()` argument.
    fn generate_multi_edit_deep_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":deep(:is(.a-{i}, .b-{i})) {{ color: red; }}"))
            .collect()
    }

    /// Three-edit variant: every occurrence's argument is an `:is()` with
    /// THREE arms, so scoping fans out to three absolute-offset inserts per
    /// occurrence. A regression gated on more than two argument edits is
    /// invisible to both the single-edit and two-edit generators above; this
    /// shape catches it. That the shape really produces three edits is pinned
    /// by the output-shape controls in the planner's direct-result tests
    /// (`three_edit_slotted_argument_scopes_each_is_arm` and the
    /// `many_three_edit_*` build-count test, which use this exact rule text).
    fn generate_three_edit_slotted_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":slotted(:is(.a-{i}, .b-{i}, .c-{i})) {{ color: red; }}"))
            .collect()
    }

    /// `:deep()` baseline with byte-for-byte the same argument shape as
    /// [`generate_three_edit_slotted_rules`], for the same isolation reason
    /// as [`generate_multi_edit_deep_rules`].
    fn generate_three_edit_deep_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":deep(:is(.a-{i}, .b-{i}, .c-{i})) {{ color: red; }}"))
            .collect()
    }

    struct Measured {
        count: u64,
        bytes: u64,
    }

    fn measure_converged(css: &str) -> Measured {
        let input = AuthoredStyleInput::new(
            css,
            CssDialect::Css,
            "probe.css",
            "space:probe",
            "artifact:probe",
        );
        // Warm any one-time lazy initialisation before the measured call.
        let _ = run_vue_style_cascade(input, SCOPE_ID, false, true, true);
        reset_alloc_counter();
        let outcome = run_vue_style_cascade(input, SCOPE_ID, false, true, true);
        std::hint::black_box(&outcome);
        Measured {
            count: alloc_count(),
            bytes: alloc_bytes(),
        }
    }

    #[test]
    fn slotted_occurrence_extra_bytes_scale_below_bump_chunk() {
        let slotted_small = measure_converged(&generate_slotted_rules(SMALL_N));
        let deep_small = measure_converged(&generate_deep_rules(SMALL_N));
        let slotted_large = measure_converged(&generate_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_deep_rules(LARGE_N));

        let extra_small = slotted_small.bytes.saturating_sub(deep_small.bytes);
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        let extra_delta = extra_large.saturating_sub(extra_small);
        let extra_n = (LARGE_N - SMALL_N) as u64;
        let extra_per = extra_delta / extra_n;

        eprintln!(
            "J1_SLOTTED_ARENA[small n={SMALL_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_small.bytes, deep_small.bytes, extra_small
        );
        eprintln!(
            "J1_SLOTTED_ARENA[large n={LARGE_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_large.bytes, deep_large.bytes, extra_large
        );
        eprintln!(
            "J1_SLOTTED_ARENA[per extra occurrence] bytes={extra_per} (cap {MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE})"
        );

        assert!(
            extra_per < MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE,
            "per-occurrence slotted-minus-deep requested bytes must stay below one \
             oxc bump first-chunk ({MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE}); observed {extra_per} \
             (({extra_large} - {extra_small}) / {extra_n}). A ~16KiB-scale floor means \
             the slotted rewrite is minting a fresh occurrence-local Allocator again."
        );
    }

    #[test]
    fn slotted_occurrence_absolute_extra_bytes_below_cap() {
        let slotted_large = measure_converged(&generate_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_deep_rules(LARGE_N));
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        eprintln!(
            "J1_SLOTTED_ARENA[absolute n={LARGE_N}] extra={extra_large} cap={}",
            LARGE_N as u64 * MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE
        );
        assert!(
            extra_large < LARGE_N as u64 * MAX_EXTRA_BYTES_PER_SLOTTED_OCCURRENCE,
            "absolute slotted-minus-deep extra at n={LARGE_N} is {extra_large}, \
             which is still a per-occurrence arena floor"
        );
    }

    #[test]
    fn multi_edit_slotted_occurrence_extra_bytes_scale_below_bump_chunk() {
        let slotted_small = measure_converged(&generate_multi_edit_slotted_rules(SMALL_N));
        let deep_small = measure_converged(&generate_multi_edit_deep_rules(SMALL_N));
        let slotted_large = measure_converged(&generate_multi_edit_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_multi_edit_deep_rules(LARGE_N));

        let extra_small = slotted_small.bytes.saturating_sub(deep_small.bytes);
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        let extra_delta = extra_large.saturating_sub(extra_small);
        let extra_n = (LARGE_N - SMALL_N) as u64;
        let extra_per = extra_delta / extra_n;

        eprintln!(
            "J1_SLOTTED_ARENA_MULTI[small n={SMALL_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_small.bytes, deep_small.bytes, extra_small
        );
        eprintln!(
            "J1_SLOTTED_ARENA_MULTI[large n={LARGE_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_large.bytes, deep_large.bytes, extra_large
        );
        eprintln!(
            "J1_SLOTTED_ARENA_MULTI[per extra occurrence] bytes={extra_per} (cap {MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE})"
        );

        assert!(
            extra_per < MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE,
            "per-occurrence multi-edit slotted-minus-deep requested bytes must stay \
             below the cap ({MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE}); observed \
             {extra_per} (({extra_large} - {extra_small}) / {extra_n}). A ~16KiB-scale floor \
             means a fresh occurrence-local Allocator is back — on the multi-argument-edit path"
        );
    }

    #[test]
    fn multi_edit_slotted_occurrence_absolute_extra_bytes_below_cap() {
        let slotted_large = measure_converged(&generate_multi_edit_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_multi_edit_deep_rules(LARGE_N));
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        eprintln!(
            "J1_SLOTTED_ARENA_MULTI[absolute n={LARGE_N}] extra={extra_large} cap={}",
            LARGE_N as u64 * MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE
        );
        assert!(
            extra_large < LARGE_N as u64 * MAX_EXTRA_BYTES_PER_MULTI_EDIT_SLOTTED_OCCURRENCE,
            "absolute multi-edit slotted-minus-deep extra at n={LARGE_N} is \
             {extra_large}, which is still a per-occurrence arena floor"
        );
    }

    #[test]
    fn three_edit_slotted_occurrence_extra_bytes_scale_below_bump_chunk() {
        let slotted_small = measure_converged(&generate_three_edit_slotted_rules(SMALL_N));
        let deep_small = measure_converged(&generate_three_edit_deep_rules(SMALL_N));
        let slotted_large = measure_converged(&generate_three_edit_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_three_edit_deep_rules(LARGE_N));

        let extra_small = slotted_small.bytes.saturating_sub(deep_small.bytes);
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        let extra_delta = extra_large.saturating_sub(extra_small);
        let extra_n = (LARGE_N - SMALL_N) as u64;
        let extra_per = extra_delta / extra_n;

        eprintln!(
            "J1_SLOTTED_ARENA_THREE[small n={SMALL_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_small.bytes, deep_small.bytes, extra_small
        );
        eprintln!(
            "J1_SLOTTED_ARENA_THREE[large n={LARGE_N}] slotted_bytes={} deep_bytes={} extra={}",
            slotted_large.bytes, deep_large.bytes, extra_large
        );
        eprintln!(
            "J1_SLOTTED_ARENA_THREE[per extra occurrence] bytes={extra_per} (cap {MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE})"
        );

        assert!(
            extra_per < MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE,
            "per-occurrence three-edit slotted-minus-deep requested bytes must stay \
             below the cap ({MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE}); observed \
             {extra_per} (({extra_large} - {extra_small}) / {extra_n}). A ~16KiB-scale floor \
             means a fresh occurrence-local Allocator is back — on a path gated on more \
             than two argument edits"
        );
    }

    #[test]
    fn three_edit_slotted_occurrence_absolute_extra_bytes_below_cap() {
        let slotted_large = measure_converged(&generate_three_edit_slotted_rules(LARGE_N));
        let deep_large = measure_converged(&generate_three_edit_deep_rules(LARGE_N));
        let extra_large = slotted_large.bytes.saturating_sub(deep_large.bytes);
        eprintln!(
            "J1_SLOTTED_ARENA_THREE[absolute n={LARGE_N}] extra={extra_large} cap={}",
            LARGE_N as u64 * MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE
        );
        assert!(
            extra_large < LARGE_N as u64 * MAX_EXTRA_BYTES_PER_THREE_EDIT_SLOTTED_OCCURRENCE,
            "absolute three-edit slotted-minus-deep extra at n={LARGE_N} is \
             {extra_large}, which is still a per-occurrence arena floor"
        );
    }

    /// Declared measurement control, not coverage: quantifies the parser-side
    /// residual between the `:deep` and `:slotted` generator arms. The two
    /// generators differ in source length and token content, so CSS parsing
    /// does NOT subtract out exactly in the isolation figures above — this
    /// prints the residual by running the cascade with `scoped=false`
    /// (parse + v-bind scan only, no selector rewriting in either arm).
    #[test]
    fn parser_only_residual_between_deep_and_slotted_generators() {
        fn measure_parse_only(css: &str) -> u64 {
            let input = AuthoredStyleInput::new(
                css,
                CssDialect::Css,
                "probe.css",
                "space:probe",
                "artifact:probe",
            );
            let _ = run_vue_style_cascade(input, SCOPE_ID, false, false, false);
            reset_alloc_counter();
            let outcome = run_vue_style_cascade(input, SCOPE_ID, false, false, false);
            std::hint::black_box(&outcome);
            alloc_bytes()
        }

        let deep = measure_parse_only(&generate_deep_rules(ATTRIBUTION_N));
        let slotted = measure_parse_only(&generate_slotted_rules(ATTRIBUTION_N));
        eprintln!(
            "J1_PARSER_RESIDUAL[n={ATTRIBUTION_N}] deep_bytes={deep} slotted_bytes={slotted} \
             residual={}",
            slotted.abs_diff(deep)
        );
        assert!(
            deep > 0 && slotted > 0,
            "control: the parse-only cascade must allocate at all"
        );
    }

    #[test]
    fn slotted_occurrence_byte_measurement_is_live() {
        let slotted_small = measure_converged(&generate_slotted_rules(SMALL_N));
        let slotted_large = measure_converged(&generate_slotted_rules(LARGE_N));
        assert!(
            slotted_large.bytes > slotted_small.bytes,
            "control: larger slotted input must request more bytes than the smaller one \
             (slotted_large={} slotted_small={})",
            slotted_large.bytes,
            slotted_small.bytes
        );
        assert!(
            slotted_small.count > 0 && slotted_large.count > 0,
            "control: the converged cascade must allocate at all"
        );
    }

    #[test]
    fn attribution_slotted_rules_and_mixed_vue_totals() {
        let slotted = measure_converged(&generate_slotted_rules(ATTRIBUTION_N));
        let mixed = measure_converged(&generate_mixed_vue(ATTRIBUTION_N));
        eprintln!(
            "J1_ATTRIBUTION[slotted_rules n={ATTRIBUTION_N}] calls={} bytes={}",
            slotted.count, slotted.bytes
        );
        eprintln!(
            "J1_ATTRIBUTION[mixed_vue n={ATTRIBUTION_N}] calls={} bytes={}",
            mixed.count, mixed.bytes
        );
        assert!(
            slotted.bytes > 0 && mixed.bytes > 0,
            "control: attribution totals must observe a real cascade"
        );
    }
}
