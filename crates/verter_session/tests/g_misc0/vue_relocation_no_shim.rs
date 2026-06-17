//! LEGACY_GATE_SELF — the Vue relocation no-shim guard.
//!
//! The Vue resolution machinery (the former `typeinfo/adapters/vue/
//! {public_type.rs, surface.rs}` and the `store.rs` cache) lives in
//! `typeinfo/framework_surface/vue_exec.rs`. Two static invariants enforce
//! that the old location holds NO surviving resolution and NO re-export
//! alias — a single home, not a dual path:
//!
//!  * `vue_resolution_files_deleted` — the three relocated/retired source
//!    files (`public_type.rs`, `surface.rs`, `store.rs`) MUST NOT exist
//!    under `typeinfo/adapters/vue/`. Their contents live in
//!    `vue_exec.rs`; a surviving file is a dual path.
//!
//!  * `vue_mod_carries_no_reexport_shim` — `typeinfo/adapters/vue/mod.rs`
//!    MUST NOT re-export any relocated resolution name (`public_type`,
//!    `surface`, `store`, `VueMacroSurface`, the three
//!    `*_from_typeinfo_surface` normalizers, the retired store types).
//!    A re-export shim under `adapters::vue::*` would let callers keep
//!    the old import path alive — the relocation must re-point callers,
//!    not alias the old path.
//!
//! The retired store types (`VueShallowMetadataStore`, `VueMacroDtoKey`,
//! `VueMacroDtos`, `VueMacroDtosEntry`) are additionally pinned absent
//! from production source by `no_legacy_walker::RETIRED_SYMBOLS`.

use std::path::PathBuf;

