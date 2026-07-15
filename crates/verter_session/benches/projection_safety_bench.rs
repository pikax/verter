//! Projection-view performance and allocation coverage.
//!
//! The fixture is lowered once; timed iterations exercise only the canonical
//! locator projection primitive through its `test-support` driver.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use verter_session::projection_bench_support::{ProjectionBenchCase, ProjectionBenchHarness};
use verter_session::semantic_query::{
    ProjectionMode, ProjectionReductionContext, ResultCompleteness,
};
use verter_session::{HostConfig, LanguageRegistry, UpsertRequest, VerterHost};

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);

// The counter is observational only and does not alter allocation behavior.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwards the allocation contract unchanged to `System`.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` and `layout` came from this allocator's `System`
        // forwarding path.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwards the reallocation contract unchanged to `System`.
        let resized = unsafe { System.realloc(ptr, layout, new_size) };
        if !resized.is_null() {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        resized
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

const FIXTURE_CANONICAL: &str = "/projection-bench.ts";

fn mixed_nested_type(depth: usize) -> String {
    let mut ty = "string".to_string();
    for level in 0..depth {
        ty = match level % 3 {
            0 => format!("{{ child{level}: {ty} }}"),
            1 => format!("({ty})[]"),
            _ => format!("({ty}) | {{ alternate{level}: number }}"),
        };
    }
    ty
}

fn union_type(width: usize) -> String {
    (0..width)
        .map(|index| format!("\"u{index}\""))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn intersection_type(width: usize) -> String {
    (0..width)
        .map(|index| format!("{{ p{index}: string }}"))
        .collect::<Vec<_>>()
        .join(" & ")
}

fn fixture_source() -> String {
    format!(
        r#"
export type Terminal = string;
export type Depth4 = {};
export type Depth8 = {};
export type Union32 = {};
export type Union1000 = {};
export type Intersection32 = {};
export type Intersection1000 = {};
export type FullObject = {{
  required: {{ nested: string[] }};
  readonly optional?: number;
  method<T extends string = string>(value: T, optional?: T, ...rest: T[]): T[];
  (input: string, extra?: number): boolean;
  new <T>(input: T): {{ value: T }};
  [key: number]: string;
}};
export type FullFunction = <T extends {{ x: string }} = {{ x: string }}>(
  this: void,
  value: T,
  optional?: T,
  ...rest: T[]
) => T[];
export type SharedLeaf = {{ value: string[] }};
export type SharedDag = {{ a: SharedLeaf; b: SharedLeaf; c: SharedLeaf }};
export type Conditional = string extends string ? {{ yes: string }} : {{ no: number }};
export type Indexed = {{ x: {{ value: string }} }}["x"];
export type Mapped = {{ [K in "a" | "b"]: {{ key: K }} }};
export type ResolvedTarget<T = string> = {{ value: T }};
export type ResolvableBare = ResolvedTarget<number>;
export type UnresolvableBare = MissingTarget<{{
  dead: string extends number ? {{ never: true }} : {{ live: false }}
}}>;
export type ResolvableImport = import("./projection-bench-dep").Imported;
export type UnresolvableImport = import("./projection-bench-missing").Missing<{{
  dead: string extends number ? never : string
}}>;
"#,
        mixed_nested_type(4),
        mixed_nested_type(8),
        union_type(32),
        union_type(1000),
        intersection_type(32),
        intersection_type(1000),
    )
}

fn upsert_ts(host: &VerterHost, canonical_id: &str, source: String) {
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_language: LanguageRegistry::global()
                .classify_static(canonical_id)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("projection benchmark fixture must parse and index");
}

fn prepare_cases<'a>(
    harness: &ProjectionBenchHarness<'a>,
) -> BTreeMap<&'static str, ProjectionBenchCase> {
    [
        "Terminal",
        "Depth4",
        "Depth8",
        "Union32",
        "Union1000",
        "Intersection32",
        "Intersection1000",
        "FullObject",
        "FullFunction",
        "SharedDag",
        "Conditional",
        "Indexed",
        "Mapped",
        "ResolvableBare",
        "UnresolvableBare",
        "ResolvableImport",
        "UnresolvableImport",
    ]
    .into_iter()
    .map(|name| {
        let resolved_names: &[(&str, &str, &str)] = match name {
            "ResolvableBare" => &[("ResolvedTarget", FIXTURE_CANONICAL, "ResolvedTarget")],
            "SharedDag" => &[("SharedLeaf", FIXTURE_CANONICAL, "SharedLeaf")],
            _ => &[],
        };
        let case = harness
            .prepare_decl_with_resolved_names(FIXTURE_CANONICAL, name, resolved_names)
            .unwrap_or_else(|| panic!("benchmark declaration {name} must lower"));
        (name, case)
    })
    .collect()
}

