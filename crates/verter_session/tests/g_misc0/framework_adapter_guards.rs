//! Static architecture guards for the framework-adapter substrate.
//!
//! These pin the CRITICAL rules the substrate lands with:
//!
//! - `framework_adapter_ctx_closed_surface` — `FrameworkAdapterCtx`
//!   exposes EXACTLY two `pub fn` (`carrier_for` / `script_facts_for`)
//!   and the module never resolves types, indexes a file, runs the
//!   project semantic dispatch, or reads a `StoreView`.
//! - `component_default_synth_parse_domain_only` — the synth ctx is
//!   parse-domain only: it never references the resolved-validation
//!   fact types.
//! - `script_fact_capture_is_syntax_only` — the syntax-capture half
//!   documents and enforces the syntax-only contract (no import
//!   resolution / capability reads in the capture surface).
//! - `framework_surface_wire_executor_validates_first` — the executor
//!   runs `validate_type_info_graph_request` BEFORE any registry lookup
//!   or selector resolution.
//!
//! Each guard is a discriminating source scan: it FAILS against a tree
//! that violates the rule (e.g. a third `pub fn` on the ctx, a resolver
//! token in the ctx module, a registry lookup hoisted above the
//! validator) and PASSES against the landed final-state tree. Companion
//! self-tests pin the detectors' discrimination.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crate")
        .to_path_buf()
}

fn read_src(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read `{}`: {err}", path.display()))
}

/// Count `pub fn` declarations inside the `impl<...> FrameworkAdapterCtx`
/// block, returning the method names. Uses a brace-balanced scan from the
/// impl header so nested closures / inner items do not confuse the count.
fn ctx_pub_methods(src: &str) -> Vec<String> {
    let mut methods = Vec::new();
    let Some(impl_start) = src.find("impl<'a> FrameworkAdapterCtx<'a>") else {
        panic!("ctx.rs must declare `impl<'a> FrameworkAdapterCtx<'a>`");
    };
    // Find the opening brace of the impl block.
    let after_header = &src[impl_start..];
    let Some(brace_rel) = after_header.find('{') else {
        panic!("impl block must have an opening brace");
    };
    let body = &after_header[brace_rel + 1..];

    let mut depth: i32 = 1;
    // Iterate char-by-char so a byte offset never lands mid-codepoint (the
    // ctx doc comments contain multibyte chars like `→`). The slice
    // `&body[i..]` is always taken at a char boundary `i`.
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth <= 0 {
            break;
        }
        // Only inspect declarations at impl-body depth (depth == 1).
        if depth == 1 {
            let rest = &body[i..];
            if let Some(after) = rest.strip_prefix("pub fn ") {
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    methods.push(name);
                }
            }
        }
    }
    methods
}

#[test]
fn framework_adapter_ctx_closed_surface() {
    let src = read_src("crates/verter_session/src/framework/ctx.rs");

    // (1) EXACTLY two pub methods on the ctx, named `carrier_for` and
    // `script_facts_for`. A third pub method (or a renamed one) fails.
    let methods = ctx_pub_methods(&src);
    assert_eq!(
        methods,
        vec!["carrier_for".to_string(), "script_facts_for".to_string()],
        "FrameworkAdapterCtx must expose EXACTLY two pub methods \
         (carrier_for, script_facts_for) — found {methods:?}. The ctx is a \
         closed surface: a new pub op would let an adapter reach past the \
         carrier / script-fact seam."
    );

    // (2) The ctx module's CODE must NOT reference the query-time resolver
    // / file indexing / project semantic dispatch. The adapter sees only
    // its typed parse carrier + resolved script facts. Doc/comment lines
    // are stripped — the rule text legitimately NAMES the banned ops in
    // prose ("never calls ProjectSemanticDispatch").
    let code = strip_comments(&src);
    for banned in CTX_BANNED_RESOLVER_TOKENS {
        assert!(
            !code.contains(banned),
            "ctx.rs code must not reference `{banned}` — the framework-adapter \
             ctx is a facts/carrier-only surface, never a resolver / store-view \
             entry point."
        );
    }
}

/// The resolver / file-indexing / store-view tokens forbidden in the
/// framework-adapter ctx CODE. The ctx exposes only the carrier + script-fact
/// seam; it must never reach the query-time resolver or a store view.
const CTX_BANNED_RESOLVER_TOKENS: &[&str] = &[
    "ProjectSemanticDispatch",
    "ensure_indexed_ready",
    "resolve_type(",
    "project_node_to_type_expr",
    "StoreView",
    "HostStoreView",
    "resolver_store_view",
    "current_store_view",
];

