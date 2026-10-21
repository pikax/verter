//! What one key costs when a mapped type's `as` clause renames every key
//! of an imported interface through a recursive template conditional.
//!
//! `Kebab<S>` splits a key at its first `_`, recurses on the rest and joins
//! the parts with `-`, so a key of `n` underscores evaluates the
//! conditional `n + 1` times. Each key's renaming is independent of every
//! other key's: what the mapping costs per key must be one small constant
//! however many keys the interface holds, in allocations, in the semantic
//! queries it builds cold and in the substitutions it walks.

use std::sync::Arc;

use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

use super::{alloc_count, peak_live_bytes, reset_alloc_counter};

const MODEL: &str = "/remap/model.ts";
const SCENARIO: &str = "/remap/scenario.ts";
const SCENARIO_SOURCE: &str = "import type { Model } from \"./model\";\n\
type Kebab<S extends string> = S extends `${infer A}_${infer B}` ? `${A}-${Kebab<B>}` : S;\n\
declare const __v: { [K in keyof Model as Kebab<K & string>]: Model[K] };\n\
type __Probe = typeof __v;\n";

/// What one cold `__Probe` demand cost on this thread.
#[derive(Debug, Clone, Copy)]
struct ColdRun {
    allocations: u64,
    peak_live_bytes: u64,
    cold_builds: u64,
    substitutions: u64,
}

impl ColdRun {
    fn counters(self) -> [(&'static str, u64); 4] {
        [
            ("allocations", self.allocations),
            ("peak live bytes", self.peak_live_bytes),
            ("cold builds", self.cold_builds),
            ("substitutions", self.substitutions),
        ]
    }
}

/// Answer `__Probe` cold over a `Model` keyed by `keys`, member `i` typed
/// `i`, and check the checker's answer: every key renamed with each `_`
/// a `-`, its value kept.
fn cold_run(keys: &[String]) -> ColdRun {
    let mut model = String::from("export interface Model {");
    for (i, key) in keys.iter().enumerate() {
        model.push_str(&format!(" {key}: {i};"));
    }
    model.push_str(" }\n");
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    workspace.inject_file(MODEL.to_string(), Arc::from(model.as_str()));
    workspace.inject_file(SCENARIO.to_string(), Arc::from(SCENARIO_SOURCE));
    let workspace: Arc<dyn WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    for (path, source) in [(MODEL, model.as_str()), (SCENARIO, SCENARIO_SOURCE)] {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(path.to_string()),
            input_id: path.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: vec![],
        });
    }
    let graph = host.project_type_store().semantic_graph();
    let before = graph.stats_snapshot();
    reset_alloc_counter();
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
        .into_parts();
    let allocations = alloc_count();
    let peak_live_bytes = u64::try_from(peak_live_bytes()).unwrap_or(0);
    let after = graph.stats_snapshot();
    let run = ColdRun {
        allocations,
        peak_live_bytes,
        cold_builds: after.misses - before.misses,
        substitutions: after.substitute_memo_misses - before.substitute_memo_misses,
    };

    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let wire = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    let answer = verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr");
    let verter_type_expr::TypeExpr::Object(object) = &answer else {
        panic!("{} keys: the remap answers an object", keys.len());
    };
    let mut renamed: Vec<(String, String)> = object
        .properties
        .iter()
        .map(|member| match member {
            verter_type_expr::ObjectMember::Property(property) => {
                let verter_type_expr::AuthoredPropertyKey::String(name) = &property.key else {
                    panic!("a property keyed by {:?}", property.key);
                };
                let value = verter_type_expr::render_type_expr_display(&property.ty)
                    .expect("the value renders")
                    .text;
                (name.to_string(), value)
            }
            other => panic!("a property, got {other:?}"),
        })
        .collect();
    renamed.sort();
    let mut expected: Vec<(String, String)> = keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.replace('_', "-"), i.to_string()))
        .collect();
    expected.sort();
    assert_eq!(renamed, expected, "every key renamed, its value kept");
    run
}

/// Keys of the benchmark's shape: three underscores, four evaluations of
/// the conditional each.
fn benchmark_keys(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("field_{i}_name_x")).collect()
}

/// A key the template does not match keeps its name; a key of one
/// underscore or of several is split at each.
#[test]
fn key_remap_renames_unmatched_single_and_repeated_separators() {
    let keys: Vec<String> = (0..40)
        .map(|i| match i % 4 {
            0 => format!("plain{i}"),
            1 => format!("one_{i}"),
            2 => format!("field_{i}_name_x"),
            _ => format!("a_b_c_d_{i}_e"),
        })
        .collect();
    cold_run(&keys);
}

/// Each further key costs one flat constant — the same between 25 and 50
/// keys as between 50 and 100 — and that constant stays small: no more
/// than the allocations, cold semantic builds and substitution walks one
/// key's four conditional evaluations need.
#[test]
fn key_remap_cost_per_key_is_a_small_flat_constant() {
    let sizes = [25usize, 50, 100];
    let runs = sizes.map(|count| cold_run(&benchmark_keys(count)));
    let per_key = |from: usize, to: usize, what: usize| {
        let before = runs[from].counters()[what].1;
        let after = runs[to].counters()[what].1;
        after.saturating_sub(before) as f64 / (sizes[to] - sizes[from]) as f64
    };
    // Measured per further key: about 1,060 allocations, 74 KB live at
    // the peak, 8 cold builds and 10 substitution walks (the levels below
    // `name_x` are shared by every key). A level that relates each capture
    // to its implied `string` constraint as a query, relates the key again
    // to the pattern instantiated with its captures, or records one
    // substitution twice exceeds these.
    let ceilings = [1_250.0, 88_000.0, 9.0, 13.0];
    for (what, ceiling) in ceilings.into_iter().enumerate() {
        let name = runs[0].counters()[what].0;
        let lower = per_key(0, 1, what);
        let upper = per_key(1, 2, what);
        eprintln!("KEY_REMAP {name} per key: {lower:.1} from 25 to 50, {upper:.1} from 50 to 100");
        assert!(
            upper <= lower * 1.25 + 1.0 && lower <= upper * 1.25 + 1.0,
            "{name}: each further key costs the same however many keys came before \
             ({lower:.1} from 25 to 50, {upper:.1} from 50 to 100): {runs:?}"
        );
        assert!(
            upper <= ceiling,
            "{name}: a key costs at most {ceiling} ({upper:.1} from 50 to 100): {runs:?}"
        );
    }
}