fn assert_complete(
    outcome: (
        verter_session::semantic_query::SemanticNodeId,
        ResultCompleteness,
    ),
) {
    assert_eq!(
        outcome.1,
        ResultCompleteness::Complete,
        "finite benchmark fixtures must never hit an operational limit"
    );
    black_box(outcome.0);
}

fn projection_safety(c: &mut Criterion) {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        "/projection-bench-dep.ts",
        "export type Imported = { value: string };\n".to_string(),
    );
    upsert_ts(&host, FIXTURE_CANONICAL, fixture_source());
    let mut harness = ProjectionBenchHarness::new(&host);
    let cases = prepare_cases(&harness);
    let expanded = ProjectionReductionContext::published(ProjectionMode::Expanded);

    let terminal = cases.get("Terminal").expect("terminal case");
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    assert_complete(harness.project_fresh(terminal, expanded));
    let terminal_cold_allocations = ALLOCATION_CALLS.load(Ordering::Relaxed);
    ALLOCATION_CALLS.store(0, Ordering::Relaxed);
    assert_complete(harness.project_warm(terminal, expanded));
    let terminal_warm_allocations = ALLOCATION_CALLS.load(Ordering::Relaxed);
    eprintln!(
        "projection-allocation-snapshot terminal_fresh={terminal_cold_allocations} terminal_memo_hit={terminal_warm_allocations}"
    );

    let mut group = c.benchmark_group("projection_view");
    group.sample_size(30);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));

    group.bench_function("terminal/cold", |b| {
        b.iter(|| assert_complete(harness.project_cold(black_box(terminal), expanded)));
    });
    assert_complete(harness.project_cold(terminal, expanded));
    group.bench_function("terminal/memo_hit", |b| {
        b.iter(|| assert_complete(harness.project_warm(black_box(terminal), expanded)));
    });

    for (bench_name, case_name) in [
        ("depth/mixed_4", "Depth4"),
        ("depth/mixed_8", "Depth8"),
        ("breadth/union_32", "Union32"),
        ("breadth/union_1000", "Union1000"),
        ("breadth/intersection_32", "Intersection32"),
        ("breadth/intersection_1000", "Intersection1000"),
        ("child_groups/object", "FullObject"),
        ("child_groups/function", "FullFunction"),
        ("staged/conditional", "Conditional"),
        ("staged/indexed", "Indexed"),
        ("staged/mapped", "Mapped"),
        ("references/bare_resolved", "ResolvableBare"),
        ("references/bare_unresolved", "UnresolvableBare"),
        ("references/import_resolved", "ResolvableImport"),
        ("references/import_unresolved", "UnresolvableImport"),
    ] {
        let case = cases.get(case_name).expect("prepared benchmark case");
        group.bench_function(bench_name, |b| {
            b.iter(|| assert_complete(harness.project_cold(black_box(case), expanded)));
        });
    }

    let shared = cases.get("SharedDag").expect("shared DAG case");
    let shallow = ProjectionReductionContext::published(ProjectionMode::Shallow);
    let navigate = ProjectionReductionContext::published(ProjectionMode::Navigate);
    group.bench_function("shared_dag/context_split", |b| {
        b.iter(|| {
            assert_complete(harness.project_cold(black_box(shared), shallow));
            assert_complete(harness.project_warm(black_box(shared), navigate));
            assert_complete(harness.project_warm(black_box(shared), expanded));
        });
    });

    group.finish();
}

criterion_group!(benches, projection_safety);
criterion_main!(benches);