#[test]
fn ctx_banned_token_scan_discriminates() {
    // The scan must catch a synthetic ctx that reads a StoreView: a
    // violating body must contain at least one banned token.
    let violating = r#"
impl<'a> FrameworkAdapterCtx<'a> {
    pub fn carrier_for<T>(&self) {
        let _ = self.host.current_store_view_for_query();
    }
}
"#;
    let code = strip_comments(violating);
    assert!(
        CTX_BANNED_RESOLVER_TOKENS.iter().any(|t| code.contains(t)),
        "the banned-token scan must catch a ctx that reaches a store view — \
         the StoreView / current_store_view tokens close that evasion."
    );
    // And a clean carrier-only body trips nothing.
    let clean = r#"
impl<'a> FrameworkAdapterCtx<'a> {
    pub fn carrier_for<T>(&self) {
        let leg = self.registration.carrier.as_ref()?;
    }
}
"#;
    let clean_code = strip_comments(clean);
    assert!(
        !CTX_BANNED_RESOLVER_TOKENS
            .iter()
            .any(|t| clean_code.contains(t)),
        "the banned-token scan must NOT flag a clean carrier-only ctx body."
    );
}

/// Strip line comments (`//` / `///`) so a banned-token scan inspects only
/// executable source, never the rule text the doc comments legitimately
/// name. Block comments are not used for these tokens in the scanned files.
fn strip_comments(src: &str) -> String {
    src.lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ctx_closed_surface_detector_discriminates() {
    // The detector must reject a synthetic impl with a third pub fn.
    let synthetic = r#"
impl<'a> FrameworkAdapterCtx<'a> {
    pub fn carrier_for<T>(&self) {}
    pub fn script_facts_for<T>(&self) {}
    pub fn resolve_everything(&self) {}
}
"#;
    let methods = ctx_pub_methods(synthetic);
    assert_eq!(methods.len(), 3, "detector must see all three pub methods");
    assert!(
        methods.contains(&"resolve_everything".to_string()),
        "detector must surface the extra method so the guard fails on it"
    );
}

#[test]
fn component_default_synth_parse_domain_only() {
    let src = read_src("crates/verter_session/src/framework/synth.rs");

    // The synth ctx + impls must NOT name the resolved-validation fact
    // types — those carry resolved-symbol / capability data and are
    // forbidden in the parse-domain synth surface.
    for banned in ["FrameworkScriptFactSet", "FrameworkScriptFacts"] {
        assert!(
            !src.contains(banned),
            "synth.rs must not reference the resolved-validation type `{banned}` \
             — the component-default synth ctx is PARSE-DOMAIN only (macros + \
             syntax-capture candidates); resolved facts stay query-side via \
             FrameworkAdapterCtx::script_facts_for."
        );
    }

    // The ctx whole-struct destructure pin must remain (it proves every
    // field is enumerated, so a new non-parse-domain field cannot be added
    // silently).
    assert!(
        src.contains("script_candidates: _,"),
        "synth.rs must keep the whole-struct destructure of \
         ComponentDefaultSynthCtx so a new field is a compile error, not a \
         silent parse-domain breach."
    );
}

#[test]
fn script_fact_capture_is_syntax_only() {
    let src = read_src("crates/verter_semantic/src/analysis/framework_facts/mod.rs");

    // The capture context is the syntax-only surface: it carries the
    // source + OXC program ONLY. It must NOT carry a resolved-import
    // surface or a capability snapshot (those belong to the
    // resolved-validation half's ResolvedValidationCx).
    let capture_cx = extract_struct(&src, "ScriptCandidateCx");
    for banned in ["ResolvedImportTarget", "capability", "resolved_canonical"] {
        assert!(
            !capture_cx.contains(banned),
            "ScriptCandidateCx must not carry `{banned}` — the syntax-capture \
             half touches OXC + lower_ts_type only; import resolution + \
             capability reads are the resolved-validation half's job."
        );
    }

    // The trait must document and keep the two-half split: a syntax-only
    // `capture` and a resolved `validate`.
    assert!(
        src.contains("fn capture(") && src.contains("fn validate("),
        "the ScriptFactProvider trait must keep the syntax-capture / \
         resolved-validation split (capture + validate)."
    );
}

/// Extract a struct definition's body text (`pub struct <name> ... { ... }`)
/// via a brace-balanced scan from the declaration.
fn extract_struct(src: &str, name: &str) -> String {
    let needle = format!("pub struct {name}");
    let Some(start) = src.find(&needle) else {
        panic!("struct `{name}` not found");
    };
    let after = &src[start..];
    let Some(brace) = after.find('{') else {
        return after.to_string();
    };
    let body = &after[brace + 1..];
    let mut depth = 1i32;
    for (i, b) in body.bytes().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return body[..i].to_string();
                }
            }
            _ => {}
        }
    }
    body.to_string()
}

