//! Static architecture guards for the **Declaration Merging (CRITICAL)** rule.
//!
//! Same-name declaration merge is produced ONLY by
//! `verter_semantic::type_eval` ordered declaration groups: `EvalEnv` appends
//! contributors (no last-wins map / no overwrite `insert` for mergeable kinds),
//! and a merged interface lowers to a distinct `SemanticNodeData::MergedDecl`
//! carrier that the peer-merge reducer walks — NEVER a bare
//! `TypeExpr::Intersection` (whose reducer applies heritage-shadow semantics
//! and cannot accumulate method overload groups).
//!
//! These scanners are static source greps (no cargo invocation); each is
//! discriminating — it FAILS if the pre-merge last-wins shape or the
//! intersection-collapse front is reintroduced.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    fs::read_to_string(workspace_root().join(rel)).unwrap_or_else(|e| panic!("read `{rel}`: {e}"))
}

fn walk_rs(path: &PathBuf, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// (i) `EvalEnv` must expose its symbol tables as ordered contributor GROUPS,
/// never a last-wins `FxHashMap<String, TypeDeclInfo>` / `…ValueDeclInfo>` map.
#[test]
fn eval_env_type_symbols_are_grouped_not_last_wins_map() {
    let src = read("crates/verter_semantic/src/analysis/type_eval.rs");
    assert!(
        src.contains("pub type_symbols: FxHashMap<String, TypeDeclGroup>"),
        "EvalEnv.type_symbols must be keyed to TypeDeclGroup (ordered contributors)"
    );
    assert!(
        src.contains("pub value_symbols: FxHashMap<String, ValueDeclGroup>"),
        "EvalEnv.value_symbols must be keyed to ValueDeclGroup (ordered contributors)"
    );
    // The last-wins map shape is forbidden.
    assert!(
        !src.contains("type_symbols: FxHashMap<String, TypeDeclInfo>"),
        "EvalEnv.type_symbols must NOT be a last-wins FxHashMap<String, TypeDeclInfo>"
    );
    assert!(
        !src.contains("value_symbols: FxHashMap<String, ValueDeclInfo>"),
        "EvalEnv.value_symbols must NOT be a last-wins FxHashMap<String, ValueDeclInfo>"
    );
}

/// (ii) `add_type` / `add_value` must APPEND contributors (no bare overwrite
/// `insert` over an existing mergeable-kind name).
#[test]
fn eval_env_add_decl_appends_not_overwrites() {
    let src = read("crates/verter_semantic/src/analysis/type_eval.rs");
    let appends = src.matches("group.contributors.push(decl)").count();
    assert!(
        appends >= 2,
        "add_type and add_value must append contributors via \
         `group.contributors.push(decl)` (found {appends} occurrences; expected >= 2)"
    );
}

/// (iii) No `raw_body = TypeExpr::intersection(...)` declaration-merge synthesis
/// anywhere in `verter_session` — the merge is a distinct `MergedDecl` carrier,
/// never an intersection fabricated on the shallow symbol.
#[test]
fn no_intersection_merge_synthesis_in_verter_session() {
    let mut files = Vec::new();
    walk_rs(
        &workspace_root().join("crates/verter_session/src"),
        &mut files,
    );
    let mut hits = Vec::new();
    for file in files {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let squashed: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            if squashed.contains("raw_body=TypeExpr::intersection")
                || squashed.contains("raw_body=TypeExpr::Intersection")
            {
                hits.push(format!("{}:{}", file.display(), n + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "declaration merge must NOT be synthesised as an intersection on a \
         shallow symbol body — route it through the `MergedDecl` carrier + \
         peer-merge reducer. Offending sites:\n{}",
        hits.join("\n")
    );
}

/// (iv) The load-bearing decision: a multi-contributor merged interface lowers
/// to a distinct `SemanticNodeData::MergedDecl` carrier (the derefed
/// merged-contributor shape, `DerefedBodyShape::Merged`), NOT a pre-collapsed
/// intersection. The body source is the locator-shape build; the carrier is
/// interned there and preserved through substitution and the view projection.
#[test]
fn merged_decl_lowers_to_distinct_carrier_not_intersection() {
    let src = read("crates/verter_session/src/project_semantic_dispatch/locator_shape.rs");
    assert!(
        src.contains("DerefedBodyShape::Merged") && src.contains("SemanticNodeData::MergedDecl {"),
        "the locator-shape body build must lower the derefed merged-contributor \
         shape to a distinct `SemanticNodeData::MergedDecl` carrier"
    );
}