/// Extract the source text of a `fn <name>(` body (its `{ … }` block),
/// brace-balanced, ignoring braces inside `"…"` string literals.
fn fn_body(src: &str, name: &str) -> String {
    let needle = format!("fn {name}(");
    let start = src
        .find(&needle)
        .unwrap_or_else(|| panic!("`fn {name}(` not found"));
    let open = src[start..]
        .find('{')
        .map(|o| start + o)
        .unwrap_or_else(|| panic!("opening brace for `{name}` not found"));
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut end = open;
    for (i, &b) in bytes.iter().enumerate().skip(open) {
        let c = b as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    src[open..end].to_string()
}

fn workspace_root() -> PathBuf {
    // tests/g_misc0/<file> → crate root is two `parent()` hops up from
    // tests/, the workspace root one more.
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crate")
        .to_path_buf()
}

fn vue_adapter_dir() -> PathBuf {
    workspace_root().join("crates/verter_session/src/typeinfo/adapters/vue")
}

#[test]
fn vue_resolution_files_deleted() {
    let dir = vue_adapter_dir();
    for retired in ["public_type.rs", "surface.rs", "store.rs"] {
        let path = dir.join(retired);
        assert!(
            !path.exists(),
            "relocated/retired Vue resolution file `{}` still exists — its contents \
             belong in typeinfo/framework_surface/vue_exec.rs (no dual path)",
            path.display()
        );
    }
}

#[test]
fn vue_mod_carries_no_reexport_shim() {
    let mod_rs = vue_adapter_dir().join("mod.rs");
    let text = std::fs::read_to_string(&mod_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", mod_rs.display()));

    // Strip doc / line comments so a historical mention in prose does not
    // trip the scan — only live `mod`/`use` declarations matter.
    let mut code = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        // Drop a trailing `// …` comment on a code line.
        let line = match line.split_once("//") {
            Some((head, _)) => head,
            None => line,
        };
        code.push_str(line);
        code.push('\n');
    }

    // A re-export of any relocated resolution name under `adapters::vue`
    // is forbidden. `mod public_type;` / `mod surface;` / `mod store;`
    // declarations are forbidden (the files are deleted); a `pub use` of
    // a relocated symbol is forbidden (no shim alias).
    for banned in [
        "mod public_type",
        "mod surface",
        "mod store",
        "public_type::",
        "surface::",
        "store::",
        "VueMacroSurface",
        "props_from_typeinfo_surface",
        "emits_from_typeinfo_surface",
        "slots_from_typeinfo_surface",
        "VueShallowMetadataStore",
        "VueMacroDtoKey",
        "VueMacroDtos",
    ] {
        assert!(
            !code.contains(banned),
            "typeinfo/adapters/vue/mod.rs re-exports / re-declares the relocated name \
             `{banned}` — the Vue resolution machinery lives in \
             typeinfo/framework_surface/vue_exec.rs with NO shim under adapters::vue"
        );
    }
}

/// The public-API entry dispatches through the framework registry's
/// component-API projector leg — NOT a hard Vue branch. Two invariants:
///
///  * `get_public_api_with_mode` consults `api_projector_for` (registry
///    dispatch by resolved adapter id). The host method stays the single
///    entry; classification + projector selection replace the Vue gate.
///
///  * `render_vue_public_api_legacy` (the projector leg's delegate) carries
///    NO `is_vue()` framework gate. The Vue-vs-not decision is made ONCE,
///    up-front, by the registry dispatch; a surviving gate in the body would
///    be a redundant second classification (a dual path).
#[test]
fn public_api_dispatches_through_registry_projector_no_vue_gate() {
    let pipeline =
        workspace_root().join("crates/verter_session/src/host_resolve/virtual_file_pipeline.rs");
    let src = std::fs::read_to_string(&pipeline)
        .unwrap_or_else(|e| panic!("read {}: {e}", pipeline.display()));

    let entry = fn_body(&src, "get_public_api_with_mode");
    assert!(
        entry.contains("api_projector_for"),
        "get_public_api_with_mode must dispatch through \
         framework_registry().api_projector_for(..) — registry dispatch, not a Vue branch"
    );
    assert!(
        !entry.contains("is_vue"),
        "get_public_api_with_mode must not consult is_vue() — classification routes \
         through the resolved adapter id, not a hard Vue gate"
    );

    let legacy = fn_body(&src, "render_vue_public_api_legacy");
    assert!(
        !legacy.contains("is_vue"),
        "render_vue_public_api_legacy must carry NO is_vue() gate — the framework \
         classification happens once at the registry-dispatch entry; a gate here is a \
         redundant second classification (dual path)"
    );
}

/// The framework-NEUTRAL executor body carries NO privileged-framework branch.
///
/// `is_vue()` (or any framework-specific identity test) inside the executor
/// module is a hardcoded privileged framework branch — the CRITICAL rule that
/// the neutral executor must decide framework behavior from REGISTRY DATA, not a
/// hardcoded framework literal. Any per-framework decision rides a descriptor
/// capability the executor reads (e.g. `supports_named_export_surfaces`) or the
/// adapter's own plan/normalize — never an `is_vue()` fork in the executor body.
#[test]
fn framework_surface_executor_body_carries_no_privileged_framework_branch() {
    let executor =
        workspace_root().join("crates/verter_session/src/typeinfo/framework_surface/executor.rs");
    let src = std::fs::read_to_string(&executor)
        .unwrap_or_else(|e| panic!("read {}: {e}", executor.display()));
    let stripped = strip_line_comments(&src);
    assert!(
        !stripped.contains("is_vue"),
        "the framework-surface executor module body must carry NO is_vue() (or any \
         framework-specific identity) branch — a per-framework decision rides a descriptor \
         capability the executor reads from registry data, not a hardcoded Vue literal"
    );
}

/// The neutral synth-injection selector derives its scratch fallback adapter id
/// from the REGISTRY, not a hardcoded `FrameworkAdapterId::vue()` literal.
///
/// `inject_component_default_into_shallow_state` is the framework-neutral
/// default-injection entry. A typeinfo-scratch canonical with no resolved
/// language must route to the synthesizing framework's leg — but that id is the
/// registry's unique synth-bearing adapter (`synthesizing_adapter_id`), never a
/// hardcoded `vue()` literal inside the neutral selector body. A surviving
/// literal is a Vue-specific privileged branch in neutral host code.
#[test]
fn neutral_default_injection_derives_scratch_adapter_from_registry() {
    let host_construction = workspace_root().join("crates/verter_session/src/host_construction.rs");
    let src = std::fs::read_to_string(&host_construction)
        .unwrap_or_else(|e| panic!("read {}: {e}", host_construction.display()));
    let body = fn_body(&src, "inject_component_default_into_shallow_state");
    let stripped = strip_line_comments(&body);
    assert!(
        !stripped.contains("FrameworkAdapterId::vue()"),
        "inject_component_default_into_shallow_state must NOT hardcode \
         FrameworkAdapterId::vue() for the scratch fallback — the synthesizing adapter id \
         is registry-derived (framework_registry().synthesizing_adapter_id())"
    );
    assert!(
        stripped.contains("synthesizing_adapter_id"),
        "the scratch fallback routes through the registry's synthesizing_adapter_id()"
    );
}

/// Strip `//`-line comments (NOT doc-prose) from Rust source so a guard scan
/// inspects executable code only. A privileged-framework branch is real code,
/// not a doc sentence that may legitimately NAME the rule it forbids.
fn strip_line_comments(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