#[test]
fn script_fact_providers_zero_cost_on_miss() {
    let src = read_src("crates/verter_semantic/src/analysis/framework_facts/mod.rs");
    // The work-doing dispatcher is `capture_script_candidates_with_module_region`
    // (the plain `capture_script_candidates` is a thin `None`-region wrapper over
    // it). The short-circuit lives in the work-doing function.
    let capture = extract_fn_body(&src, "capture_script_candidates_with_module_region");

    // The dispatcher MUST short-circuit on an empty active-provider set
    // with a default (empty) candidate set BEFORE any per-provider work —
    // the byte-identical zero-cost pre-existing path. Removing this fast
    // path would make the no-provider path allocate + iterate.
    assert!(
        capture.contains("active_providers.is_empty()"),
        "capture_script_candidates must keep the `active_providers.is_empty()` \
         zero-cost short-circuit — an empty active set is the byte-identical \
         pre-existing path with ZERO capture work."
    );
    // The empty branch returns the default set (no allocation / iteration).
    let empty_branch_returns_default = capture
        .split("active_providers.is_empty()")
        .nth(1)
        .map(|after| after.contains("FrameworkScriptCandidateSet::default()"))
        .unwrap_or(false);
    assert!(
        empty_branch_returns_default,
        "the empty active-provider branch must return \
         `FrameworkScriptCandidateSet::default()` (zero-cost), not iterate."
    );
}

/// Extract a free function's body via a brace-balanced scan from its
/// `fn <name>(` declaration.
fn extract_fn_body(src: &str, name: &str) -> String {
    let needle = format!("fn {name}(");
    let Some(start) = src.find(&needle) else {
        panic!("fn `{name}` not found");
    };
    let after = &src[start..];
    let Some(brace) = after.find('{') else {
        panic!("fn `{name}` has no body brace");
    };
    let body = &after[brace + 1..];
    let mut depth = 1i32;
    for (i, ch) in body.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return body[..i].to_string();
                }
            }
            _ => {}
        }
    }
    body.to_string()
}

#[test]
fn framework_surface_wire_executor_validates_first() {
    let src = read_src("crates/verter_session/src/typeinfo/framework_surface/executor.rs");

    // Scope to the EXECUTABLE body of `fn execute_framework_surface` (the
    // executor body that runs validation), with comments stripped — a doc
    // comment naming `validate_type_info_graph_request` before the registry
    // lookup must NOT satisfy the ordering, only the real call does.
    let body = strip_comments(&extract_fn_body(&src, "execute_framework_surface"));

    // In the executor body, the real `validate_type_info_graph_request` call
    // MUST appear before the first registry lookup (`framework_registry().get(`)
    // and before any selector resolution — a malformed envelope is rejected
    // before any registry / semantic work.
    let validate_at = body
        .find("validate_type_info_graph_request")
        .expect("executor body must call validate_type_info_graph_request");
    let registry_lookup_at = body
        .find("framework_registry().get(")
        .expect("executor body must look up the registry by adapter id");
    let resolve_selector_at = body
        .find("resolve_component_selector")
        .expect("executor body must resolve the component selector");

    assert!(
        validate_at < registry_lookup_at,
        "validation-first violated: the `validate_type_info_graph_request` \
         call (offset {validate_at}) must precede the registry lookup \
         `framework_registry().get(` (offset {registry_lookup_at}) — a \
         malformed envelope must be rejected before any registry dispatch."
    );
    assert!(
        validate_at < resolve_selector_at,
        "validation-first violated: the validation call (offset {validate_at}) \
         must precede selector resolution (offset {resolve_selector_at})."
    );
}

#[test]
fn svelte_fact_capture_defaults_are_syntax_only() {
    // Prop-default guard: the Svelte prop-DEFAULT capture is SYNTAX-ONLY — it walks the
    // OXC destructuring + slices the source default-value text, but MUST NOT
    // resolve imports or read capability bits (the
    // `script_fact_capture_is_syntax_only` discipline). The default-capture
    // function lives in the syntax-capture provider half.
    let src = read_src("crates/verter_semantic/src/analysis/framework_facts/svelte.rs");
    let capture = extract_fn_body(&src, "collect_bindable_and_defaults");
    // The default capture only reads OXC + the source slice — never a resolver /
    // capability / import-resolution call.
    for banned in [
        "resolve_import",
        "resolved_canonical",
        "capability",
        "ResolvedImportTarget",
        "ProjectSemanticDispatch",
        "store_view",
    ] {
        assert!(
            !capture.contains(banned),
            "collect_bindable_and_defaults must be syntax-only — it must not \
             reference `{banned}` (import resolution / capability reads belong to \
             the resolved-validation half)."
        );
    }
    // It DOES slice the source for the default value (the one allowed source-text
    // read — the runtime default expression display).
    let push = extract_fn_body(&src, "push_default");
    assert!(
        push.contains("source.get("),
        "push_default must slice the default VALUE source text (the runtime \
         default expression display is the one allowed source read)."
    );
}

