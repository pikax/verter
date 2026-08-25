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

    const N: usize = 50;
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

    // The generators live in the sibling module and are private. Mirror the
    // 11-category set here so this module can name them without widening the
    // legacy canary's visibility.
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

    fn all_categories() -> [(&'static str, String); 11] {
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
