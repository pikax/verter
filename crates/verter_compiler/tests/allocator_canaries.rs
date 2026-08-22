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
//! # Legacy-path allocation baseline
//!
//! This module records the ALLOCATION half of the ratified Latency/Allocation
//! bound: live allocations through the current legacy `css::process_style`
//! entry point, one count per `crates/verter_bench/benches/css_bench.rs`
//! generator category (the same input generators the wall-clock baseline in
//! `docs/arch/refactor/rev11/evidence/J1/perf-baseline.md` measures). The
//! numbers this binary prints (`eprintln!` markers, `cargo test -- --nocapture`)
//! are the allocation baseline recorded in that same document; the converged
//! style pipeline that replaces this entry point is required to stay within
//! the same 1.2x (20%) ceiling per category.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;

thread_local! {
    static ALLOC_COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn increment_alloc_counter() {
    // Allocation can occur while a thread is tearing down TLS. Do not turn
    // an otherwise valid allocation into a panic if this key is no longer
    // accessible.
    let _ = ALLOC_COUNTER.try_with(|counter| counter.set(counter.get().wrapping_add(1)));
}

fn reset_alloc_counter() {
    ALLOC_COUNTER.with(|counter| counter.set(0));
}

fn alloc_count() -> u64 {
    ALLOC_COUNTER.with(Cell::get)
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter();
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        increment_alloc_counter();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        increment_alloc_counter();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

mod legacy_process_style_allocation_baseline {
    //! One canary per `css_bench.rs` generator category, each driving the
    //! generated CSS through the legacy `css::process_style` entry point
    //! with a fixed representative option set (`scoped: true`,
    //! `is_module: false`) so counts are comparable across categories and,
    //! later, against the converged style pipeline this legacy path is
    //! replaced by. `scoped: true` is used uniformly (rather than mirroring
    //! each generator's own bench group's options) because the ratified
    //! bound compares ALLOCATION COUNT PER CATEGORY across the two
    //! pipelines, not per exact option permutation — a fixed option set
    //! removes that as a confound.

    use verter_compiler::css::{process_style, ProcessStyleOptions};

    use super::{alloc_count, reset_alloc_counter};

    // ---- Generators mirrored 1:1 from css_bench.rs (kept in lock-step; the
    // benchmark file and this canary must measure the same inputs). ----

    fn generate_class_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(".class-{i} {{ color: red; padding: {i}px; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_descendant_selectors(n: usize) -> String {
        (0..n)
            .map(|i| format!(".parent-{i} .child-{i} {{ color: blue; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_pseudo_selectors(n: usize) -> String {
        let pseudos = [":hover", ":focus", ":active", ":first-child", ":last-child"];
        (0..n)
            .map(|i| {
                let pseudo = pseudos[i % pseudos.len()];
                format!(".btn-{i}{pseudo} {{ color: red; }}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_selector_lists(n: usize) -> String {
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

    fn generate_v_bind_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(".item-{i} {{ color: v-bind(color{i}); }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_v_bind_dotted(n: usize) -> String {
        (0..n)
            .map(|i| format!(".item-{i} {{ color: v-bind('theme.colors.primary{i}'); }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_deep_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":deep(.inner-{i}) {{ color: red; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_slotted_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":slotted(.slot-{i}) {{ color: red; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_mixed_vue(n: usize) -> String {
        (0..n)
            .map(|i| match i % 3 {
                0 => format!(".item-{i} {{ color: v-bind(color{i}); }}"),
                1 => format!(":deep(.inner-{i}) {{ padding: {i}px; }}"),
                _ => format!(":slotted(.slot-{i}) {{ margin: {i}px; }}"),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_global_rules(n: usize) -> String {
        (0..n)
            .map(|i| format!(":global(.reset-{i}) {{ margin: 0; }}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn generate_repeated_classes(unique: usize, repeats: usize) -> String {
        let mut rules = Vec::new();
        for r in 0..repeats {
            for i in 0..unique {
                rules.push(format!(".btn-{i} {{ padding: {r}px; }}"));
            }
        }
        rules.join("\n")
    }

    const N: usize = 50;

    fn measure(css: &str) -> u64 {
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