#[test]
fn resolve_snippet_props_does_not_call_shared_slots_normalizer() {
    // Slot-normalizer guard: the shared Vue `slots_from_typeinfo_surface` is VUE-ONLY.
    // The Svelte snippet path must call its OWN normalizer
    // (`svelte_snippet_slots_from_typeinfo_surface`), never the shared Vue one
    // (which surfaces only a slot callable's first-parameter object and would
    // drop 2nd+ positional snippet params).
    let src = read_src("crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs");
    let body = strip_comments(&extract_fn_body(&src, "resolve_snippet_props"));
    assert!(
        body.contains("svelte_snippet_slots_from_typeinfo_surface"),
        "resolve_snippet_props must call the Svelte-specific \
         `svelte_snippet_slots_from_typeinfo_surface` normalizer."
    );
    assert!(
        !body.contains("slots_from_typeinfo_surface(")
            || body.contains("svelte_snippet_slots_from_typeinfo_surface("),
        "resolve_snippet_props must NOT call the shared Vue \
         `slots_from_typeinfo_surface` (it drops 2nd+ positional snippet params)."
    );
    // The shared Vue normalizer must not even be IMPORTED into svelte_exec (the
    // import was removed when the Svelte normalizer replaced it).
    let import_region = strip_comments(&src);
    assert!(
        !import_region.contains("    slots_from_typeinfo_surface,"),
        "svelte_exec must not import the shared Vue `slots_from_typeinfo_surface` \
         — the Svelte snippet path owns its own normalizer."
    );
}

#[test]
fn vue_shared_slot_normalizer_uses_first_param_only() {
    // Slot-normalizer NO-REGRESSION guard: the shared Vue slot normalizer
    // (`slot_callable_param_and_return`) stays byte-identical to its
    // first-parameter-only behavior — a `TypeExpr::Function` arm surfaces
    // `func.parameters.first()`, NOT every positional param. If a future edit
    // made the SHARED fn iterate all params (the Svelte behavior), Vue slot
    // bindings would regress; this pins the shared fn to first-param-only.
    // The per-surface normalizers (incl. `slot_callable_param_and_return`)
    // relocated into the `vue_exec/normalize.rs` submodule (file-size split);
    // the shared Vue path is still this one module.
    let src =
        read_src("crates/verter_session/src/typeinfo/framework_surface/vue_exec/normalize.rs");
    let body = extract_fn_body(&src, "slot_callable_param_and_return");
    assert!(
        body.contains("func.parameters.first()"),
        "the shared Vue `slot_callable_param_and_return` must keep its \
         first-parameter-only behavior (`func.parameters.first()`) — Svelte's \
         all-positional-params normalizer is a SEPARATE fn and must not leak \
         into the shared Vue path."
    );
    // The Svelte-specific normalizer must NOT live in the shared Vue module.
    assert!(
        !src.contains("svelte_snippet_slots_from_typeinfo_surface"),
        "the Svelte snippet normalizer must NOT live in the shared Vue module \
         (vue_exec/normalize.rs) — it belongs to svelte_exec.rs."
    );
}

#[test]
fn svelte_executor_no_source_or_regex_type_parsing() {
    // Slot/origin/prop-type guard: the Svelte executor drives slot / origin / prop TYPES from
    // the typed IR — NO source slicing, NO regex, NO parse-then-reparse of type
    // text. The ONE allowed source read is the legacy `<slot>` NAME slice
    // (`slice_attr_value`, an existing carve-out) — never type-text parsing.
    let src = read_src("crates/verter_session/src/typeinfo/framework_surface/svelte_exec.rs");
    let stripped = strip_comments(&src);
    for banned in [
        "parse_type_annotation",
        "split_top_level",
        "find_top_level_char",
        "starts_with(\"Pick<\")",
        ".contains(\"/node_modules/\")",
        "Regex::",
        "regex::",
    ] {
        assert!(
            !stripped.contains(banned),
            "svelte_exec.rs must not type-parse source text — found `{banned}`. \
             Walk the typed IR (TypeExpr) instead."
        );
    }
}
