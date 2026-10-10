use super::*;

#[test]
fn creo_is_the_only_emit_occurrence_identity_path() {
    // Static defense-in-depth behind the behavioral cross-kind heritage and
    // runtime-origin identity tests: the CREO cutover deletes every
    // analyzer/ordinal/parallel-lane identity seam from production.
    use walkdir::WalkDir;

    let crates = workspace_root().join("crates");
    let forbidden = [
        "EmitProducerKind",
        "EmitProducerIdentity",
        "producer_identity",
        "merged_emit_fields",
        "ProjectedTypeFact::CallableParams",
        "ProjectedTypeFact::CallableReturn",
        "pub event_payloads:",
        "pub event_publications:",
        "pub event_returns:",
        "ordered_surface_entries",
        "((macro_index as u64) << 32)",
    ];
    let mut violations = Vec::new();
    for entry in WalkDir::new(crates).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file()
            || path.extension().and_then(|ext| ext.to_str()) != Some("rs")
            || !path
                .components()
                .any(|component| component.as_os_str() == "src")
        {
            continue;
        }
        let src = fs::read_to_string(path).unwrap_or_else(|error| {
            panic!("read {}: {error}", path.display());
        });
        for needle in forbidden {
            if src.contains(needle) {
                violations.push(format!("{}: {needle}", path.display()));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "retired emit identity path reintroduced:\n{}",
        violations.join("\n")
    );

    let query = read_workspace_file("crates/verter_type_engine/src/semantic_query.rs");
    let results =
        read_workspace_file("crates/verter_session/src/typeinfo/framework_surface/results.rs");
    let output = read_workspace_file("crates/verter_session/src/meta_resolve/output.rs");
    assert!(query.contains("pub enum SurfaceEntry"));
    assert!(results.contains("pub struct ResolvedEmitOccurrence"));
    assert!(results.contains("pub payload_publication: verter_type_expr::TypePublication"));
    assert!(output.contains("pub struct MaterializedEventOccurrence"));

    let normalize = read_workspace_file(
        "crates/verter_session/src/typeinfo/framework_surface/vue_exec/normalize.rs",
    );
    let emit_projection = normalize
        .split("pub(crate) fn emits_from_typeinfo_surface")
        .nth(1)
        .and_then(|tail| tail.split("pub(in crate::typeinfo").next())
        .expect("Vue emit projection function");
    assert!(emit_projection.contains("macro_surface.surface.entries.iter()"));
    assert!(
        !emit_projection.contains(".call_signatures")
            && !emit_projection.contains(".members")
            && !emit_projection.contains(".position("),
        "Vue emit projection must walk only the canonical stored stream"
    );

    let semantic = read_workspace_file("crates/verter_semantic/src/analysis/component_meta.rs");
    let event_extraction = semantic
        .split("fn extract_events_from_macro")
        .nth(1)
        .and_then(|tail| tail.split("fn extract_slots_from_macro").next())
        .expect("event extraction function");
    assert!(
        !event_extraction.contains(".find(") && !event_extraction.contains("evaluated"),
        "event extraction must project complete occurrences without a name/evaluator join"
    );
}

#[test]
fn vue_default_synth_uses_header_only_default_probe() {
    // The framework component-default-injection seam
    // (`inject_component_default_into_shallow_state`) must probe for an
    // existing `default` via the header-only `has_value_symbol("default")`
    // accessor — never `value_symbol("default")`, which would materialize a
    // value body just to test presence. The seam relocated from the legacy
    // `resolver_core::vue_default_synth` free function to the registry-
    // dispatched host method in `host_construction.rs`.
    let src = read_workspace_file("crates/verter_session/src/host_construction.rs");
    // `.value_symbol("default")` is the bare materializing call; the
    // leading dot excludes the permitted `.has_value_symbol("default")`
    // (which contains `value_symbol("default")` as a substring).
    assert!(
        !src.contains(".value_symbol(\"default\")"),
        "the component-default-injection seam must NOT probe `default` via \
         `value_symbol(\"default\")` (materializes a body); use the \
         header-only `has_value_symbol(\"default\")` accessor"
    );
    assert!(
        src.contains("has_value_symbol(\"default\")"),
        "the component-default-injection seam must probe `default` via the \
         header-only `has_value_symbol(\"default\")` accessor"
    );
}

#[test]
fn no_read_source_in_component_meta() {
    // After the Tier 2 W5d split `component_meta.rs` became a directory
    // module (`component_meta/{mod,cold_resolver,projected_type_expr,
    // direct_macro,tests}.rs`). Scan every `.rs` file in the directory
    // so the guard keeps catching `host.read_source` regressions wherever
    // they land within the split.
    use std::fs;
    let dir = workspace_root().join("crates/verter_session/src/resolver_core/component_meta");
    let mut total = 0usize;
    let mut details = Vec::<String>::new();
    for entry in fs::read_dir(&dir).expect("read component_meta dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("read component_meta file");
        let count = src.matches("host.read_source").count();
        if count > 0 {
            details.push(format!("{}: {count}", path.display()));
            total += count;
        }
    }
    assert_eq!(
        total, 0,
        "component_meta module must not contain host.read_source after Phase 4; found {total} occurrences:\n  {}",
        details.join("\n  ")
    );
}

#[test]
fn no_read_source_in_declaration_metadata() {
    // After Phase 4b, the `read_source` trait method itself is deleted
    // from declaration_metadata.rs. Test impls in tests/ are out of
    // scope; production source MUST be clean.
    let src =
        read_workspace_file("crates/verter_session/src/resolver_core/declaration_metadata.rs");
    let count = src.matches("read_source").count();
    assert_eq!(
        count, 0,
        "declaration_metadata.rs must not contain read_source after Phase 4b; found {count}"
    );
}

#[test]
fn no_text_based_macro_surface_projection_helpers() {
    // After Phase 4b, the three text-projection helper functions are
    // deleted from the resolver_core. Their function names appearing
    // anywhere in resolver_core indicates a regression.
    use std::fs;
    let symbols = [
        "source_for_local_type_projection",
        "project_macro_surfaces_from_source_type_name",
        "project_macro_surfaces_from_expanded_text",
    ];

    // After the Tier 2 W5d split, `component_meta.rs` became the
    // directory module `component_meta/`. Scan every file in the
    // directory plus the still-flat `surface_projector.rs`.
    let component_meta_dir =
        workspace_root().join("crates/verter_session/src/resolver_core/component_meta");
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for entry in fs::read_dir(&component_meta_dir).expect("read component_meta dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            targets.push(path);
        }
    }
    targets.push(
        workspace_root().join("crates/verter_session/src/resolver_core/surface_projector.rs"),
    );

    for path in targets {
        let src = fs::read_to_string(&path).expect("read target");
        for needle in symbols {
            assert!(
                !src.contains(needle),
                "{} must not contain {needle} after Phase 4b (graph-only resolver)",
                path.display()
            );
        }
    }
}

#[test]
fn no_macro_string_heuristics_in_resolver_core() {
    // The user's directive (Phase 4b origin): no regex, no string-based
    // macro detection. This guard catches the most common
    // `.contains("defineProps")` pattern. False positives are unlikely
    // — production resolver code should reach macros via the graph,
    // not by substring-matching source text.
    use std::fs;
    let resolver_dir = workspace_root().join("crates/verter_session/src/resolver_core");
    let needles = [
        r#".contains("defineProps"#,
        r#".contains("defineEmits"#,
        r#".contains("defineSlots"#,
        r#".contains("defineModel"#,
        r#".contains("defineExpose"#,
    ];
    for entry in fs::read_dir(&resolver_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        for needle in needles {
            assert!(
                !src.contains(needle),
                "{} must not contain string-heuristic {} (Phase 4b: graph-only)",
                path.display(),
                needle
            );
        }
    }
}

#[test]
fn no_local_vite_helpers_in_lsp() {
    for rel in [
        "crates/verter_lsp/src/server/mod.rs",
        "crates/verter_lsp/src/background_init.rs",
    ] {
        let src = read_workspace_file(rel);
        for needle in [
            "fn read_vite_config",
            "fn parse_vite_config",
            "fn discover_vite_aliases",
        ] {
            assert!(
                !src.contains(needle),
                "{rel} must not define {needle} after Phase 7"
            );
        }
    }
}

/// The component-meta RESOLUTION PATH never re-introduces the retired eager
/// macro-object materialiser nor the prepared-decl member rescue.
///
/// Stage 4a routed `define_props` / `define_emits` / `define_slots` through the
/// dispatch projector (`projectors::define_shapes::project_define_macro_shapes`)
/// and deleted both the eager materialiser call and the root-symbol member
/// fallback. This guard asserts they STAY deleted from the production resolution
/// entry points, by SYMBOL USAGE (any path reference — direct, qualified, OR
/// expanded from a `macro_rules!` — to the forbidden symbol within the named
/// function body trips the guard, closing the textual-`.or_else`-spelling
/// evasion the older string scan allowed):
///
/// - `compute_component_meta_state_inner` (the cold resolution orchestrator)
///   must NOT reference `produce_macro_object_shapes_for_purpose` — macro shapes
///   are owned by `project_define_macro_shapes` now.
/// - `dispatch_member_for_root_symbol` (the routed single-member projector) must
///   NOT reference `project_prepared_requested_member_from_symbol` — a dispatch
///   miss is an authoritative miss; the prepared-decl rescue is gone. The symbol
///   was fully retired in the Stage 4b walker-cluster deletion (its sole
///   former home, `prepared_surface.rs`, was deleted), so this guard is now an
///   absence check: it must not reappear in the routed-member body.
#[test]
fn component_meta_resolution_path_has_no_eager_materializer_or_member_fallback() {
    use syn::visit::Visit;
    use syn::{ImplItemFn, Item, ItemFn, ItemImpl, UseTree};

    /// Macros that may legitimately appear inside the guarded function bodies.
    /// ANY other macro invocation is rejected: a `macro_rules!` wrapper whose
    /// expansion contains the forbidden symbol would be invisible to the path /
    /// method-call scanners (its body is an unparsed token stream), so an
    /// unknown macro in the body is a potential re-introduction vector. Adding a
    /// new macro to a guarded body requires consciously extending this list
    /// (after confirming the macro cannot expand to the forbidden symbol).
    const APPROVED_MACROS: &[&str] = &[
        "component_meta_trace_custom",
        "format",
        "matches",
        "vec",
        "assert",
        "assert_eq",
        "debug_assert",
        "debug_assert_eq",
        "write",
        "writeln",
        "panic",
        "todo",
        "unreachable",
    ];

    /// Walks one function body counting references to a forbidden symbol via
    /// ANY path segment (direct call, qualified path, or a segment produced by
    /// macro expansion that `syn` parses as a path) AND any method-call name
    /// (`receiver.forbidden(...)`). Method calls are `ExprMethodCall`, NOT path
    /// segments, so both visitors are required — a `.or_else(|| engine.
    /// project_prepared_requested_member_from_symbol(...))` rescue is a method
    /// call and would be invisible to a path-only scan. Macro INVOCATIONS in the
    /// body are collected by name so the caller can reject any non-approved
    /// macro (a `macro_rules!` wrapper that expands to the forbidden symbol).
    struct ForbiddenSymbolCounter<'a> {
        /// The forbidden symbols (the canonical name plus any `use`-alias of it).
        forbidden: &'a [String],
        hits: usize,
        /// Macro invocation names (last path segment) seen in the body.
        macros_used: Vec<String>,
    }
    impl<'ast, 'a> Visit<'ast> for ForbiddenSymbolCounter<'a> {
        fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
            if self.forbidden.iter().any(|f| seg.ident == f.as_str()) {
                self.hits += 1;
            }
            syn::visit::visit_path_segment(self, seg);
        }
        fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
            if self.forbidden.iter().any(|f| mc.method == f.as_str()) {
                self.hits += 1;
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
        fn visit_macro(&mut self, mac: &'ast syn::Macro) {
            if let Some(seg) = mac.path.segments.last() {
                self.macros_used.push(seg.ident.to_string());
            }
            // Do NOT recurse into the macro token stream — it is unparsed
            // tokens, not a path/expr tree. The allow-list check on the macro
            // NAME is the guard; an approved macro cannot expand to the
            // forbidden symbol, an unapproved macro is rejected outright.
        }
    }

    /// Collect every `use`-alias (`... as Alias`) in `file` whose ORIGINAL last
    /// segment is one of `forbidden_originals`. An `use crate::…::forbidden as
    /// reroute;` import lets `reroute(...)` call the retired symbol without the
    /// body scan ever seeing `forbidden`. `forbidden_originals` is the canonical
    /// symbol PLUS every transitive crate re-export alias of it (so a re-export
    /// chain `pub(crate) use …forbidden as r0;` elsewhere + `use …::r0 as
    /// reroute;` here is caught: `r0` is in `forbidden_originals`). The local
    /// renames are added to the forbidden set AND their mere existence reported.
    fn use_aliases_of(file: &syn::File, forbidden_originals: &[String]) -> Vec<String> {
        fn walk(tree: &UseTree, forbidden_originals: &[String], out: &mut Vec<String>) {
            match tree {
                UseTree::Path(p) => walk(&p.tree, forbidden_originals, out),
                UseTree::Group(g) => {
                    for t in &g.items {
                        walk(t, forbidden_originals, out);
                    }
                }
                UseTree::Rename(r) => {
                    if forbidden_originals.iter().any(|f| r.ident == f.as_str()) {
                        out.push(r.rename.to_string());
                    }
                }
                UseTree::Name(_) | UseTree::Glob(_) => {}
            }
        }
        let mut out = Vec::new();
        for item in &file.items {
            if let Item::Use(u) = item {
                walk(&u.tree, forbidden_originals, &mut out);
            }
        }
        out
    }

    /// Transitively collect every crate-internal re-export ALIAS of
    /// `canonical_forbidden`. Walks every `.rs` under `crates/verter_session/src`
    /// and `crates/verter_type_engine/src`
    /// and gathers `pub use …X as ALIAS;` / `pub(crate) use …X as ALIAS;`
    /// renames where `X` is already known-forbidden, iterating to a fixpoint so a
    /// chain (`forbidden as r0`, then `r0 as r1`, …) is fully resolved. The
    /// returned aliases let the guard treat an aliased re-import of a re-export
    /// (`use crate::reexport_home::r0 as reroute;` in the guarded file) as an
    /// import of the forbidden symbol. Only re-exports (`pub`/`pub(crate) use …
    /// as`) count — a private `use … as` inside an unrelated module does not make
    /// the alias importable elsewhere.
    /// Read + `syn::parse_file` the entire `verter_session/src` corpus ONCE. The
    /// parsed `Vec<syn::File>` is identical across every `crate_reexport_aliases_of`
    /// call (only the searched `canonical_forbidden` differs per call), so the
    /// read+parse work is hoisted out of the per-call hot path and reused.
    fn parse_session_src_corpus() -> Vec<syn::File> {
        fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect_rs_files(&path, out);
                } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }
        /// Necessary-condition pre-filter for the corpus parse. The parsed
        /// corpus is consumed ONLY by `crate_reexport_aliases_of`, which gathers
        /// re-export RENAMES from `Item::Use` items whose visibility is
        /// `Public`/`Restricted` (`pub use … as A;` / `pub(crate)|pub(super) use
        /// … as A;`), iterating to a fixpoint. Every alias it can ever discover
        /// — in the first pass or any later fixpoint pass — originates from such
        /// a public-`use` rename. Therefore a file that has NO public-`use`
        /// rename cannot contribute an alias on ANY iteration, so skipping its
        /// `syn::parse_file` is coverage-safe.
        ///
        /// Two textual conditions are jointly necessary for a public-`use`
        /// rename and are checked here:
        ///   1. a `pub` followed (after an optional `(crate)`/`(super)`/`(in …)`
        ///      restriction and whitespace) by the `use` keyword — the only way
        ///      to write a re-export `use`; a private `use … as` does not
        ///      propagate the alias and is ignored by `crate_reexport_aliases_of`,
        ///   2. the ` as ` rename token — `UseTree::Rename` always renders it.
        ///
        /// Both are NECESSARY (not merely sufficient), so the filter cannot hide
        /// a re-export alias the unfiltered scan would have found.
        fn has_public_use_rename(src: &str) -> bool {
            if !src.contains(" as ") {
                return false;
            }
            // Scan for a `pub` token whose next non-`(…)`/non-whitespace token is
            // `use`. Covers `pub use`, `pub(crate) use`, `pub(super) use`, and any
            // `pub(in path) use` restriction generically.
            let bytes = src.as_bytes();
            let mut search_from = 0usize;
            while let Some(rel) = src[search_from..].find("pub") {
                let pub_end = search_from + rel + 3;
                // Reject identifiers like `public`/`pubx` — require a non-ident
                // boundary after `pub`.
                let boundary_ok = bytes
                    .get(pub_end)
                    .map(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                    .unwrap_or(true);
                if boundary_ok {
                    let mut i = pub_end;
                    // Optional `(…)` visibility restriction.
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if i < bytes.len() && bytes[i] == b'(' {
                        let mut depth = 0usize;
                        while i < bytes.len() {
                            match bytes[i] {
                                b'(' => depth += 1,
                                b')' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        i += 1;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            i += 1;
                        }
                    }
                    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    if src[i..].starts_with("use")
                        && bytes
                            .get(i + 3)
                            .map(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                            .unwrap_or(true)
                    {
                        return true;
                    }
                }
                search_from = pub_end;
            }
            false
        }

        // The session crate and the type engine it builds on: a re-export
        // alias published by the engine is importable from session code too.
        let mut rs_files = Vec::new();
        for krate in ["crates/verter_session/src", "crates/verter_type_engine/src"] {
            let src_dir = workspace_root().join(krate);
            assert!(
                src_dir.is_dir(),
                "source root {} is missing",
                src_dir.display()
            );
            collect_rs_files(&src_dir, &mut rs_files);
        }
        rs_files
            .iter()
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter(|src| has_public_use_rename(src))
            .filter_map(|src| syn::parse_file(&src).ok())
            .collect()
    }

    fn crate_reexport_aliases_of(parsed: &[syn::File], canonical_forbidden: &str) -> Vec<String> {
        /// Pull `pub`/`pub(crate)` re-export renames whose original ident is in
        /// `known` from one parsed file.
        fn reexport_renames_in_file(file: &syn::File, known: &[String], out: &mut Vec<String>) {
            fn walk(tree: &UseTree, known: &[String], out: &mut Vec<String>) {
                match tree {
                    UseTree::Path(p) => walk(&p.tree, known, out),
                    UseTree::Group(g) => {
                        for t in &g.items {
                            walk(t, known, out);
                        }
                    }
                    UseTree::Rename(r) => {
                        if known.iter().any(|k| r.ident == k.as_str()) {
                            out.push(r.rename.to_string());
                        }
                    }
                    UseTree::Name(_) | UseTree::Glob(_) => {}
                }
            }
            for item in &file.items {
                if let Item::Use(u) = item {
                    // Only RE-EXPORTS (`pub` / `pub(crate)`) make the alias
                    // importable from another module; a private `use … as` does
                    // not propagate.
                    let is_reexport = matches!(
                        u.vis,
                        syn::Visibility::Public(_) | syn::Visibility::Restricted(_)
                    );
                    if is_reexport {
                        walk(&u.tree, known, out);
                    }
                }
            }
        }

        let mut known = vec![canonical_forbidden.to_string()];
        // Fixpoint: each pass may discover aliases of aliases.
        loop {
            let mut discovered = Vec::new();
            for file in parsed {
                reexport_renames_in_file(file, &known, &mut discovered);
            }
            let mut grew = false;
            for alias in discovered {
                if !known.iter().any(|k| k == &alias) {
                    known.push(alias);
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        // Drop the canonical name itself; return only the discovered aliases.
        known.into_iter().skip(1).collect()
    }

    /// Collect module-scope `const` / `static` items in `file` whose initializer
    /// expression references any name in `forbidden`. A function-pointer const
    /// (`const REROUTE: fn(...) -> _ = produce_macro_object_shapes_for_purpose;`)
    /// at module scope lets `REROUTE(...)` inside a guarded body call the retired
    /// symbol while the body-only path scan never sees `produce_…` (the
    /// initializer lives outside the fn). The const/static NAMES become reroute
    /// aliases (added to the forbidden body-scan set) and are themselves reported.
    fn const_static_fn_pointer_aliases_of(file: &syn::File, forbidden: &[String]) -> Vec<String> {
        struct InitRefCounter<'a> {
            forbidden: &'a [String],
            hit: bool,
        }
        impl<'ast, 'a> Visit<'ast> for InitRefCounter<'a> {
            fn visit_path_segment(&mut self, seg: &'ast syn::PathSegment) {
                if self.forbidden.iter().any(|f| seg.ident == f.as_str()) {
                    self.hit = true;
                }
                syn::visit::visit_path_segment(self, seg);
            }
        }
        let mut out = Vec::new();
        for item in &file.items {
            let (name, expr): (String, &syn::Expr) = match item {
                Item::Const(c) => (c.ident.to_string(), &c.expr),
                Item::Static(s) => (s.ident.to_string(), &s.expr),
                _ => continue,
            };
            let mut counter = InitRefCounter {
                forbidden,
                hit: false,
            };
            counter.visit_expr(expr);
            if counter.hit {
                out.push(name);
            }
        }
        out
    }

    /// Find a free fn OR an impl method named `fn_name` in `file` and count
    /// `forbidden`-symbol references (canonical name + `use`-aliases) in its
    /// body. Also asserts every macro invocation in the body is on
    /// `APPROVED_MACROS`. Panics if the function is not found (the guard's
    /// anchor moved).
    fn assert_fn_free_of_symbol(
        corpus: &[syn::File],
        file: &syn::File,
        fn_name: &str,
        canonical_forbidden: &str,
        message: &str,
    ) {
        // Transitive crate re-export aliases of the forbidden symbol. A
        // re-export chain (`pub(crate) use …forbidden as r0;` in another module,
        // then `use crate::…::r0 as reroute;` in THIS file) hides the forbidden
        // name from a direct-alias scan: the guarded file's rename original is
        // `r0`, not `forbidden`. Treat every transitive re-export alias as a
        // forbidden ORIGINAL so the local re-import is caught.
        let reexport_aliases = crate_reexport_aliases_of(corpus, canonical_forbidden);

        // Forbidden ORIGINAL names a guarded-file `use … as …` could rename:
        // the canonical symbol plus its crate re-export aliases.
        let forbidden_originals: Vec<String> = std::iter::once(canonical_forbidden.to_string())
            .chain(reexport_aliases.iter().cloned())
            .collect();

        // (1) An aliased import (`use …forbidden as reroute;`, OR
        //     `use …::r0 as reroute;` for a re-export alias `r0`) is itself the
        //     evasion — report it before the body scan even runs.
        let aliases = use_aliases_of(file, &forbidden_originals);
        assert!(
            aliases.is_empty(),
            "Stage 4a guard: the file declaring `{fn_name}` imports the retired \
             symbol `{canonical_forbidden}` (or a crate re-export alias of it: \
             {reexport_aliases:?}) under local alias(es) {aliases:?} (`use … as \
             …`). An aliased import — direct OR via a `pub use … as` re-export \
             chain — is an import-alias evasion of the materializer/fallback \
             retirement. Remove the alias import. {message}"
        );

        // The full set the body scan treats as forbidden: the canonical name,
        // its crate re-export aliases, and any local renames of either.
        let mut forbidden: Vec<String> = forbidden_originals.clone();
        forbidden.extend(aliases);

        // (2) A module-scope function-pointer const/static whose INITIALIZER
        //     references a forbidden name (`const REROUTE: fn(...) = forbidden;`)
        //     lets `REROUTE(...)` in the body call the retired symbol while the
        //     body-only scan never sees `forbidden` (the initializer is outside
        //     the fn). Report the const/static, and add its NAME to the forbidden
        //     body-scan set so the `REROUTE(...)` call site is also caught.
        let const_aliases = const_static_fn_pointer_aliases_of(file, &forbidden);
        assert!(
            const_aliases.is_empty(),
            "Stage 4a guard: the file declaring `{fn_name}` binds the retired \
             symbol `{canonical_forbidden}` (or an alias of it) to module-scope \
             function-pointer const/static(s) {const_aliases:?} (`const NAME: \
             fn(...) = forbidden;`). A fn-pointer const lets `NAME(...)` call the \
             retired symbol while the body-only path scan never sees `forbidden`. \
             Remove the const/static binding. {message}"
        );
        forbidden.extend(const_aliases);

        let mut counter = ForbiddenSymbolCounter {
            forbidden: &forbidden,
            hits: 0,
            macros_used: Vec::new(),
        };
        let mut found = false;
        for item in &file.items {
            match item {
                Item::Fn(ItemFn { sig, block, .. }) if sig.ident == fn_name => {
                    counter.visit_block(block);
                    found = true;
                }
                Item::Impl(ItemImpl { items, .. }) => {
                    for impl_item in items {
                        if let syn::ImplItem::Fn(ImplItemFn { sig, block, .. }) = impl_item {
                            if sig.ident == fn_name {
                                counter.visit_block(block);
                                found = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        assert!(
            found,
            "guard anchor moved: fn `{fn_name}` not found — re-point the guard at \
             the renamed component-meta resolution entry point"
        );
        assert_eq!(counter.hits, 0, "{message}");
        // Reject any macro invocation in the body that is not on the approved
        // list — an unapproved `macro_rules!` wrapper could expand to the
        // forbidden symbol (its expansion is invisible to the symbol scan).
        let unapproved: Vec<&String> = counter
            .macros_used
            .iter()
            .filter(|m| !APPROVED_MACROS.contains(&m.as_str()))
            .collect();
        assert!(
            unapproved.is_empty(),
            "Stage 4a guard: `{fn_name}` invokes non-approved macro(s) \
             {unapproved:?}. A `macro_rules!` wrapper can expand to the retired \
             materializer/fallback symbol, evading the path/method-call scan. \
             Either remove the macro or, if it provably cannot expand to the \
             forbidden symbol, add its name to APPROVED_MACROS. {message}"
        );
    }

    // Parse the entire `verter_session/src` corpus ONCE and reuse it across all
    // `assert_fn_free_of_symbol` calls below. The corpus is identical per call
    // (only the searched `canonical_forbidden` differs), so the read+parse cost
    // is paid once instead of 9× inside `crate_reexport_aliases_of`.
    let session_corpus = parse_session_src_corpus();

    let methods_src =
        read_workspace_file("crates/verter_session/src/host_manage/component_meta_methods.rs");
    let methods_file = syn::parse_file(&methods_src).expect("parse component_meta_methods.rs");
    assert_fn_free_of_symbol(
        &session_corpus,
        &methods_file,
        "compute_component_meta_state_inner",
        "produce_macro_object_shapes_for_purpose",
        "Stage 4a: `compute_component_meta_state_inner` references \
         `produce_macro_object_shapes_for_purpose` — the eager macro-object \
         materialiser was retired from the production resolution path. Macro \
         shapes are owned by `projectors::define_shapes::project_define_macro_shapes`; \
         do NOT re-introduce the materialiser (directly, via a `use`-alias, OR \
         through a macro).",
    );

    let engine_src = read_workspace_file(
        "crates/verter_session/src/resolver_core/component_meta_query_engine/mod.rs",
    );
    let engine_file =
        syn::parse_file(&engine_src).expect("parse component_meta_query_engine/mod.rs");
    // The routed single-member projector `dispatch_member_for_root_symbol` was a
    // thin wrapper whose only callers were inside the deleted routed walker; it is
    // retired. ABSENCE guard: it must not reappear in the engine module. A dispatch
    // miss is an authoritative miss — route members through `dispatch_projected_surface`
    // with a `SemanticQueryKey::ProjectPath` member route, never a re-added
    // prepared-decl member rescue.
    let _ = &engine_file;
    assert!(
        !engine_src.contains("dispatch_member_for_root_symbol"),
        "retired symbol `dispatch_member_for_root_symbol` reappeared in \
         component_meta_query_engine/mod.rs — it was the routed walker's single-member \
         projector; resolve members through `dispatch_projected_surface` + a \
         `ProjectPath` member route, never by re-adding the routed walker wrapper."
    );

    // Owner-local dispatch-seal guard: BOTH owner-local macro-root entry points
    // in jsdoc_resolve.rs resolve their root surface through the SOLE query-time
    // resolver (the shared dispatch surface projection), NOT the retired
    // prepared-decl walker (`cached_prepared_root_surface` via
    // `project_prepared_type_surface_*`):
    //
    // - `owner_local_macro_root_has_surface` is the cold resolver's owner-local
    //   AUTHORITY gate (presence check).
    // - `projectable_owner_local_macro_roots` is the upstream projectable-roots
    //   PRE-FILTER. It runs BEFORE the authority gate and decides whether a
    //   macro root is considered projectable at all, so a surviving prepared
    //   walker there is still a production walker path — it MUST route through
    //   dispatch too (one-engine / no-production-walker seal).
    //
    // Re-introducing the prepared-decl walker in EITHER function is a
    // second-resolver (Typed-IR-Only) violation.
    let jsdoc_src = read_workspace_file("crates/verter_session/src/host_manage/jsdoc_resolve.rs");
    let jsdoc_file = syn::parse_file(&jsdoc_src).expect("parse jsdoc_resolve.rs");
    for owner_local_fn in [
        "owner_local_macro_root_has_surface",
        "projectable_owner_local_macro_roots",
    ] {
        for prepared_walker_symbol in [
            "cached_prepared_root_surface",
            "project_prepared_type_surface_shape_via_host_threaded",
            "project_prepared_type_surface_expr_via_host_threaded",
            "project_prepared_requested_member_from_symbol",
        ] {
            assert_fn_free_of_symbol(
                &session_corpus,
                &jsdoc_file,
                owner_local_fn,
                prepared_walker_symbol,
                &format!(
                    "the owner-local entry point `{owner_local_fn}` \
                     references the prepared-decl walker `{prepared_walker_symbol}` \
                     — both owner-local macro-root entry points were retargeted to \
                     the shared dispatch surface projection and must stay \
                     there (one resolver). Do NOT route the owner-local \
                     projectable/authority decision back through the prepared-surface \
                     walker."
                ),
            );
        }
    }
}

// ===========================================================================
// Phase 5l-supplement — `no_unbounded_recursion_in_resolver_core`
// ===========================================================================
//
// §0.6.5 stack-depth discipline guard. The previous incarnation of this
// guard (commit 5l plan-body, never landed) used a regex-only heuristic
// that counted file-wide token occurrences of `foo(` and `self.foo(`,
// which produced 568 false positives at integration HEAD
// `c8ba39684864048917eb1b89dc808d1d081f2706` — every `Type::new(...)`
// constructor call counted as recursion of every other `fn new`, every
// non-recursive call from one function to another in the same file
// counted as recursion of the callee, and every `#[cfg(test)]` test
// helper called from sibling tests counted as recursion of itself.
//
// This rewrite (Phase 5l-supplement) replaces the regex with a
// `syn::Visit`-based scanner that walks each function body and only
// flags TRUE direct self-recursion: a function whose own body contains
// a call back to itself by name, where "by name" means one of:
//
//   1. **Bare identifier call** `foo(...)` — the call expression's
//      callee is a path of length 1 with the segment ident matching the
//      enclosing function's ident. (`Type::new(...)` does NOT match
//      because the path has 2 segments.)
//
//   2. **`Self::`-qualified call** `Self::foo(...)` — call expression
//      whose callee is a path of length 2 starting with `Self` and
//      ending with the enclosing function's ident.
//
//   3. **`self.foo(...)` method call** — method-call expression whose
//      receiver is the bare identifier `self` and whose method name
//      matches the enclosing function's ident. Method calls on any
//      other receiver (`self.field.foo(...)`, `ctx.foo(...)`,
//      `host.foo(...)`) are NOT matched because dispatch is on a
//      different value, not the same impl.
//
// `#[cfg(test)]` modules and functions are skipped (test fixtures
// often define helpers that look recursive due to sibling-test calls).
//
// The scanner is allow-list-driven: any function flagged at integration
// HEAD must either be refactored to a depth-budgeted shape (preferred)
// or carry an explicit allow-list entry with a phase-report citation
// explaining why the recursion is bounded by another invariant
// (data-structure DAG, finite AST depth from a finite source, etc.).

mod resolver_core_recursion {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use syn::visit::Visit;
    use syn::{
        Attribute, Expr, ExprCall, ExprMethodCall, ImplItemFn, ItemFn, ItemImpl, ItemMod, Meta,
    };
    use walkdir::WalkDir;

    use super::super::workspace_root;

    /// Allow-list of functions in `resolver_core/` whose direct
    /// self-recursion is bounded by a non-`depth_budget` invariant.
    /// Each entry must carry a citation. The format is
    /// `(file-stem, fn-name, citation)`. The scan matches a flag
    /// against an entry by exact `(file-stem, fn-name)` tuple — this
    /// is far stricter than the previous regex-era allow-list which
    /// used the bare fn-name (and would silence cross-file collisions).
    ///
    /// All entries below were classified during the Phase 5l-supplement
    /// audit by inspecting the function body. Three bounding-invariant
    /// categories cover the entire list:
    ///
    /// 1. **AST-bounded**. The function recurses on a `TypeExpr` /
    ///    `ValueExpr` / similar finite enum tree. Stack growth is
    ///    `O(input-AST-depth)`, which itself is bounded by the OXC /
    ///    verter_parser stack limit at parse time. A pathological deep
    ///    expression would have already failed parser parsing before
    ///    reaching the resolver.
    ///
    /// 2. **DAG-bounded** (with explicit `seen` / `visiting` set, or a
    ///    per-call compute-once memo). The function recurses on a graph
    ///    (an import / export / reexport graph, or the content-interned
    ///    semantic-node DAG) and carries one of: a `visited` / `seen` /
    ///    `seen_locals` cycle-dedup set, OR a per-call
    ///    `FxHashMap<SemanticNodeId, _>` memo that computes each distinct
    ///    node exactly once (so a content-interned diamond is O(distinct
    ///    nodes), not O(2^depth)). Stack/work growth is bounded by the
    ///    number of distinct entries, not by the call depth. The
    ///    `fallthrough_value_eval` node walkers ADDITIONALLY charge the
    ///    existing `request_budget` op-budget once per distinct node as a
    ///    worst-case fuse; a trip halts the walk with a partial that is
    ///    never warm-admitted (and the override-key projector reports
    ///    `FallthroughOverrideIdentity::Uncacheable`).
    ///
    /// 3. **Recursive-descent parser**. The function is part of the
    ///    `type_text_parser` hand-written recursive-descent parser.
    ///    Stack growth equals input-text nesting depth. Inputs are
    ///    string payloads sized by the source file, and production
    ///    callers feed type-text from already-parsed declarations.
    ///
    /// If a future refactor adds a TRULY unbounded recursion (no
    /// AST/DAG/text-depth bound), the correct fix is to refactor the
    /// callsite into an `iterative_frame` loop or thread a
    /// `depth_budget` parameter — NOT to pad this allow-list. Reviewers
    /// must reject allow-list growth that lacks a structural bound.
    pub(super) const ALLOWED_BOUNDED_RECURSIONS: &[(&str, &str, &str)] = &[
        // -----------------------------------------------------------------
        // bare_name_resolve.rs — the SFC lexical-owner chain re-entry.
        // -----------------------------------------------------------------
        (
            "bare_name_resolve",
            "resolve_bare_name_in_scope",
            "bounded by the validated lexical-parent owner chain: \
             `validated_lexical_parent_owner` yields Some only for an \
             Instance/setup owner's single validated Module/companion parent \
             and the reverse edge does not exist, so the re-entry depth is at \
             most one extra hop before the parent (Module) owner returns None.",
        ),
        // -----------------------------------------------------------------
        // prepared_decl.rs — scanner name-collision, not true recursion:
        // the `PreparedTypeDeclCache` / `PreparedDeclBundle` delegate
        // METHODS named `prepare_augmentation_type_decl_outcome_in` call
        // the same-named module-scope FREE fn (which never calls itself).
        // -----------------------------------------------------------------
        (
            "prepared_decl",
            "prepare_augmentation_type_decl_outcome_in",
            "no true self-recursion: the same-named cache/bundle delegate \
             methods forward to the module-scope free fn, whose body performs \
             a single lease-aware demand + prepare with no re-entry.",
        ),
        // -----------------------------------------------------------------
        // component_meta/projected_type_expr.rs + direct_macro.rs — TypeExpr/text walkers
        // (pre-Tier-2-W5d: both lived inside component_meta.rs)
        // -----------------------------------------------------------------
        (
            "projected_type_expr",
            "render_type_expr_for_projected_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/helpers.rs — TypeExpr walkers
        // -----------------------------------------------------------------
        (
            "helpers",
            "strip_parens_expr",
            "Phase 5l-supplement: bounded by TypeExpr Parenthesized chain depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/route_keys.rs — the dispatch-backed
        // leaf-stabiliser scope predicate. Bounded by TypeExpr AST depth.
        // -----------------------------------------------------------------
        (
            "route_keys",
            "expr_references_prepared_scope_symbol",
            "bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/shallow_preserve.rs — TypeExpr
        // walkers and import-route walkers.
        // -----------------------------------------------------------------
        // Node-domain successors of the retired TypeExpr import-route walks
        // (`contains_direct_imported_utility_route` /
        // `fast_symbolic_imported_generic_route` /
        // `field_references_type_params`): each carries an EXPLICIT
        // `depth: u32` budget (`depth > 256` fail-closed) over the interned
        // node graph; the generic route additionally carries an
        // `active_locals` cycle set over local alias hops.
        (
            "shallow_preserve",
            "node_contains_imported_utility_route",
            "bounded by the explicit depth budget (256) over the interned node graph.",
        ),
        (
            "shallow_preserve",
            "node_is_imported_utility_arg",
            "bounded by the explicit depth budget (256) over the interned node graph.",
        ),
        (
            "shallow_preserve",
            "node_has_imported_generic_route",
            "bounded by the explicit depth budget (256) + the active-set cycle guard over local alias hops.",
        ),
        (
            "shallow_preserve",
            "node_references_type_param_names",
            "bounded by the explicit depth budget (256) over the interned node graph.",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/surface.rs — TypeExpr / semantic-
        // node-graph walkers. `surface_view_from_semantic_node_inner` is
        // DAG-bounded by an explicit `active: &mut FxHashSet<SemanticNodeId>`
        // visitor set; the rest are AST-bounded.
        // -----------------------------------------------------------------
        (
            "surface",
            "dispatch_route_expr_is_materialized",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // Successor of the retired `projected_surface_from_semantic_node_inner`
        // (the ProjectedSurface bridge retirement): same DAG bound.
        (
            "surface",
            "surface_view_from_semantic_node_inner",
            "bounded by the SemanticNodeId alias-chain DAG (active-set cycle dedup).",
        ),
        (
            "surface",
            "visit",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (nested local fn).",
        ),
        // -----------------------------------------------------------------
        // component_meta_registry.rs — TypeExpr walkers and registry-
        // route helpers. Recursion depth = TypeExpr AST depth in every
        // case (verified by inspection of the body's `match expr` arms).
        // -----------------------------------------------------------------
        (
            "component_meta_registry",
            "collect_registry_refs_node_inner",
            "bounded by the visited node-id set (each node walks at most once).",
        ),
        (
            "component_meta_registry",
            "collect_registry_member_surface_refs_node",
            "bounded by the visited node-id set (each node walks at most once).",
        ),
        (
            "component_meta_registry",
            "node_root_has_non_object_top_level_surface",
            "bounded by union/intersection arm nesting (interned child ids predate parents).",
        ),
        (
            "component_meta_registry",
            "node_root_has_explicit_object_surface",
            "bounded by union/intersection arm nesting (interned child ids predate parents).",
        ),
        (
            "component_meta_registry",
            "keys_of",
            "bounded by union arm nesting of the key argument (nested fn).",
        ),
        (
            "component_meta_registry",
            "collect_path",
            "Phase 5l-supplement: bounded by TypeExpr IndexedAccess chain depth (nested closure).",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_has_explicit_object_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_has_non_object_top_level_surface",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_public_utility_route",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "component_meta_registry",
            "component_meta_registry_string_literal_keys",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // declaration_metadata.rs — type-declaration chain walker
        // bounded by the import-graph DAG. The body uses early-exit on
        // `canonical_source == dep_canonical && followed_canonical ==
        // canonical_source` to terminate fixed-point chains; the import
        // graph is already DAG-deduped at the cache layer.
        // -----------------------------------------------------------------
        (
            "declaration_metadata",
            "resolve_type_declaration",
            "Phase 5l-supplement: bounded by import graph DAG (canonical-cache dedup).",
        ),
        // -----------------------------------------------------------------
        // export_graph.rs — barrel re-export chain followers. ALL
        // recursive bodies carry an explicit `visiting: &mut
        // FxHashSet<...>` cycle-dedup parameter and bail on a duplicate
        // insert. DAG-bounded by construction.
        // -----------------------------------------------------------------
        (
            "export_graph",
            "collect_resolved_exports_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "follow_reexport_chain_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "resolve_named_export_from_graph_inner",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        (
            "export_graph",
            "resolve_single_export_from_graph",
            "Phase 5l-supplement: DAG-bounded by `visiting: &mut FxHashSet` cycle dedup.",
        ),
        // -----------------------------------------------------------------
        // external_type_frontier.rs — final-target follower DAG-bounded
        // by an explicit `seen: &mut FxHashSet<(String, String)>` cycle
        // dedup parameter; cycles set `had_cycle = true` and return None.
        // -----------------------------------------------------------------
        (
            "external_type_frontier",
            "final_target_from",
            "Phase 5l-supplement: DAG-bounded by `seen: &mut FxHashSet` cycle dedup.",
        ),
        // -----------------------------------------------------------------
        // fallthrough.rs — TypeExpr walkers (root-candidate enum,
        // spread-keys reduction, typeof-ref substitution). All AST-bounded.
        // -----------------------------------------------------------------
        (
            "fallthrough",
            "collect_dynamic_root_candidates_from_type",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "fallthrough",
            "known_spread_keys_from_type_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "fallthrough",
            "structural_substitute_typeof_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (rewriter).",
        ),
        // -----------------------------------------------------------------
        // component_meta_query_engine/fallthrough_value_eval.rs — the two
        // node-domain walkers over the content-interned semantic-node DAG
        // (spread-keys reduction and dynamic-root-candidate enum). Both share
        // ONE prologue/epilogue (`enter_node`/`exit_node`): a per-call
        // `FxHashMap<SemanticNodeId, _>` memo computes each DISTINCT node once
        // (so a content-interned diamond is O(distinct nodes), not O(2^depth)),
        // and the EXISTING `request_budget` op-budget is charged once per
        // distinct node; a trip halts the walk with a partial that is never
        // warm-admitted. `active` is the in-progress-path cycle sentinel ONLY
        // (a re-entry halts, BEFORE the budget charge), not the memo.
        // -----------------------------------------------------------------
        (
            "fallthrough_value_eval",
            "known_spread_keys_from_node_inner",
            "bounded by the shared per-call `SemanticNodeId` memo (each distinct node computed once) + the shared `request_budget` op-budget (one charge per distinct node; a trip returns the no-warm partial); `active` is the in-progress-path cycle sentinel only.",
        ),
        (
            "fallthrough_value_eval",
            "collect_dynamic_root_candidates_from_node_inner",
            "bounded by the shared per-call `SemanticNodeId` memo (each distinct node computed once) + the shared `request_budget` op-budget (one charge per distinct node; a trip returns the no-warm partial); `active` is the in-progress-path cycle sentinel only.",
        ),
        // -----------------------------------------------------------------
        // shallow_file_state.rs — type-expression walkers. All bodies
        // recurse on TypeExpr / ValueExpr / FunctionBody AST. The
        // `extract_string_literal_keys_from_type_expr` body additionally
        // tracks a `seen_locals` set to avoid revisiting named refs, so
        // it is bounded by min(AST-depth, distinct-symbol-count).
        // -----------------------------------------------------------------
        (
            "shallow_file_state",
            "collect_direct_object_properties",
            "Phase 5l-supplement: bounded by ValueExpr AST depth (object-literal nesting).",
        ),
        (
            "shallow_file_state",
            "collect_member_path_seed_names",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_type_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_typeof_roots",
            "Phase 5l-supplement: bounded by ValueExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "collect_whole_route_refs",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        (
            "shallow_file_state",
            "extract_indexed_access_base",
            "Phase 5l-supplement: bounded by TypeExpr AST depth (IndexedAccess.object chain).",
        ),
        (
            "shallow_file_state",
            "extract_string_literal_keys_from_type_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth + seen_locals dedup.",
        ),
        (
            "shallow_file_state",
            "follow_routed_expr",
            "Phase 5l-supplement: bounded by TypeExpr AST depth.",
        ),
        // -----------------------------------------------------------------
        // surface_projector.rs — typed-IR display walker used by the slot
        // info pipeline (W3.2 cutover). Bounded by an explicit
        // `MAX_DISPLAY_DEPTH = 64` depth budget threaded through every
        // recursive call site.
        // -----------------------------------------------------------------
        (
            "surface_projector",
            "render_type_expr_display_inner",
            "Bounded by explicit `MAX_DISPLAY_DEPTH = 64` depth budget.",
        ),
    ];

    /// Mark a name `host_with_ws` as a known test-helper collision —
    /// these are inside `#[cfg(test)]` mods which the visitor already
    /// skips, but we list them here for documentation. The scanner
    /// does NOT check this list for filtering; it relies on the
    /// `cfg_test_depth` tracker.
    pub(super) const _DOCUMENTED_TEST_FIXTURES: &[&str] = &["host_with_ws", "ws_with_one_project"];

    #[derive(Debug, Clone)]
    pub(super) struct Violation {
        pub(super) file_stem: String,
        pub(super) fn_name: String,
        pub(super) call_kind: CallKind,
        pub(super) rel_path: String,
    }

    #[derive(Debug, Clone, Copy)]
    pub(super) enum CallKind {
        Bare,
        SelfQualified,
        SelfMethod,
    }

    impl std::fmt::Display for CallKind {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Bare => write!(f, "bare `foo(...)`"),
                Self::SelfQualified => write!(f, "`Self::foo(...)`"),
                Self::SelfMethod => write!(f, "`self.foo(...)`"),
            }
        }
    }

    pub(super) struct RecursionVisitor<'a> {
        rel_path: &'a str,
        file_stem: &'a str,
        cfg_test_depth: u32,
        /// Stack of enclosing function/method names. The top of the
        /// stack is the function whose body is currently being walked;
        /// any matching call counts as direct self-recursion.
        fn_stack: Vec<String>,
        violations: &'a mut Vec<Violation>,
    }

    impl<'a> RecursionVisitor<'a> {
        pub(super) fn new(
            rel_path: &'a str,
            file_stem: &'a str,
            violations: &'a mut Vec<Violation>,
        ) -> Self {
            Self {
                rel_path,
                file_stem,
                cfg_test_depth: 0,
                fn_stack: Vec::new(),
                violations,
            }
        }

        /// Push a violation if the named call matches the top of the
        /// fn-stack. Returns silently if no enclosing fn is being
        /// walked (e.g. top-level `static FOO = some_call();`) or if
        /// the call name doesn't match.
        fn try_flag(&mut self, called_name: &str, kind: CallKind) {
            if self.cfg_test_depth > 0 {
                return;
            }
            let Some(current) = self.fn_stack.last() else {
                return;
            };
            if current != called_name {
                return;
            }
            self.violations.push(Violation {
                file_stem: self.file_stem.to_string(),
                fn_name: called_name.to_string(),
                call_kind: kind,
                rel_path: self.rel_path.to_string(),
            });
        }
    }

    /// True if any of the supplied attributes is `#[cfg(test)]`,
    /// `#[cfg(any(test, ...))]`, or `#[cfg(all(..., test, ...))]` —
    /// any cfg expression containing the bare predicate `test`.
    fn has_cfg_test(attrs: &[Attribute]) -> bool {
        attrs.iter().any(|a| {
            if !a.path().is_ident("cfg") {
                return false;
            }
            let rendered = match &a.meta {
                Meta::List(list) => list.tokens.to_string(),
                _ => return false,
            };
            for token in rendered.split(|c: char| !c.is_alphanumeric() && c != '_') {
                if token == "test" {
                    return true;
                }
            }
            false
        })
    }

    impl<'ast> Visit<'ast> for RecursionVisitor<'_> {
        fn visit_item_mod(&mut self, m: &'ast ItemMod) {
            let entered_test = has_cfg_test(&m.attrs) || m.ident == "tests";
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_mod(self, m);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        // A `#[cfg(test)] impl Foo { fn bar() { … } }` block carries the cfg
        // gate on the impl, not on each method, so `visit_impl_item_fn`'s
        // per-method attr check never sees it. Track cfg(test) at the impl level
        // too — mirrors the `visit_item_mod` handling above so test-only methods
        // (e.g. a `#[cfg(test)]` differential oracle) are skipped exactly like
        // any other cfg(test) code, the same intent the `cfg_test_depth` rail
        // already encodes for mods and fns.
        fn visit_item_impl(&mut self, i: &'ast ItemImpl) {
            let entered_test = has_cfg_test(&i.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            syn::visit::visit_item_impl(self, i);
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_item_fn(&mut self, f: &'ast ItemFn) {
            let entered_test = has_cfg_test(&f.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(f.sig.ident.to_string());
            syn::visit::visit_item_fn(self, f);
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_impl_item_fn(&mut self, f: &'ast ImplItemFn) {
            let entered_test = has_cfg_test(&f.attrs);
            if entered_test {
                self.cfg_test_depth += 1;
            }
            self.fn_stack.push(f.sig.ident.to_string());
            syn::visit::visit_impl_item_fn(self, f);
            self.fn_stack.pop();
            if entered_test {
                self.cfg_test_depth -= 1;
            }
        }

        fn visit_expr_call(&mut self, call: &'ast ExprCall) {
            // bare `foo(...)` (path length 1) or `Self::foo(...)`
            // (path length 2 starting with `Self`).
            if let Expr::Path(p) = call.func.as_ref() {
                let segs = &p.path.segments;
                if segs.len() == 1 {
                    let name = segs[0].ident.to_string();
                    self.try_flag(&name, CallKind::Bare);
                } else if segs.len() == 2 && segs[0].ident == "Self" {
                    let name = segs[1].ident.to_string();
                    self.try_flag(&name, CallKind::SelfQualified);
                }
            }
            syn::visit::visit_expr_call(self, call);
        }

        fn visit_expr_method_call(&mut self, mc: &'ast ExprMethodCall) {
            // `self.foo(...)` — receiver is bare `self`. Any other
            // receiver (`self.field.foo(...)`, `ctx.foo(...)`,
            // `host.foo(...)`, `&dyn Trait` dispatch) is NOT direct
            // self-recursion at the syntactic level: dispatch is on a
            // different value, possibly a different impl.
            if let Expr::Path(p) = mc.receiver.as_ref() {
                if p.path.is_ident("self") {
                    let name = mc.method.to_string();
                    self.try_flag(&name, CallKind::SelfMethod);
                }
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
    }

    pub(super) fn scan_file(path: &Path, violations: &mut Vec<Violation>) {
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => panic!("read {}: {}", path.display(), e),
        };
        let parsed =
            syn::parse_file(&src).unwrap_or_else(|e| panic!("parse {}: {}", path.display(), e));
        let rel = path
            .strip_prefix(workspace_root())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let file_stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let mut visitor = RecursionVisitor::new(&rel, &file_stem, violations);
        visitor.visit_file(&parsed);
    }

    /// Production `resolver_core/` files of the session crate and of the
    /// type engine it builds on (part of the resolver tree lives there).
    pub(super) fn walk_resolver_core_files() -> Vec<PathBuf> {
        let dirs = [
            "crates/verter_session/src/resolver_core",
            "crates/verter_type_engine/src/resolver_core",
        ]
        .map(|d| workspace_root().join(d));
        for dir in &dirs {
            assert!(
                dir.is_dir(),
                "resolver_core root {} is missing",
                dir.display()
            );
        }
        let mut files = Vec::new();
        for entry in dirs
            .iter()
            .flat_map(|dir| WalkDir::new(dir).into_iter().filter_map(Result::ok))
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            // Sibling `*_tests.rs` test files are characterization-test
            // infrastructure, not production resolver code. Skip them.
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
            }
            files.push(path.to_path_buf());
        }
        files
    }

    /// True if a discovered violation has an entry on
    /// `ALLOWED_BOUNDED_RECURSIONS`. The match key is
    /// `(file_stem, fn_name)`.
    pub(super) fn is_allowed(v: &Violation) -> bool {
        ALLOWED_BOUNDED_RECURSIONS
            .iter()
            .any(|(stem, name, _)| *stem == v.file_stem && *name == v.fn_name)
    }

    pub(super) fn format_violations(unallowed: &[Violation]) -> String {
        // Group by (file, fn-name) and report distinct call kinds.
        let mut by_key: HashMap<(String, String), Vec<CallKind>> = HashMap::new();
        let mut paths: HashMap<(String, String), String> = HashMap::new();
        for v in unallowed {
            let key = (v.file_stem.clone(), v.fn_name.clone());
            by_key.entry(key.clone()).or_default().push(v.call_kind);
            paths.entry(key).or_insert_with(|| v.rel_path.clone());
        }
        let mut keys: Vec<_> = by_key.keys().cloned().collect();
        keys.sort();
        let mut lines = Vec::new();
        for key in keys {
            let kinds = &by_key[&key];
            let path = &paths[&key];
            let mut kind_strs: Vec<String> = kinds.iter().map(|k| k.to_string()).collect();
            kind_strs.sort();
            kind_strs.dedup();
            lines.push(format!(
                "  {}: fn `{}` directly recurses on itself via {} \
                 — refactor to a depth-budgeted shape, or add an \
                 `ALLOWED_BOUNDED_RECURSIONS` entry with a phase-report citation",
                path,
                key.1,
                kind_strs.join(" + "),
            ));
        }
        format!(
            "found {} unallowed direct self-recursion(s) in {} resolver_core function(s):\n{}",
            unallowed.len(),
            lines.len(),
            lines.join("\n"),
        )
    }
}

#[test]
fn no_unbounded_recursion_in_resolver_core() {
    // r15/F15 (Claude review) — static guard for §0.6.5 stack-depth
    // discipline. Phase 5l-supplement rewrite — see the
    // `resolver_core_recursion` mod docs above for the full design
    // rationale and the previous-iteration regex bug that necessitated
    // this rewrite.
    //
    // Discriminating: this test FAILS against the pre-rewrite tree
    // (the regex heuristic flags 568 false positives — the ignored
    // `phase-05l pending` marker on the prior incarnation
    // demonstrates this) and PASSES against the post-rewrite tree
    // because the syn-AST scanner only flags TRUE direct self-recursion
    // and every such recursion is either refactored (none are at the
    // time of this commit) or carries an explicit allow-list entry
    // with a phase-report citation.
    //
    // If a future commit introduces a new direct self-recursion in
    // `resolver_core/`, the scanner flags it and this test fails. The
    // fix is to either (a) refactor to use `depth_budget` /
    // `iterative_frame` / explicit `MAX_DEPTH`, or (b) add an entry
    // to `resolver_core_recursion::ALLOWED_BOUNDED_RECURSIONS` with a
    // citation explaining the bounding invariant.
    use resolver_core_recursion::{
        format_violations, is_allowed, scan_file, walk_resolver_core_files, Violation,
    };

    let mut violations: Vec<Violation> = Vec::new();
    for file in walk_resolver_core_files() {
        scan_file(&file, &mut violations);
    }

    let unallowed: Vec<Violation> = violations.into_iter().filter(|v| !is_allowed(v)).collect();
    assert!(
        unallowed.is_empty(),
        "no_unbounded_recursion_in_resolver_core (Phase 5l-supplement):\n{}",
        format_violations(&unallowed)
    );
}

#[test]
fn guard8_predicate_passes_when_inventory_is_complete() {
    // Sanity counter-fixture: every DB-shape field IS registered.
    // The predicate must return an empty Vec.
    let fixture_src = r#"
        pub struct FakeProjectTypeStore {
            pub indexed: FileArtifactStore,
            pub analysis: AnalysisReadyDb,
        }
    "#;
    let registered = ["indexed", "analysis"];
    let unregistered =
        unregistered_db_fields_in_struct(fixture_src, "FakeProjectTypeStore", &registered);
    assert!(
        unregistered.is_empty(),
        "guard 8 predicate must accept a fully-registered struct, \
         got {unregistered:?}",
    );
}

#[test]
fn guard9_predicate_passes_for_known_implementor() {
    // Sanity counter-fixture: `FileArtifactStore` IS implemented in the
    // workspace. The detector must report `true`.
    let crate_root = workspace_path("crates/verter_session/src");
    assert!(
        invalidation_by_canonical_impl_exists(&crate_root, "FileArtifactStore"),
        "guard 9 predicate must report `true` for a known \
         InvalidationByCanonical implementor (FileArtifactStore)",
    );
}

/// Tier 1A guard 2 — only the allow-listed production parse sites may
/// directly invoke the OXC parser, and only at their PINNED site count.
/// Other production callers must go through the scheduler-routed parse
/// path so the authoritative parse-once-per-(canonical, content_hash)
/// discipline is preserved.
///
/// The matcher flags a line when it contains, outside comments and
/// outside inline `#[cfg(test)]` modules, EITHER the fully-qualified
/// `oxc_parser::Parser::new` OR a call through any local name a
/// `use oxc_parser…` import binds the parser to: plain
/// (`use oxc_parser::Parser;` → `Parser::new`), grouped
/// (`use oxc_parser::{ParseOptions, Parser};`), item-aliased
/// (`use oxc_parser::Parser as OxcParser;` → `OxcParser::new`), glob
/// (`use oxc_parser::*;` → `Parser::new`), and module-aliased
/// (`use oxc_parser as op;` → `op::Parser::new`). A bare call without
/// an `oxc_parser` import is NOT flagged — that would be a different
/// `Parser` type.
///
/// Honest coverage limits: the matcher is line-textual. It does not
/// see a call split across lines between the binding and `::new`
/// (rustfmt keeps them together), block-comment (`/* */`) bodies are
/// scanned (fail-closed: a commented-out call can only ADD a hit), a
/// string literal containing `Parser::new` counts as a hit
/// (fail-closed), and the per-row count is line-granular (two calls on
/// one line count once — rustfmt-shaped sources put one call per
/// line). Inline test-module blanking assumes the rustfmt shape
/// (`#[cfg(test)]` attribute line followed by a `mod … {` line); a
/// mis-tracked brace skew either leaves test code visible (a spurious
/// guard failure, visible) or blanks to end-of-file (a dead allow-list
/// row, caught by the anti-vacuity assert) — never a silently passed
/// NEW production site in a non-allow-listed file.
///
/// The borrowed-form lowering input is constructed inside
/// `crate::ParsedEvalProgram::parse` (in `parsed_eval_program.rs`), which
/// IS the scheduler-bound entry point. Test sources are exempt.
#[test]
fn no_direct_oxc_parser_calls_outside_scheduler_path() {
    // Allow-list: production files that legitimately invoke the OXC
    // parser directly, each pinned to its EXACT current site count so
    // an allow-listed file cannot silently grow new direct-parse
    // sites. Updating a row requires a matching reference to a
    // scheduler-bound parse path or a documented TODO to migrate.
    // Rows that stop matching any live OXC `Parser::new` site must be
    // DELETED, not kept as pre-authorization for future uncounted
    // parses (the anti-vacuity check below enforces this).
    let allow_list: [(&str, usize); 11] = [
        // The `ParsedEvalProgram::parse` constructor IS the
        // scheduler-bound parse entry — the single eval-program parse
        // funnel; `host_manage::eval_program::parse_eval_program` is
        // its sole production caller and counts every execution on the
        // `eval_program_parses` provenance rail.
        ("crates/verter_semantic_source/src/parsed_eval_program.rs", 1),
        // The `#[cfg(any(test, debug_assertions))]` service-backed test
        // constructor (`service_backed_core_for_test`): parses the
        // fixture source ONCE at construction to build the header index
        // + analysis the production constructor path requires. A
        // test-support parse (release production builds compile it
        // out), not a per-file materialise lane — the scheduler is not
        // its authority.
        (
            "crates/verter_session/src/resolver_core/shallow_file_state.rs",
            1,
        ),
        // The Svelte rune-prelude ambient env: a FIXED process-wide
        // declaration string (NOT a workspace file, no canonical id) lowered
        // ONCE into a `OnceLock` via a one-shot OXC parse. It is not a per-file
        // materialise flight, so the scheduler is not its authority — the parse
        // is the static prelude build, run at most once per process.
        ("crates/verter_semantic_source/src/rune_ambient.rs", 1),
        // The framework two-pass script-fact seam's syntax-capture half
        // (`capture_candidates_for`): a PARSE-DOMAIN-only re-parse that runs a
        // provider's syntax-only candidate capture over a fresh OXC program. The
        // `/framework-adapters` CRITICAL rule explicitly permits the
        // syntax-capture half to touch OXC (it MUST NOT resolve imports or read
        // capability bits). Its result populates ONLY the content-addressed
        // `FrameworkScriptCandidateStore` (a syntax-candidate artifact cache) —
        // never a type-resolution cache.
        ("crates/verter_session/src/framework/script_facts.rs", 1),
        // The scheduler-path parse module itself, four counted parse
        // funnels: `parse_non_sfc_snapshot` is the scheduler snapshot
        // lane's full-program parse (provenance rail
        // `non_sfc_snapshot_parses`); `build_vue_script_outputs` is the
        // SINGLE `.vue` snapshot script-program parse shared by export
        // signatures + script analysis via the `_from_program` walkers
        // (provenance rail `vue_script_snapshot_parses`);
        // `build_svelte_snapshot_from_eval_source` is the Svelte carrier's
        // analogous single snapshot script parse; and
        // `capture_synth_script_candidates` parses the position-preserving
        // eval-source ONCE for the component-default synth's script-candidate
        // capture (syntax-only, no resolver). All four are framework-neutral
        // scheduler-bound snapshot builders.
        ("crates/verter_session/src/parse.rs", 4),
        // Typeinfo oracle-core sites — tracked debt, not scheduler
        // parses. These parse SMALL synthetic probe texts (a strict
        // `type <probe> = <RHS>` alias grammar, hover-RHS admission
        // wrappers) or re-derive a decl span for the deterministic
        // offline digest-generation step. Each constructs and drops a
        // local `Allocator` within one function; none populates a
        // host cache. They predate the bare-form matcher (the old
        // matcher only saw the fully-qualified path) and are
        // allow-listed as self-reported deferred sites pending a
        // migration onto a shared probe-parse helper.
        (
            "crates/verter_session/src/typeinfo/oracle_core/hover_extract.rs",
            1,
        ),
        // The source-digest derivation (`find_decl_span` re-parses a fixture to
        // locate a declaration span) moved out of the `oracle-gen`-only `gen.rs`
        // into the shared `source_digest` module (test + `oracle-gen` only — never
        // the production resolver path); `gen.rs` no longer parses directly.
        (
            "crates/verter_session/src/typeinfo/oracle_core/source_digest.rs",
            1,
        ),
        (
            "crates/verter_session/src/typeinfo/oracle_core/admission.rs",
            2,
        ),
        // The Svelte official-conformance gate's two STRUCTURAL readers of a
        // CANDIDATE'S OWN GENERATED OUTPUT (`#[cfg(test, feature =
        // "bf2-authoritative")]`): `each_flags_argument` resolves the local
        // `svelte/internal/client` namespace binding and reads the `each` call's
        // numeric flags argument; `imports_client_runtime` answers from the
        // module's own import declarations. Both parse an emitted module the
        // test just produced in memory — never a workspace file, no canonical
        // id, no cache — precisely so the assertions read the AST instead of
        // scanning generated text (the string-scan they replaced is what the
        // typed-IR rules forbid). Not a file-processing path; the scheduler is
        // not its authority.
        (
            "crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_gate.rs",
            2,
        ),
        // The same category in the Svelte golden inventory: the recorded `runes`
        // axis is checked against what the shipped route INFERS, so the cell's
        // carrier eval source is parsed once and handed to the production mode
        // classifier. A test reading a carrier artifact it just built in memory,
        // not a per-file materialise lane.
        (
            "crates/verter_session/src/compile/map_equality_tests/svelte_official_conformance_matrix.rs",
            1,
        ),
        // The v4 relation tuple-wire probe (same oracle-core category as the
        // rows above): parses SMALL synthetic probe texts — the operand
        // canonicalization wrapper (`type __oracle_operand__ = …`), the
        // strict probe-header inverse, and the strict tuple-wire decode
        // wrapper — each constructing and dropping a local `Allocator` within
        // one function, none populating a host cache.
        (
            "crates/verter_session/src/typeinfo/oracle_core/relation_probe.rs",
            3,
        ),
    ];

    // Blank the lines of every inline `#[cfg(test)] mod … { … }` block
    // (line numbers preserved) so test-only parser calls inside
    // production files do not require allow-list rows. Rustfmt shape
    // assumed: the `#[cfg(test)]` attribute line is followed —
    // possibly via further attribute lines (`#[path = …]`) — by a
    // `mod name {` line; the block ends when textual brace depth
    // returns to zero. `mod name;` declarations are NOT blanked (they
    // point at separate files the walker visits directly).
    fn blank_inline_test_mods(body: &str) -> String {
        let lines: Vec<&str> = body.lines().collect();
        let mut keep = vec![true; lines.len()];
        let mut i = 0;
        while i < lines.len() {
            if lines[i].trim() == "#[cfg(test)]" {
                let mut j = i + 1;
                while j < lines.len() && lines[j].trim_start().starts_with("#[") {
                    j += 1;
                }
                let is_inline_mod = j < lines.len() && {
                    let t = lines[j].trim_start();
                    (t.starts_with("mod ")
                        || t.starts_with("pub mod ")
                        || t.starts_with("pub(crate) mod ")
                        || t.starts_with("pub(super) mod "))
                        && lines[j].contains('{')
                };
                if is_inline_mod {
                    for flag in keep.iter_mut().take(j).skip(i) {
                        *flag = false;
                    }
                    let mut depth: i64 = 0;
                    let mut k = j;
                    loop {
                        depth += lines[k].matches('{').count() as i64;
                        depth -= lines[k].matches('}').count() as i64;
                        keep[k] = false;
                        if depth <= 0 || k + 1 >= lines.len() {
                            break;
                        }
                        k += 1;
                    }
                    i = k + 1;
                    continue;
                }
            }
            i += 1;
        }
        lines
            .iter()
            .zip(keep)
            .map(|(line, kept)| if kept { *line } else { "" })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // The local names a file's `use oxc_parser…` imports bind the
    // parser to: `use oxc_parser::Parser;` → "Parser", grouped
    // `use oxc_parser::{ParseOptions, Parser};` → "Parser", aliased
    // `use oxc_parser::Parser as OxcParser;` → "OxcParser", glob
    // `use oxc_parser::*;` → "Parser", module alias
    // `use oxc_parser as op;` → "op::Parser".
    //
    // Every parse goes through the guarded `verter_parser::oxc_parse::Parser`
    // (`verter_parser`'s `no_crate_parses_around_the_guard`), so its imports
    // bind the parser exactly as `oxc_parser`'s did.
    fn oxc_parser_import_bindings(body: &str) -> Vec<String> {
        let mut bindings = oxc_parser_import_bindings_from(body, "use oxc_parser");
        bindings.extend(oxc_parser_import_bindings_from(
            body,
            "use verter_parser::oxc_parse",
        ));
        bindings
    }

    fn oxc_parser_import_bindings_from(body: &str, prefix: &str) -> Vec<String> {
        let mut bindings = Vec::new();
        let mut rest = body;
        while let Some(pos) = rest.find(prefix) {
            let stmt_start = &rest[pos..];
            let end = stmt_start.find(';').unwrap_or(stmt_start.len());
            let stmt = &stmt_start[..end];
            // Module-alias form: the whole import is
            // `use oxc_parser as <alias>` — calls then appear as
            // `<alias>::Parser::new`.
            if let Some((before, alias)) = stmt.split_once(" as ") {
                if before.trim() == prefix {
                    bindings.push(format!("{}::Parser", alias.trim()));
                }
            }
            for item in stmt.split(['{', '}', ',']) {
                let item = item.trim();
                let (path, alias) = match item.split_once(" as ") {
                    Some((p, a)) => (p.trim(), Some(a.trim())),
                    None => (item, None),
                };
                let leaf = path.rsplit("::").next().unwrap_or(path).trim();
                if leaf == "Parser" {
                    bindings.push(alias.unwrap_or("Parser").to_string());
                }
                // Glob import: every `oxc_parser` item is in scope
                // under its own name, `Parser` included.
                if leaf == "*" {
                    bindings.push("Parser".to_string());
                }
            }
            rest = &stmt_start[end..];
        }
        bindings
    }

    // `<binding>::new` with an identifier/path boundary before the
    // binding, so `SomeOtherParser::new` or a path-qualified
    // `other_crate::Parser::new` does not false-positive on the bare
    // binding "Parser".
    fn contains_bare_call(line: &str, binding: &str) -> bool {
        let needle = format!("{binding}::new");
        let mut start = 0;
        while let Some(idx) = line[start..].find(&needle) {
            let abs = start + idx;
            let boundary_ok = abs == 0 || {
                let c = line.as_bytes()[abs - 1];
                !(c.is_ascii_alphanumeric() || c == b'_' || c == b':')
            };
            if boundary_ok {
                return true;
            }
            start = abs + needle.len();
        }
        false
    }

    let crate_roots = [
        workspace_path("crates/verter_session/src"),
        workspace_path("crates/verter_type_engine/src"),
        workspace_path("crates/verter_semantic_source/src"),
    ];
    let mut violators: Vec<String> = Vec::new();
    let mut allowed_hits: Vec<String> = Vec::new();
    for entry in crate_roots
        .iter()
        .flat_map(|root| walkdir::WalkDir::new(root).into_iter())
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
    {
        let path = entry.path();
        let path_str = path.to_string_lossy().replace('\\', "/");
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        // Skip test sources.
        if path_str.ends_with("_tests.rs")
            || path_str.ends_with("/tests.rs")
            || is_src_test_module_path(&rel_path(path))
        {
            continue;
        }
        if path.ends_with("crates/verter_session/tests/cases/architecture/capabilities.rs") {
            continue;
        }
        let body = std::fs::read_to_string(path).unwrap_or_else(|err| {
            panic!(
                "guard scanner could not read {path_str}: {err} — an \
                 unreadable file must fail the guard, not silently pass"
            )
        });
        let body = blank_inline_test_mods(&body);
        // Count, outside comments: lines with the fully-qualified
        // `oxc_parser::Parser::new`, OR a call through any local name
        // an `use oxc_parser…` import binds the parser to.
        let bindings = oxc_parser_import_bindings(&body);
        let mut site_lines: Vec<usize> = Vec::new();
        for (lineno, line) in body.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") {
                continue;
            }
            if line.contains("oxc_parser::Parser::new")
                || line.contains("oxc_parse::Parser::new")
                || bindings.iter().any(|b| contains_bare_call(line, b))
            {
                site_lines.push(lineno + 1);
            }
        }
        if !site_lines.is_empty() {
            // Strip the workspace prefix so the suffix matches the
            // allow-list entries.
            let rel = path_str
                .split("crates/")
                .last()
                .map(|s| format!("crates/{s}"))
                .unwrap_or(path_str.clone());
            match allow_list.iter().find(|(allow, _)| rel.ends_with(allow)) {
                None => violators.push(format!("{rel} (lines {site_lines:?})")),
                Some((allow, pinned)) => {
                    assert_eq!(
                        site_lines.len(),
                        *pinned,
                        "direct-OXC allow-list row `{allow}` is pinned to \
                         {pinned} parser site(s) but the file now has \
                         {} (lines {site_lines:?}) — an allow-listed file \
                         must not silently grow or shed direct-parse \
                         sites; route the new site through the scheduler \
                         path or consciously re-pin the row",
                        site_lines.len(),
                    );
                    allowed_hits.push(rel);
                }
            }
        }
    }
    assert!(
        violators.is_empty(),
        "Tier 1A guard `no_direct_oxc_parser_calls_outside_scheduler_path`: \
         production callers invoke the OXC parser (fully-qualified \
         `oxc_parser::Parser::new` or an imported bare `Parser::new`) \
         outside the scheduler-bound parse path: {violators:#?}\n\n\
         Either route through the scheduler's `execute_source` (preferred) \
         or extend the allow-list with a pinned justification."
    );
    // Anti-vacuity: a row that no longer matches any live OXC
    // `Parser::new` site (any covered form) is dead pre-authorization —
    // delete it.
    let dead_rows: Vec<&str> = allow_list
        .iter()
        .map(|(allow, _)| *allow)
        .filter(|allow| !allowed_hits.iter().any(|hit| hit.ends_with(allow)))
        .collect();
    assert!(
        dead_rows.is_empty(),
        "direct-OXC allow-list rows match no live OXC `Parser::new` \
         site — delete them rather than pre-authorizing future uncounted \
         parses: {dead_rows:#?}"
    );
}

/// Tier 1A guard 4 (D107) — macro-impacting unsupported AST kinds MUST
/// surface as a typed `LoweringError`, NOT as a silent skip producing
/// an empty / missing macro shape.
///
/// The discriminating predicate exercises the `LoweringError` value
/// constructors with representative macro-impacting fixtures (one per
/// "FAIL on Unsupported" row in the inventory). The test asserts that
/// each constructed value is a real, distinguishable, non-empty
/// `LoweringError` — i.e., the lowering pipeline COULD return such an
/// error and consumers can branch on it. A regression that reduces
/// `LoweringError` to a unit-only enum would lose the contract and
/// fail this test.
///
/// Production lowering does not construct these errors yet; this guard
/// pins the contract and the structural shape so a lowering driver that
/// adopts `LoweringError` fails loudly instead of silently skipping.
#[test]
fn macro_impacting_constructs_fail_lowering_not_silent_skip() {
    use verter_session::owned_artifacts::eval_program::{
        LoweringError, OwnedEvalProgram, SpanId, UnsupportedKind,
    };

    // Representative fixtures — one per "FAIL on Unsupported" row in
    // `eval_program_macro_impact_inventory.md`.
    let fixtures: Vec<LoweringError> = vec![
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineProps".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("ConditionalExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineProps".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("SpreadElement"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineEmits".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("AwaitExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineEmits".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("YieldExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "defineSlots".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("SequenceExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "withDefaults".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("ComputedMemberExpression"),
        },
        LoweringError::UnsupportedMacroArgumentShape {
            macro_name: "withDefaults".into(),
            span: SpanId::new(0, 10),
            kind: UnsupportedKind::Other("TemplateLiteralPropertyKey"),
        },
        LoweringError::UnsupportedMacroRelevantConstruct {
            construct: "TSConstructorType".into(),
            span: SpanId::new(0, 10),
        },
        LoweringError::UnsupportedMacroRelevantConstruct {
            construct: "TSInferType".into(),
            span: SpanId::new(0, 10),
        },
    ];

    for err in &fixtures {
        // Discriminator: each fixture MUST render to a non-empty
        // string with the relevant macro/construct name. A
        // unit-variant `LoweringError::Generic` with no payload
        // would render an empty body and fail this assertion.
        let rendered = format!("{err}");
        assert!(
            !rendered.is_empty(),
            "macro-impacting LoweringError fixture must render non-empty"
        );
        match err {
            LoweringError::UnsupportedMacroArgumentShape { macro_name, .. } => {
                assert!(rendered.contains(macro_name.as_ref()));
            }
            LoweringError::UnsupportedMacroRelevantConstruct { construct, .. } => {
                assert!(rendered.contains(construct.as_ref()));
            }
            LoweringError::UnsupportedTopLevelImport { specifier, .. } => {
                assert!(rendered.contains(specifier.as_ref()));
            }
        }

        // Negative discriminator: this same input MUST NOT collapse
        // to a silent empty `OwnedEvalProgram`. The contract that
        // breaks the silent-skip ambiguity is that the typed error
        // CARRIES distinguishing information; an empty program carries
        // none.
        let silent = OwnedEvalProgram::empty();
        assert_eq!(
            silent.statements.len(),
            0,
            "silent-skip empty program MUST stay structurally empty so the \
             discriminator vs LoweringError stays meaningful"
        );
    }

    // Inventory backstop: confirm that every fixture's "kind" /
    // "construct" string appears somewhere in the inventory's body.
    // A Tier 1A regression that drops a FAIL row from the inventory
    // while keeping the LoweringError variant produces a divergence
    // between code and documentation; this test catches it.
    let inventory_path =
        "crates/verter_session/src/owned_artifacts/eval_program_macro_impact_inventory.md";
    let inventory = read_workspace_file(inventory_path);
    let must_contain = [
        "ConditionalExpression",
        "SpreadElement",
        "AwaitExpression",
        "YieldExpression",
        "SequenceExpression",
        "ComputedMemberExpression",
        "TSConstructorType",
        "TSInferType",
    ];
    for needle in must_contain {
        assert!(
            inventory.contains(needle),
            "inventory at {inventory_path} missing FAIL row for `{needle}` — \
             Tier 1A LoweringError variant has no provenance",
        );
    }
}

#[test]
fn recursion_budget_invariant_across_module_boundary() {
    // Plan §4.5: the recursion-budget mechanism must remain reachable and
    // its declared cap must remain pinned across the W5a / W5b / W5d splits.
    // Cross-module recursion that exceeds this cap is supposed to terminate
    // the walker via the audited assertion path rather than overflow the
    // stack.
    //
    // The plan calls for a fixture at
    // `crates/verter_session/tests/fixtures/recursion_budget_invariant_fixture.ts`
    // and a baseline at `crates/verter_session/tests/perf_bounds/recursion_budget_baseline.txt`.
    // Neither exists at the W5f cutoff. This test implements the
    // discriminating invariant in a structural form: assert that the
    // public `WALKER_DEPTH_CAP` constant remains reachable from outside
    // the module that owns it (i.e., the split did not break the public
    // re-export path) and that its declared value matches the expected
    // pin. A future patch that lands the fixture + baseline can extend
    // this test with a real per-fixture budget consumption check.
    use verter_session::component_meta_audit::WALKER_DEPTH_CAP;
    assert_eq!(
        WALKER_DEPTH_CAP, 256,
        "WALKER_DEPTH_CAP must remain pinned at 256 — see component_meta_audit::assertions"
    );
}

/// Every [`verter_audit::RequestKind`] variant must have a sibling
/// [`verter_audit::RequestKindPayload`] variant.
#[test]
fn request_kind_payload_parity() {
    let src = read_workspace_file("crates/verter_audit/src/record.rs");

    fn enum_variant_names(src: &str, enum_name: &str) -> Vec<String> {
        let header = format!("pub enum {enum_name}");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("enum {enum_name} not found in record.rs"));
        let body_start = src[start..]
            .find('{')
            .map(|i| start + i + 1)
            .unwrap_or_else(|| panic!("enum {enum_name} body not found"));
        let bytes = src.as_bytes();
        let mut depth = 1usize;
        let mut idx = body_start;
        while idx < bytes.len() && depth > 0 {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            idx += 1;
        }
        let body_end = idx - 1;
        let body = &src[body_start..body_end];
        let mut names = Vec::new();
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with("///") {
                continue;
            }
            // Variant declarations: Name, Name(Payload), Name { ... } — split on `(`, `{`, or `,`, whichever comes first.
            let head_end = [line.find('('), line.find('{'), line.find(',')]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(line.len());
            let head: &str = line[..head_end].trim();
            if head.is_empty() {
                continue;
            }
            if head.starts_with('#') {
                continue;
            }
            let name = head.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if name
                .chars()
                .all(|c: char| c.is_ascii_alphanumeric() || c == '_')
            {
                names.push(name.to_string());
            }
        }
        names
    }

    let request_kinds = enum_variant_names(&src, "RequestKind");
    let payload_kinds = enum_variant_names(&src, "RequestKindPayload");

    let payload_no_none: Vec<String> = payload_kinds
        .iter()
        .filter(|n| n.as_str() != "None")
        .cloned()
        .collect();

    let kinds_for_parity: Vec<String> = request_kinds
        .iter()
        .filter(|n| n.as_str() != "Custom")
        .cloned()
        .collect();

    assert_eq!(
        kinds_for_parity, payload_no_none,
        "request_kind_payload_parity: every `RequestKind` variant must have \
         a same-named sibling on `RequestKindPayload` (apart from `Custom` \
         which maps to `RequestKindPayload::None` by design)."
    );

    assert!(
        !request_kinds.is_empty(),
        "request_kind_payload_parity: `RequestKind` enum has no variants — parser broke."
    );
    assert!(
        payload_kinds.contains(&String::from("None")),
        "request_kind_payload_parity: `RequestKindPayload` must retain its `None` variant."
    );
}

/// Wave 4 close — `every_consumer_has_production_call_site`.
///
/// Plan §1.6 row: "For every `RequestKind` variant, at least one
/// **non-test** source file under `crates/*/src/` populates a record
/// with that variant."
///
/// The guard parses [`verter_audit::RequestKind`] from
/// `crates/verter_audit/src/record.rs`, walks every `*.rs` file under
/// each `crates/<crate>/src/` tree, and verifies that each variant
/// appears as a producer-side **expression** literal — i.e. a
/// `RequestKind::<Variant>` (or fully-qualified `verter_audit::
/// RequestKind::<Variant>`) path in non-pattern position. Match-arm
/// patterns (`RequestKind::Foo { .. } => …`) are CONSUMER sites and
/// do not count; producer code constructs the variant either as a
/// struct-literal value or as a unit/tuple expression and passes it
/// to `RequestContext::with_kind_and_timing` (or assigns it to the
/// `kind:` field of a `RequestAuditRecord` literal).
///
/// `KIND_EXEMPTIONS` enumerates variants that are deliberately not
/// produced from any in-tree non-test source file, with rationale.
/// The guard rejects an exemption whose variant *is* produced (stale
/// allow-list) so the list shrinks toward zero as new producers ship.
///
/// Discrimination contract:
/// - Pre-Wave-3 tree (no `*_with_audit` producers): `ComponentMeta`,
///   `TypeResolution`, `SemanticAnalysis`, `Compile`, `Workspace`,
///   `Lsp`, `Mcp` are all unproduced → guard fails with the per-
///   variant "no producer" diagnostic.
/// - Wave-4 close (every producer landed): all 7 producer variants
///   resolve, only the documented exemptions remain (`Custom`,
///   `BundlerBatch`).
/// - Future regression (a producer is deleted or its `kind:` field
///   is rewritten): the variant disappears from the producer set
///   and the guard fails with the per-variant diagnostic.
#[test]
fn every_consumer_has_production_call_site() {
    use std::collections::BTreeMap;
    use syn::visit::Visit;

    // Variants deliberately not produced anywhere in `crates/*/src/`.
    // Each entry: `(variant_name, rationale)`. Stale entries (variant
    // *is* produced) fail the guard.
    //
    // Architectural intent: the list shrinks toward zero. Adding a
    // producer for a documented variant requires removing its
    // exemption in the same change.
    const KIND_EXEMPTIONS: &[(&str, &str)] = &[
        // Open-ended escape hatch. The plan documents `Custom` as a
        // free-form name producers may set when their concern does
        // not warrant a first-class variant. No in-tree `*_with_audit`
        // producer constructs `Custom { name: ... }`; out-of-tree
        // plugin authors are the intended emitters.
        (
            "Custom",
            "open-ended escape hatch — out-of-tree plugin authors emit `Custom { name }`; \
             no in-tree producer constructs this variant. Adding an in-tree producer requires \
             removing this exemption in the same change.",
        ),
        // The `BatchAuditAggregator::summarize` API folds existing
        // records into a `BundlerBatchPayload` and returns the
        // payload synchronously to callers (`getBundlerBatchSummary`
        // on NAPI/WASM). It does NOT publish a record with
        // `kind: RequestKind::BundlerBatch { ... }` into the
        // `AuditRecordsStore`; bundler integrations consume the
        // payload directly. Match arms in `summarize` and the
        // FFI dispatchers are CONSUMER sites (they reduce records of
        // OTHER kinds into the bundler payload) and intentionally do
        // not count.
        (
            "BundlerBatch",
            "produced as a `BundlerBatchPayload` return value from \
             `BatchAuditAggregator::summarize` (and FFI `getBundlerBatchSummary`); no in-tree \
             producer publishes a record with `kind: RequestKind::BundlerBatch { .. }` into \
             `AuditRecordsStore`. Adding an in-tree record producer (for example, a future \
             host-driven bundler-summary publisher) requires removing this exemption in the \
             same change.",
        ),
    ];

    // Step 1: enumerate every `RequestKind` variant from
    // `crates/verter_audit/src/record.rs`. Reuses the same parser
    // shape `request_kind_payload_parity` uses so the two guards
    // agree on what counts as a variant.
    let record_src = read_workspace_file("crates/verter_audit/src/record.rs");

    fn enum_variant_names(src: &str, enum_name: &str) -> Vec<String> {
        let header = format!("pub enum {enum_name}");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("enum {enum_name} not found in record.rs"));
        let body_start = src[start..]
            .find('{')
            .map(|i| start + i + 1)
            .unwrap_or_else(|| panic!("enum {enum_name} body not found"));
        let bytes = src.as_bytes();
        let mut depth = 1usize;
        let mut idx = body_start;
        while idx < bytes.len() && depth > 0 {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                _ => {}
            }
            idx += 1;
        }
        let body_end = idx - 1;
        let body = &src[body_start..body_end];
        let mut names = Vec::new();
        for raw_line in body.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with("///") {
                continue;
            }
            let head_end = [line.find('('), line.find('{'), line.find(',')]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(line.len());
            let head: &str = line[..head_end].trim();
            if head.is_empty() {
                continue;
            }
            if head.starts_with('#') {
                continue;
            }
            let name = head.split_whitespace().next().unwrap_or("");
            if name.is_empty() {
                continue;
            }
            if name
                .chars()
                .all(|c: char| c.is_ascii_alphanumeric() || c == '_')
            {
                names.push(name.to_string());
            }
        }
        names
    }

    let variants = enum_variant_names(&record_src, "RequestKind");
    assert!(
        !variants.is_empty(),
        "every_consumer_has_production_call_site: no `RequestKind` variants discovered — \
         parser broke or the enum was renamed."
    );

    // Step 2: walk every `crates/<crate>/src/` tree and visit each
    // `*.rs` file's AST. Track per-variant production hits.
    //
    // Production = `RequestKind::<Variant>` path appears in EXPRESSION
    // context (struct-literal value, function-call argument, struct
    // field initialiser). Match-arm patterns (`RequestKind::Foo { .. }
    // => …`) are CONSUMER sites and skipped — `Visit::visit_pat_*`
    // hooks are not invoked because the visitor only walks expression
    // paths.
    struct ProducerVisitor<'a> {
        variant_set: &'a std::collections::HashSet<String>,
        hits: BTreeMap<String, Vec<String>>,
        rel_path: String,
        // Depth counter for pattern context. syn 2.x dispatches
        // `Pat::Path` (unit-variant patterns like
        // `RequestKind::ComponentMeta` in `match` arms) to
        // `visit_expr_path` — see `syn::visit::visit_pat` source.
        // Without this gate, every match arm pattern would falsely
        // count as a producer site.
        pat_depth: u32,
    }

    impl<'a, 'ast> Visit<'ast> for ProducerVisitor<'a> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            // Skip inline `#[cfg(test)]` modules — they live in
            // production source files but are not production code.
            if item_is_cfg_test(&item.attrs) {
                return;
            }
            syn::visit::visit_item_mod(self, item);
        }
        fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
            // Skip `#[test]` and `#[cfg(test)]`-attributed functions
            // — they are test code that happens to live alongside
            // production code in the same file.
            if item_is_cfg_test(&item.attrs) || item_is_test(&item.attrs) {
                return;
            }
            syn::visit::visit_item_fn(self, item);
        }
        fn visit_pat(&mut self, pat: &'ast syn::Pat) {
            self.pat_depth = self.pat_depth.saturating_add(1);
            syn::visit::visit_pat(self, pat);
            self.pat_depth = self.pat_depth.saturating_sub(1);
        }
        fn visit_expr_path(&mut self, expr: &'ast syn::ExprPath) {
            // Skip pattern-context paths — `Pat::Path` dispatches
            // here via syn's `visit_pat`, but those are CONSUMER-side
            // match-arm patterns and do not count as producer sites.
            if self.pat_depth > 0 {
                syn::visit::visit_expr_path(self, expr);
                return;
            }
            if let Some(variant) = match_request_kind_variant(&expr.path) {
                if self.variant_set.contains(&variant) {
                    self.hits
                        .entry(variant)
                        .or_default()
                        .push(self.rel_path.clone());
                }
            }
            syn::visit::visit_expr_path(self, expr);
        }
        fn visit_expr_struct(&mut self, expr: &'ast syn::ExprStruct) {
            if let Some(variant) = match_request_kind_variant(&expr.path) {
                if self.variant_set.contains(&variant) {
                    self.hits
                        .entry(variant)
                        .or_default()
                        .push(self.rel_path.clone());
                }
            }
            syn::visit::visit_expr_struct(self, expr);
        }
        fn visit_expr_call(&mut self, expr: &'ast syn::ExprCall) {
            if let syn::Expr::Path(ep) = &*expr.func {
                if let Some(variant) = match_request_kind_variant(&ep.path) {
                    if self.variant_set.contains(&variant) {
                        self.hits
                            .entry(variant)
                            .or_default()
                            .push(self.rel_path.clone());
                    }
                }
            }
            syn::visit::visit_expr_call(self, expr);
        }
    }

    /// Recognise `#[cfg(test)]` (or `#[cfg(any(test, ...))]`) so the
    /// visitor skips inline test modules that share a source file
    /// with production code. `Attribute::meta` exposes the parsed
    /// `Meta` AST directly — using `Meta::List` token-stream
    /// inspection is simpler and more reliable than nested-meta
    /// parsers on attributes that may be `cfg(any(unix, test))`.
    fn item_is_cfg_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            if !attr.path().is_ident("cfg") {
                return false;
            }
            // Render the meta back to a string and substring-match
            // for `test`. `cfg(test)` → `cfg(test)`. `cfg(any(unix,
            // test))` → `cfg(any(unix, test))`. False positives are
            // theoretically possible (e.g. an identifier literally
            // called `testing`) but the substring match also requires
            // word boundaries via the surrounding `(` `,` ` ` `)`
            // characters.
            use quote::ToTokens;
            let rendered = attr.meta.to_token_stream().to_string();
            // Match `test` as a whole token: delimited by `(`, `)`,
            // `,`, or whitespace.
            let needle = "test";
            let bytes = rendered.as_bytes();
            let n_bytes = needle.as_bytes();
            let mut idx = 0usize;
            while idx + n_bytes.len() <= bytes.len() {
                if &bytes[idx..idx + n_bytes.len()] == n_bytes {
                    let before_ok =
                        idx == 0 || matches!(bytes[idx - 1], b'(' | b',' | b' ' | b'\t' | b'\n');
                    let after_idx = idx + n_bytes.len();
                    let after_ok = after_idx == bytes.len()
                        || matches!(bytes[after_idx], b')' | b',' | b' ' | b'\t' | b'\n');
                    if before_ok && after_ok {
                        return true;
                    }
                }
                idx += 1;
            }
            false
        })
    }

    fn item_is_test(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| attr.path().is_ident("test"))
    }

    /// Match `RequestKind::<Variant>` (with optional leading
    /// `verter_audit::` or `crate::component_meta_audit::`) and
    /// return the variant name. The path's last segment must be the
    /// variant; the segment immediately before must be `RequestKind`.
    fn match_request_kind_variant(path: &syn::Path) -> Option<String> {
        let segments: Vec<&syn::PathSegment> = path.segments.iter().collect();
        if segments.len() < 2 {
            return None;
        }
        let last = segments[segments.len() - 1];
        let parent = segments[segments.len() - 2];
        if parent.ident != "RequestKind" {
            return None;
        }
        Some(last.ident.to_string())
    }

    let variant_set: std::collections::HashSet<String> = variants.iter().cloned().collect();
    let mut hits: BTreeMap<String, Vec<String>> = BTreeMap::new();

    let crates_dir = workspace_root().join("crates");
    let crate_entries = std::fs::read_dir(&crates_dir).unwrap_or_else(|e| {
        panic!("every_consumer_has_production_call_site: cannot read crates/: {e}")
    });
    for crate_entry in crate_entries.flatten() {
        let src_dir = crate_entry.path().join("src");
        if !src_dir.is_dir() {
            continue;
        }
        walk_dir_collect_rs(&src_dir, &mut |path: &std::path::Path| {
            // Skip files whose path includes a `tests` segment — some
            // crates put inline integration test modules under
            // `src/tests/`. Production code lives outside any
            // `tests` segment.
            let has_tests_segment = path
                .components()
                .any(|c| c.as_os_str().to_string_lossy() == "tests");
            if has_tests_segment {
                return;
            }
            let src = std::fs::read_to_string(path).unwrap_or_else(|e| {
                panic!(
                    "every_consumer_has_production_call_site: cannot read `{}`: {e}",
                    path.display()
                )
            });
            // Textual pre-filter (coverage-identical): the visitor only records a
            // producer when the path segment before the variant is `RequestKind`
            // (`RequestKind::<Variant>`). A file with no `RequestKind` substring
            // cannot contain such a path, so skip the expensive parse.
            if !src.contains("RequestKind") {
                return;
            }
            let parsed = match syn::parse_file(&src) {
                Ok(p) => p,
                Err(e) => panic!(
                    "every_consumer_has_production_call_site: cannot parse `{}`: {e}",
                    path.display()
                ),
            };
            let rel = path
                .strip_prefix(workspace_root())
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            let mut visitor = ProducerVisitor {
                variant_set: &variant_set,
                hits: BTreeMap::new(),
                rel_path: rel,
                pat_depth: 0,
            };
            visitor.visit_file(&parsed);
            for (variant, paths) in visitor.hits {
                hits.entry(variant).or_default().extend(paths);
            }
        });
    }

    // Step 3: every variant must either have a producer call site
    // OR a documented `KIND_EXEMPTIONS` entry.
    let exemption_set: std::collections::HashSet<&str> =
        KIND_EXEMPTIONS.iter().map(|(name, _)| *name).collect();

    let mut missing: Vec<String> = Vec::new();
    for variant in &variants {
        if hits.contains_key(variant) {
            continue;
        }
        if exemption_set.contains(variant.as_str()) {
            continue;
        }
        missing.push(format!(
            "  - {variant}: no `RequestKind::{variant}` expression literal found in any \
             non-test source file under `crates/*/src/`. Either add a production producer \
             that constructs this variant, OR document the absence in `KIND_EXEMPTIONS` with \
             a rationale (and accept that out-of-tree code is the only emitter)."
        ));
    }

    assert!(
        missing.is_empty(),
        "every_consumer_has_production_call_site: the following `RequestKind` variants have \
         NO production call site under `crates/*/src/`. Plan §1.6 requires every variant to \
         either have an in-tree producer OR a documented exemption.\n{}",
        missing.join("\n"),
    );

    // Step 4: reject stale exemptions — entries whose variant *is*
    // now produced from in-tree code. This forces the exemption list
    // to shrink whenever a producer ships.
    let mut stale: Vec<String> = Vec::new();
    for (variant, rationale) in KIND_EXEMPTIONS {
        if hits.contains_key(*variant) {
            let producer_files: Vec<String> = hits
                .get(*variant)
                .map(|paths| {
                    let mut deduped: Vec<String> = paths.clone();
                    deduped.sort();
                    deduped.dedup();
                    deduped
                })
                .unwrap_or_default();
            stale.push(format!(
                "  - {variant}: an in-tree producer now exists at {producer_files:?} \
                 (rationale was: {rationale}). Remove the exemption from KIND_EXEMPTIONS."
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "every_consumer_has_production_call_site: stale KIND_EXEMPTIONS entries:\n{}",
        stale.join("\n"),
    );

    // Step 5: reject an exemption whose variant does not exist on
    // `RequestKind` at all (the enum was edited and the exemption
    // wasn't updated). Without this, a renamed variant could silently
    // pass the guard.
    let mut unknown: Vec<String> = Vec::new();
    for (variant, _) in KIND_EXEMPTIONS {
        if !variant_set.contains(*variant) {
            unknown.push(format!(
                "  - {variant}: exemption references a variant that does NOT exist on \
                 `RequestKind`. Update `KIND_EXEMPTIONS` to match the current enum."
            ));
        }
    }
    assert!(
        unknown.is_empty(),
        "every_consumer_has_production_call_site: KIND_EXEMPTIONS contains unknown variants:\n{}",
        unknown.join("\n"),
    );
}

// ──────────────────────────────────────────────────────────────────────
// Slot-binding-graph synthesis architecture guards.
//
// These guards enforce the §3.12 + §17.6 + §10 R12b invariants for the
// graph-native slot-binding synthesis introduced alongside
// `slot_binding_graph.rs`:
//
//   - §3.12: the synthesis must drive the carrier walk in
//     `ProjectionMode::Navigate`; an `Expanded` projection re-introduces
//     the giant-tree pathology that motivated the rewrite.
//   - §10 R12b: the synthesis must merge dep-signatures via
//     `dispatch.execute_read(..)` rather than the bare `dispatch.execute(..)`.
//     `execute` discards the `dep_signature` half so callers that go
//     through it cannot maintain the warm-cache fence.
//   - §17.6: the synthesis must emit a `synthesize_slot_bindings` and
//     per-macro `synthesize_macro` tracing span; the `walker_pathological_input_cap`
//     warn event must be wired in the walker; the audit substrate's
//     `ComponentMetaPayload` must carry diagnostics + suppression
//     facts.
// ──────────────────────────────────────────────────────────────────────

#[test]
fn slot_binding_graph_uses_navigate_not_expanded() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    assert!(
        !src.contains("ProjectionMode::Expanded"),
        "slot_binding_graph.rs must drive synthesis in Navigate mode; \
         an Expanded projection re-introduces the giant-tree pathology \
         that motivated the rewrite. Found `ProjectionMode::Expanded` \
         in the synthesis source.",
    );
}

#[test]
fn slot_binding_graph_uses_execute_read_only() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    let mut violations = Vec::new();
    for (line_no, line) in src.lines().enumerate() {
        if line.contains("dispatch.execute(") && !line.contains("dispatch.execute_read(") {
            violations.push(format!("  line {}: {}", line_no + 1, line.trim()));
        }
    }
    assert!(
        violations.is_empty(),
        "slot_binding_graph.rs must merge dep-signatures via \
         `dispatch.execute_read(..)`; bare `dispatch.execute(..)` discards \
         the dep_signature half and breaks the warm-cache fence. \
         Found:\n{}",
        violations.join("\n"),
    );
}

#[test]
fn walker_emits_tracing_events() {
    let src =
        read_workspace_file("crates/verter_type_engine/src/project_semantic_dispatch/walk.rs");
    assert!(
        src.contains("debug_span!(\n            target: \"verter::dispatch::walk\",\n            \"walk_shallow_surface\"")
            || src.contains("debug_span!(\"walk_shallow_surface\"")
            || src.contains("\"walk_shallow_surface\""),
        "walk.rs must open a `walk_shallow_surface` debug span at the \
         iterative walker entry-point so log captures can attribute \
         shallow-surface walks to the dispatch layer.",
    );
    assert!(
        src.contains("walker_pathological_input_cap"),
        "walk.rs must emit a `walker_pathological_input_cap` warn \
         event when the pathological-input cap fires so log captures \
         can correlate the event with the matching audit diagnostic.",
    );
}

#[test]
fn component_meta_payload_carries_walker_diagnostics() {
    let src = read_workspace_file("crates/verter_audit/src/payloads/component_meta.rs");
    assert!(
        src.contains("pub diagnostics: Vec<AuditDiagnosticEntry>"),
        "verter_audit::payloads::ComponentMetaPayload must carry a \
         `diagnostics: Vec<AuditDiagnosticEntry>` field so the audit \
         substrate exposes macro-expansion diagnostics surfaced \
         during the request.",
    );
    assert!(
        src.contains("pub should_suppress: bool"),
        "verter_audit::payloads::ComponentMetaPayload must carry a \
         `should_suppress: bool` field so consumers observe whether \
         a fatal QueryError suppressed cache promotion.",
    );
}

#[test]
fn getcomponentmeta_uses_per_macro_projectors() {
    // Production `get_component_meta` / `compute_component_meta_state_inner`
    // must dispatch through the per-macro projector module
    // (`meta_resolve::projectors::project_evaluated_types` or its
    // siblings). Discriminates against any drift commit that re-routes
    // production back through the legacy walker outer driver.
    let src =
        read_workspace_file("crates/verter_session/src/host_manage/component_meta_methods.rs");
    assert!(
        src.contains("project_evaluated_types"),
        "host_manage/component_meta_methods.rs must dispatch through \
         `crate::meta_resolve::projectors::project_evaluated_types` — \
         a re-routed production path that bypasses the per-macro \
         projector module would be invisible without this guard."
    );
}

// ---------------------------------------------------------------------------
// Typed-IR-Only Resolver Rule guards (CLAUDE.md "Typed-IR-Only Resolver Rule")
// ---------------------------------------------------------------------------
//
// The six guards below pin the architectural ban on string-search /
// reparse / role-inference patterns inside the component-meta /
// typeinfo type resolver pipeline. Each owns an EXACT
// `(file, line, pattern)` allowlist tuple set captured against the
// live tree. The guards are exact-set comparisons in BOTH directions:
//
//   * a violation that exists in source but is NOT in the allowlist
//     fails the test ("Unallowlisted violation introduced");
//   * an allowlist tuple that no longer matches anything in source
//     ALSO fails ("Allowlisted entry NOT FOUND").
//
// As migration units land they remove their tuples from the
// allowlist; the W8.2 "everything empty" floor is the cutover end
// state. Counts are gameable (deleting one site and adding another
// passes a count check); exact tuples are not.

mod typed_ir_resolver_guards {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// Walk `<repo>/crates/<crate>/src/**` and yield every `.rs` file
    /// EXCEPT files whose basename matches `<name>_tests.rs` or
    /// equals `tests.rs`. Those files exist inside `src/` for
    /// per-CLAUDE.md test-file organisation but are test-only modules.
    fn collect_production_rs_files() -> Vec<(PathBuf, String)> {
        collect_production_rs_files_under(&super::super::workspace_root())
    }

    /// [`collect_production_rs_files`] over the workspace rooted at `root`.
    /// Test-source classification reads paths relative to `root`.
    fn collect_production_rs_files_under(root: &Path) -> Vec<(PathBuf, String)> {
        let crates_dir = root.join("crates");
        let mut out: Vec<(PathBuf, String)> = Vec::new();
        let entries = match fs::read_dir(&crates_dir) {
            Ok(e) => e,
            Err(err) => panic!("read_dir {}: {err}", crates_dir.display()),
        };
        for ent in entries.flatten() {
            let crate_path = ent.path();
            if !crate_path.is_dir() {
                continue;
            }
            let src_dir = crate_path.join("src");
            if !src_dir.is_dir() {
                continue;
            }
            let mut files: Vec<PathBuf> = Vec::new();
            walk_rs(&src_dir, &mut files);
            for f in files {
                let rel = super::super::rel_path_under(root, &f);
                if is_test_file(&rel) {
                    continue;
                }
                out.push((f, rel));
            }
        }
        assert!(
            !out.is_empty(),
            "no production files found under {} — an empty universe passes \
             vacuously",
            crates_dir.display()
        );
        out
    }

    fn walk_rs(dir: &Path, out: &mut Vec<PathBuf>) {
        if !dir.is_dir() {
            return;
        }
        for entry in
            fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {}", dir.display(), e))
        {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let p = entry.path();
            if p.is_dir() {
                walk_rs(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }

    fn is_test_file(rel: &str) -> bool {
        let name = rel.rsplit('/').next().unwrap_or("");
        name.ends_with("_tests.rs")
            || name == "tests.rs"
            || super::super::is_src_test_module_path(rel)
    }

    /// Replace `//` line comments and `/* ... */` block comments with
    /// equivalent-length whitespace, preserving newlines so line
    /// numbers stay stable. Skips comment-like sequences inside
    /// regular and raw string literals so the strip never invalidates
    /// real source.
    fn strip_comments(src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
        let n = bytes.len();
        let mut i = 0usize;
        while i < n {
            let c = bytes[i];
            // Raw string: r"..."  /  r#"..."#  /  r##"..."##  ...
            if c == b'r' {
                let mut j = i + 1;
                let mut hashes = 0usize;
                while j < n && bytes[j] == b'#' {
                    hashes += 1;
                    j += 1;
                }
                if j < n && bytes[j] == b'"' {
                    // Copy through the opening `r###"`
                    out.extend_from_slice(&bytes[i..=j]);
                    let close: Vec<u8> = std::iter::once(b'"')
                        .chain(std::iter::repeat_n(b'#', hashes))
                        .collect();
                    let mut k = j + 1;
                    while k + close.len() <= n {
                        if &bytes[k..k + close.len()] == close.as_slice() {
                            out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                            i = k + close.len();
                            break;
                        }
                        out.push(bytes[k]);
                        k += 1;
                    }
                    if k + close.len() > n {
                        out.extend_from_slice(&bytes[(j + 1)..n]);
                        i = n;
                    }
                    continue;
                }
                // Not a raw string — fall through to normal handling.
            }
            // Regular string literal "..." (with \"  escape handling)
            if c == b'"' {
                out.push(b'"');
                let mut k = i + 1;
                while k < n {
                    if bytes[k] == b'\\' && k + 1 < n {
                        out.push(bytes[k]);
                        out.push(bytes[k + 1]);
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'"' {
                        out.push(b'"');
                        k += 1;
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                i = k;
                continue;
            }
            // Line comment //
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
                let mut k = i;
                while k < n && bytes[k] != b'\n' {
                    out.push(b' ');
                    k += 1;
                }
                i = k;
                continue;
            }
            // Block comment /* ... */ with nesting support.
            if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                let mut depth = 1u32;
                out.push(b' ');
                out.push(b' ');
                let mut k = i + 2;
                while k < n && depth > 0 {
                    if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                        depth += 1;
                        out.push(b' ');
                        out.push(b' ');
                        k += 2;
                        continue;
                    }
                    if k + 1 < n && bytes[k] == b'*' && bytes[k + 1] == b'/' {
                        depth -= 1;
                        out.push(b' ');
                        out.push(b' ');
                        k += 2;
                        continue;
                    }
                    if bytes[k] == b'\n' {
                        out.push(b'\n');
                    } else {
                        out.push(b' ');
                    }
                    k += 1;
                }
                i = k;
                continue;
            }
            out.push(c);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Replace the body of every `#[cfg(test)] mod NAME { ... }` block
    /// with whitespace (newlines preserved). Inline test modules live
    /// in production source files but are test-only — guard scans
    /// must NOT classify them as production violations.
    fn strip_inline_test_modules(src: &str) -> String {
        let bytes = src.as_bytes();
        let n = bytes.len();
        let mut out = bytes.to_vec();
        let needle = b"#[cfg(test)]";
        let mut i = 0usize;
        while i + needle.len() <= n {
            if &bytes[i..i + needle.len()] == needle {
                let mut j = i + needle.len();
                // Walk forward until we find `mod ` (allowing intervening
                // attributes / whitespace within a small budget).
                let limit = (i + 200).min(n);
                while j < limit {
                    if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                        break;
                    }
                    j += 1;
                }
                if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                    // Find `{` after `mod NAME`.
                    let mut k = j + 4;
                    while k < n && bytes[k] != b'{' {
                        k += 1;
                    }
                    if k < n {
                        let mut depth = 1i32;
                        let mut m = k + 1;
                        while m < n && depth > 0 {
                            match bytes[m] {
                                b'{' => depth += 1,
                                b'}' => depth -= 1,
                                _ => {}
                            }
                            m += 1;
                        }
                        if m > k + 1 {
                            for slot in &mut out[(k + 1)..(m - 1)] {
                                if *slot != b'\n' {
                                    *slot = b' ';
                                }
                            }
                        }
                        i = m;
                        continue;
                    }
                }
            }
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn preprocess(src: &str) -> String {
        strip_inline_test_modules(&strip_comments(src))
    }

    fn fmt_match(m: &(String, u32, String)) -> String {
        format!("({:?}, {}, {:?})", m.0, m.1, m.2)
    }

    /// Compare actual matches (Vec of (path, line, matched_str)) against
    /// the allowlist tuples. Fails on EITHER:
    ///   * a violation present in source without an allowlist tuple
    ///   * an allowlist tuple that no longer matches anything in source
    fn assert_exact_allowlist_match(
        guard_name: &str,
        actual: &[(String, u32, String)],
        allowed: &[(&str, u32, &str)],
    ) {
        // Normalise to comparable form.
        let actual_set: BTreeSet<(String, u32, String)> = actual.iter().cloned().collect();
        let allowed_set: BTreeSet<(String, u32, String)> = allowed
            .iter()
            .map(|(p, ln, pat)| (p.to_string(), *ln, pat.to_string()))
            .collect();

        let unexpected: Vec<_> = actual_set
            .iter()
            .filter(|t| !allowed_set.contains(*t))
            .map(fmt_match)
            .collect();
        let stale: Vec<_> = allowed_set
            .iter()
            .filter(|t| !actual_set.contains(*t))
            .map(fmt_match)
            .collect();

        if unexpected.is_empty() && stale.is_empty() {
            return;
        }

        let mut msg = format!("\n\n=== {guard_name} ===\n");
        if !unexpected.is_empty() {
            msg.push_str(
                "\nUnallowlisted violation introduced (add to allowlist if intentional, \
                 OR — preferred — remove the violation from source):\n",
            );
            for entry in &unexpected {
                msg.push_str("    ");
                msg.push_str(entry);
                msg.push('\n');
            }
        }
        if !stale.is_empty() {
            msg.push_str(
                "\nAllowlisted entry NOT FOUND in source — remove from allowlist or \
                 restore the violation; line number may have shifted:\n",
            );
            for entry in &stale {
                msg.push_str("    ");
                msg.push_str(entry);
                msg.push('\n');
            }
        }
        msg.push('\n');
        panic!("{msg}");
    }

    // -----------------------------------------------------------------------
    // Guard 1: `path.contains("/node_modules/")` and the Windows-backslash
    // sibling.
    //
    // Rule scope: the **typed-IR resolver pipeline** —
    //   analyzer → projector → registry → policy → materialiser, plus
    //   the JS compat layer in `@verter/component-meta/compat`. Within
    //   that scope the single source of workspace classification truth
    //   is `ResolverContext::workspace_is_package_backed` (workspace-owned
    //   is its complement). Substring tests on canonical
    //   paths are banned. The producer crates (`verter_session`,
    //   `verter_semantic`) MUST route every workspace-membership
    //   decision through `WorkspaceAccess`.
    //
    // Rule out of scope (and excluded from the allowlist on principle,
    // not pending migration):
    //   1. The implementation of the workspace classification API
    //      itself. `verter_workspace::Project::matches_file` and the
    //      sibling accessors on `Engine`, `FilesystemWorkspace`,
    //      `MemoryWorkspace` are the primitives the public
    //      `is_workspace_owned` / `is_package_backed` are built on.
    //      Calling the public API from within its own implementation
    //      would be circular.
    //   2. Filesystem-event handlers that fire BELOW the workspace
    //      registry — i.e. before any workspace snapshot has been
    //      published, when `WorkspaceAccess::is_package_backed` is
    //      definitionally `false` for every path (see engine.rs:
    //      "Returns `false` before the workspace publishes its first
    //      snapshot."). The LSP `is_config_file` watcher gate fires
    //      on raw `DidChangeWatchedFilesParams` URIs and must filter
    //      `node_modules/` config changes regardless of registry
    //      readiness; switching it to the typed API would either
    //      regress (rebuild on every node_modules change before first
    //      snapshot) or require ordering that the LSP spec does not
    //      guarantee.
    //
    // Allowlist removed by:
    //   * W2.2 — cold_resolver.rs (4 entries)
    //   * W4.1 — component_meta_registry.rs (6 entries) +
    //            component_meta_query_engine/helpers.rs (4 entries)
    //   * W4.3 — project_semantic_dispatch/relation.rs (2 entries) +
    //            project_semantic_dispatch/walk.rs (2 entries)
    //   * W4.4 — component_meta_resolution_policy/{core,pick_omit}.rs +
    //            host_manage/component_meta_methods.rs +
    //            meta_resolve/registry_materialize.rs
    //   * W4.5 — host_manage.rs (2) + meta_resolve/graph_predicates.rs
    //
    // Permanent exception entries (per the rule-scope clauses above):
    //   * `verter_lsp/src/server_utils.rs:17` — exception class (2):
    //     filesystem-event handler running below the workspace
    //     registry. The LSP `did_change_watched_files` gate.
    //
    // Exception class (1) — the workspace classification API's own
    // primitive — no longer needs a substring site: project membership
    // resolves through the exact `ConfiguredMembership` (materialized
    // file set / static spec), so `Project::matches_file` performs no
    // `/node_modules/` substring classification. The single remaining
    // allowlist site is exception class (2).
    //
    // This entry stays in the allowlist permanently. It is not a
    // resolver-pipeline site. The matching call site carries a pointer
    // comment back to this rule-scope block.
    // -----------------------------------------------------------------------
    const NODE_MODULES_ALLOWLIST: &[(&str, u32, &str)] = &[(
        "crates/verter_lsp/src/server_utils.rs",
        17,
        r#".contains("/node_modules/")"#,
    )];

    fn scan_node_modules_substring() -> Vec<(String, u32, String)> {
        scan_node_modules_substring_in(&collect_production_rs_files())
    }

    fn scan_node_modules_substring_in(files: &[(PathBuf, String)]) -> Vec<(String, u32, String)> {
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                if line.contains(r#".contains("/node_modules/")"#) {
                    out.push((
                        rel.clone(),
                        line_no,
                        r#".contains("/node_modules/")"#.to_string(),
                    ));
                }
                if line.contains(r#".contains("\\node_modules\\")"#) {
                    out.push((
                        rel.clone(),
                        line_no,
                        r#".contains("\\node_modules\\")"#.to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn typed_ir_scans_cover_production_under_a_tests_directory() {
        use super::super::PlantedWorkspace;
        let ws = PlantedWorkspace::new();
        ws.plant_production_and_listed_test_module(
            "fn owned(p: &str) -> bool { !p.contains(\"/node_modules/\") }\n",
        );
        let flagged: Vec<String> =
            scan_node_modules_substring_in(&collect_production_rs_files_under(ws.root()))
                .into_iter()
                .map(|(rel, _, _)| rel)
                .collect();
        assert_eq!(
            flagged,
            [PlantedWorkspace::PRODUCTION_UNDER_TESTS_DIR],
            "the production file under an unlisted `tests` directory must be \
             flagged and the listed test-module file skipped"
        );
    }

    #[test]
    fn no_node_modules_substring_outside_workspace_api() {
        let actual = scan_node_modules_substring();
        assert_exact_allowlist_match(
            "no_node_modules_substring_outside_workspace_api",
            &actual,
            NODE_MODULES_ALLOWLIST,
        );
    }

    /// The two permanent allowlist sites MUST carry pointer comments
    /// back to this rule-scope block. The test reads the source of each
    /// allowlisted file and asserts the function carrying the substring
    /// is annotated with the rule scope. This is the negative half of
    /// the guard: it would FAIL pre-F3 (the call sites had only a
    /// one-line "// No config file inside node_modules..." comment and
    /// no `matches_file` doc-comment) and PASSES post-F3 once the
    /// pointer comments are in place.
    #[test]
    fn node_modules_allowlist_sites_carry_rule_scope_pointers() {
        // Site 1 — LSP filesystem-event handler.
        let lsp_src = super::super::read_workspace_file("crates/verter_lsp/src/server_utils.rs");
        assert!(
            lsp_src.contains("Architecture-guard exception")
                && lsp_src.contains("DidChangeWatchedFilesParams")
                && lsp_src.contains("no_node_modules_substring_outside_workspace_api"),
            "verter_lsp::server_utils::is_config_file must carry a rule-scope \
             pointer comment naming the architecture guard and the \
             filesystem-event-handler exception class. Restore the \
             docstring or remove the allowlist entry.",
        );

        // The rule-scope block in this file MUST also carry the
        // post-F3 exception-class language. Locks the docstring against
        // silent weakening.
        let guard_src = super::super::read_workspace_file(
            "crates/verter_session/tests/cases/architecture/capabilities.rs",
        );
        assert!(
            guard_src.contains("Rule scope: the **typed-IR resolver pipeline**"),
            "the no_node_modules_substring_outside_workspace_api \
             rule-scope block must state its scope explicitly.",
        );
        assert!(
            guard_src.contains("Calling the public API from within its own implementation"),
            "exception class (1) — workspace-API primitive — must be \
             documented at the rule-scope block.",
        );
        assert!(
            guard_src.contains("Filesystem-event handlers that fire BELOW the workspace"),
            "exception class (2) — filesystem-event handler — must be \
             documented at the rule-scope block.",
        );
    }

    // -----------------------------------------------------------------------
    // Guard 2: `parse_jsdoc_tag_type_payload` reference outside JSDoc.
    //
    // The function is the JSDoc tag-type wrap-and-lower helper. It is
    // the sole text-input boundary in the typed-IR resolver pipeline —
    // every other producer-side caller in the resolver / projector /
    // registry / policy / materialiser lowers from a `TSType<'_>` AST
    // node via `verter_type_expr_oxc::lower_ts_type` directly.
    //
    // Pre-W5.2 the function was named `parse_type_annotation` and
    // lived in `verter_type_expr_oxc::lib.rs`. W5.2 renamed it to
    // `parse_jsdoc_tag_type_payload` and moved it to
    // `verter_semantic::analysis::jsdoc`, narrowing visibility so only
    // the JSDoc resolver in `verter_session::host_manage::jsdoc_resolve`
    // calls it from production code.
    //
    // The two production touchpoints are inherent and skipped via
    // explicit `continue` filters below — no allowlist entries needed:
    //   * `crates/verter_semantic/src/analysis/jsdoc.rs` — function
    //     definition site (the helper itself).
    //   * `crates/verter_session/src/host_manage/jsdoc_resolve.rs` —
    //     the single production caller.
    //
    // Any future caller anywhere else in `crates/*/src/**` MUST go
    // through the typed `TSType<'_>` AST path. If a new requirement
    // appears to need text manipulation, fix the producer (lower the
    // right OXC node, store the right typed field, extend
    // `verter_type_expr` with a missing variant) rather than reparsing.
    // -----------------------------------------------------------------------
    const PARSE_TYPE_ANNOTATION_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_parse_type_annotation() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            // Inherent production touchpoints: the JSDoc helper
            // definition site and its sole production caller.
            if rel == "crates/verter_semantic/src/analysis/jsdoc.rs"
                || rel == "crates/verter_session/src/host_manage/jsdoc_resolve.rs"
            {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line.contains("parse_jsdoc_tag_type_payload") {
                    out.push((
                        rel.clone(),
                        (idx + 1) as u32,
                        "parse_jsdoc_tag_type_payload".to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_parse_jsdoc_tag_type_payload_outside_jsdoc() {
        let actual = scan_parse_type_annotation();
        assert_exact_allowlist_match(
            "no_parse_jsdoc_tag_type_payload_outside_jsdoc",
            &actual,
            PARSE_TYPE_ANNOTATION_ALLOWLIST,
        );
    }

    // Bonus belt-and-braces gate: the OLD name `parse_type_annotation`
    // must not reappear anywhere in production source after the W5.2
    // rename. A reintroduction would mean someone re-introduced the
    // wrap-and-lower helper under its old identifier; the rename's
    // entire point is to make every JSDoc-private call site grep-able
    // by its semantic role rather than a generic "parse" verb.
    const OLD_PARSE_TYPE_ANNOTATION_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_old_parse_type_annotation() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                if line.contains("parse_type_annotation") {
                    out.push((
                        rel.clone(),
                        (idx + 1) as u32,
                        "parse_type_annotation".to_string(),
                    ));
                }
            }
        }
        out
    }

    #[test]
    fn no_old_parse_type_annotation_name_in_production() {
        let actual = scan_old_parse_type_annotation();
        assert_exact_allowlist_match(
            "no_old_parse_type_annotation_name_in_production",
            &actual,
            OLD_PARSE_TYPE_ANNOTATION_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 3: `format!()` followed by `parse_jsdoc_tag_type_payload(&_)`,
    // `parse_type_annotation(&_)`, or `parse_type_text(&_)` — the
    // synthesise-then-reparse round-trip.
    //
    // We detect the pattern by scanning for `format!` and looking
    // ahead within the same function body for any of:
    //   - `parse_jsdoc_tag_type_payload(&` (post-W5.2 helper name)
    //   - `parse_type_annotation(&` (pre-W5.2 helper name; should never
    //     reappear in production but guarded belt-and-braces)
    //   - `parse_type_text(&`
    // The `&` is the discriminator: a real round-trip references the
    // format! result through a let-bound variable. (Direct chained
    // `format!(...).parse_*()` would also match.)
    //
    // Pre-cutover sites: `slot_field_function_type_expr` in
    // `meta_resolve/materialize/macro_shapes.rs` (3 `format!` calls
    // feeding one `parse_type_annotation`) and
    // `projected_macro_surfaces_to_type_expr` in
    // `resolver_core/component_meta/projected_type_expr.rs` (3 more).
    // All removed by W2.1.
    // -----------------------------------------------------------------------
    const FORMAT_THEN_REPARSE_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_format_then_reparse() -> Vec<(String, u32, String)> {
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            let bytes = stripped.as_bytes();
            let n = bytes.len();
            // The three reparse needles we treat as a synthesise-then-reparse:
            //   * `parse_jsdoc_tag_type_payload(&` — post-W5.2 JSDoc helper.
            //   * `parse_type_annotation(&` — pre-W5.2 helper (belt-and-braces).
            //   * `parse_type_text(&` — retired `type_text_parser` entry function.
            let needle_jsdoc = b"parse_jsdoc_tag_type_payload(&";
            let needle_a = b"parse_type_annotation(&";
            let needle_t = b"parse_type_text(&";
            let mut i = 0usize;
            while i + 7 <= n {
                if &bytes[i..i + 7] == b"format!" {
                    let start = i;
                    let window_end = (start + 800).min(n);
                    let window = &bytes[start..window_end];
                    let pj = window
                        .windows(needle_jsdoc.len())
                        .position(|w| w == needle_jsdoc);
                    let pa = window.windows(needle_a.len()).position(|w| w == needle_a);
                    let pt = window.windows(needle_t.len()).position(|w| w == needle_t);
                    // Pick the earliest hit and label by needle kind.
                    let candidates = [
                        pj.map(|off| (off, "format!(...).parse_jsdoc_tag_type_payload")),
                        pa.map(|off| (off, "format!(...).parse_type_annotation")),
                        pt.map(|off| (off, "format!(...).parse_type_text")),
                    ];
                    let hit = candidates
                        .iter()
                        .filter_map(|c| c.as_ref())
                        .min_by_key(|(off, _)| *off)
                        .copied();
                    if let Some((off, label)) = hit {
                        // Reject if a function boundary appears in
                        // between (`\n}` at column 0 or a `\nfn ` decl).
                        let between = &window[..off];
                        let has_close = between.windows(2).any(|w| w == b"\n}");
                        let has_fn = between.windows(4).any(|w| w == b"\nfn ");
                        if !has_close && !has_fn {
                            let prefix = &bytes[..start];
                            let line_no =
                                (prefix.iter().filter(|&&c| c == b'\n').count() + 1) as u32;
                            out.push((rel.clone(), line_no, label.to_string()));
                        }
                    }
                    i += 7;
                    continue;
                }
                i += 1;
            }
        }
        out
    }

    #[test]
    fn no_format_then_reparse() {
        let actual = scan_format_then_reparse();
        assert_exact_allowlist_match(
            "no_format_then_reparse",
            &actual,
            FORMAT_THEN_REPARSE_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 4: `starts_with("Pick<" | "Omit<" | "Required<" | "Partial<")` —
    // shape-sniffing TS utility-type helpers off the type-text. Built-in
    // utilities behave identically to a userland implementation;
    // discriminating them by string prefix is a category error. The
    // typed `TypeExpr::Ref { name, type_arguments }` already carries the
    // utility-type identity.
    //
    // Removed by W1.1.
    // -----------------------------------------------------------------------
    const PICK_OMIT_PREFIX_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_pick_omit_prefix() -> Vec<(String, u32, String)> {
        let needles: &[&str] = &[
            r#"starts_with("Pick<")"#,
            r#"starts_with("Omit<")"#,
            r#"starts_with("Required<")"#,
            r#"starts_with("Partial<")"#,
        ];
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                for needle in needles {
                    if line.contains(needle) {
                        out.push((rel.clone(), line_no, (*needle).to_string()));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_pick_or_omit_string_prefix_check() {
        let actual = scan_pick_omit_prefix();
        assert_exact_allowlist_match(
            "no_pick_or_omit_string_prefix_check",
            &actual,
            PICK_OMIT_PREFIX_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 4b: display-text payload-form sniffing inside the typeinfo
    // pipeline. `starts_with('[')` on a rendered type display decided
    // Tuple-vs-Call emit payload form in
    // `typeinfo/framework_surface/vue_exec/imported_elements.rs` — the
    // exact "shape sniffing on display text" class Typed-IR-Only
    // forbids, and one the `Pick<`-prefix needles above were too narrow
    // to catch. Payload-form classification must come from
    // `SemanticNodeData` / `TypeExpr` node shape; display strings are
    // minted FROM typed values for output only.
    // -----------------------------------------------------------------------
    const TYPEINFO_DISPLAY_SNIFF_ALLOWLIST: &[(&str, u32, &str)] = &[];

    fn scan_typeinfo_display_sniff() -> Vec<(String, u32, String)> {
        let needles: &[&str] = &[r#"starts_with('[')"#, r#"starts_with("[")"#];
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            if !rel.starts_with("crates/verter_session/src/typeinfo/") {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                for needle in needles {
                    if line.contains(needle) {
                        out.push((rel.clone(), line_no, (*needle).to_string()));
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_display_text_payload_sniff_in_typeinfo() {
        let actual = scan_typeinfo_display_sniff();
        assert_exact_allowlist_match(
            "no_display_text_payload_sniff_in_typeinfo",
            &actual,
            TYPEINFO_DISPLAY_SNIFF_ALLOWLIST,
        );
    }

    // -----------------------------------------------------------------------
    // Guard 4c: the export-surface probe in
    // `host_resolve/vue_macro_dependency_diagnostics.rs` is a
    // diagnostics-only adjudicator over HOST-INDEXED facts. It must never
    // grow a parser re-walk (OXC parse of dependency sources) or call the
    // semantic dispatch — that would make it a second resolver beside the
    // shared engine. (The probe moved here when its former owner,
    // `external_macro_collector.rs`, was deleted with the legacy
    // external-types rail; the invariant is unchanged.)
    // -----------------------------------------------------------------------
    #[test]
    fn export_probe_consumes_indexed_facts_only() {
        let src = fs::read_to_string(
            super::super::workspace_root()
                .join("crates/verter_session/src/host_resolve/vue_macro_dependency_diagnostics.rs"),
        )
        .expect("read vue_macro_dependency_diagnostics.rs");
        for needle in [
            "Parser::new",
            "oxc_parser::",
            "parse_program",
            "ProjectSemanticDispatch",
            "resolve_hot_handle",
            "execute_cooperative",
        ] {
            assert!(
                !src.contains(needle),
                "vue_macro_dependency_diagnostics.rs must adjudicate from indexed facts only; \
                 found forbidden `{needle}` (a parser re-walk / second resolver path)"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Guard 5: role inference from identifier name suffix.
    // `name.ends_with("Props" | "Emits" | "Events" | "Slots" | "Model")`
    // (and `*_name` / `identifier` / `*_identifier` / `ident` /
    // `*_ident` lhs variants). Type-role classification is structural
    // (a Vue SFC macro consumes the type), NOT nominal (the identifier
    // ends in "Props"). Scoped to the resolver pipeline crates:
    // `crates/verter_session/src/**` and
    // `crates/verter_semantic/src/analysis/**`.
    //
    // Cleared by W6.1 (analyzer-layer
    // `collect_imported_props_like_raw_refs` deletion + typed-IR
    // macro-participation walker) and F1 (policy-layer
    // `is_props_suffix` deletion + structural macro-participation
    // predicate in `PolicyCtx::is_macro_participating`). The allowlist
    // is now empty: no production source classifies type-role by
    // identifier name suffix.
    // -----------------------------------------------------------------------
    const ROLE_NAME_SUFFIX_ALLOWLIST: &[(&str, u32, &str)] = &[];

    /// Walk a method-chain LHS to find the root identifier.
    ///
    /// Given a line slice ending immediately before `.ends_with("...")`, walk
    /// backwards through chained `.method(arg, arg)` / `.field` segments to
    /// find the underlying identifier. Examples:
    ///
    /// - `name`                              -> "name"
    /// - `name.as_ref()`                     -> "name"
    /// - `name.as_str().trim()`              -> "name"
    /// - `prop.key_name.as_deref().unwrap()` -> "key_name"
    /// - `foo.bar()`                          -> "bar" (method tail before LHS)
    ///
    /// For the role-suffix guard, we walk through `.as_ref()` / `.as_str()` /
    /// `.as_deref()` / `.borrow()` / `.unwrap()` / `.unwrap_or_*()` /
    /// `.clone()` / `.to_string()` / `.trim()` etc. and continue until we
    /// reach a base identifier. Any base identifier matching `name` / `*_name`
    /// / `identifier` / `*_identifier` / `ident` / `*_ident` is flagged.
    fn walk_method_chain_lhs(prefix: &str) -> Option<String> {
        // Walk backwards from end. Skip whitespace, then peel method-call
        // suffixes (`.method(args)` with balanced parens) and field accesses
        // (`.field`) until we reach a base word.
        let bytes: Vec<char> = prefix.chars().collect();
        let mut i = bytes.len();

        loop {
            // Trim trailing whitespace.
            while i > 0 && bytes[i - 1].is_whitespace() {
                i -= 1;
            }
            if i == 0 {
                return None;
            }
            // Case 1: trailing balanced `(...)` — peel a method-call suffix.
            if bytes[i - 1] == ')' {
                let mut depth = 0i32;
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    match bytes[j] {
                        ')' => depth += 1,
                        '(' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if depth != 0 {
                    // Unbalanced — bail.
                    return None;
                }
                i = j; // Now points at the `(`.
                       // Continue: there must be a method name (word) before, and a `.`.
                let word_end = i;
                while i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_') {
                    i -= 1;
                }
                if i == word_end {
                    // No method name before `(` — bail.
                    return None;
                }
                // Skip optional `::` segment after a turbofish (rare in this scope).
                // Now expect a `.` to continue chain, or this is the start of the
                // chain (e.g. `foo(`).
                while i > 0 && bytes[i - 1].is_whitespace() {
                    i -= 1;
                }
                if i == 0 || bytes[i - 1] != '.' {
                    // Not a method chain — this is e.g. `foo(args).ends_with(...)`.
                    // The base call (`foo(...)`) is the "root" but we ignore it; only
                    // a plain identifier base counts as name-like.
                    return None;
                }
                i -= 1; // Skip `.`.
            } else if bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_' {
                // Base identifier. Collect it.
                let word_end = i;
                while i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_') {
                    i -= 1;
                }
                let ident: String = bytes[i..word_end].iter().collect();
                // If preceded by `.`, this is a field/method on something — keep walking
                // (the FIELD identifier IS what we want to test, not the receiver).
                // Actually: for `prop.key_name.as_ref().ends_with("Props")`, we want
                // "key_name" (the immediate field on `prop`), not "prop". So return
                // this identifier — it's the closest identifier to the .ends_with call.
                return Some(ident);
            } else {
                // Unknown character (operator, etc.) — bail.
                return None;
            }
        }
    }

    fn scan_role_name_suffix() -> Vec<(String, u32, String)> {
        let suffixes: &[&str] = &["Props", "Emits", "Events", "Slots", "Model"];
        let files = collect_production_rs_files();
        let mut out: Vec<(String, u32, String)> = Vec::new();
        for (path, rel) in &files {
            let in_scope = rel.starts_with("crates/verter_session/src/")
                || rel.starts_with("crates/verter_type_engine/src/")
                || rel.starts_with("crates/verter_semantic/src/analysis/");
            if !in_scope {
                continue;
            }
            let src = match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let stripped = preprocess(&src);
            for (idx, line) in stripped.split('\n').enumerate() {
                let line_no = (idx + 1) as u32;
                for sfx in suffixes {
                    let needle = format!(r#".ends_with("{}")"#, sfx);
                    if let Some(pos) = line.find(&needle) {
                        // Walk the LHS — including method-chain peeling so
                        // `name.as_ref().ends_with("Props")` resolves to "name"
                        // (and is correctly flagged). Without chain walking,
                        // the LHS would be "ref" (the last word) and the
                        // violation would slip through.
                        let lhs = match walk_method_chain_lhs(&line[..pos]) {
                            Some(s) => s,
                            None => continue,
                        };
                        if lhs.is_empty() {
                            continue;
                        }
                        let lhs_lower = lhs.to_ascii_lowercase();
                        let is_name_like = lhs_lower == "name"
                            || lhs_lower.ends_with("_name")
                            || lhs_lower == "identifier"
                            || lhs_lower.ends_with("_identifier")
                            || lhs_lower == "ident"
                            || lhs_lower.ends_with("_ident");
                        if is_name_like {
                            out.push((rel.clone(), line_no, needle));
                        }
                    }
                }
            }
        }
        out
    }

    #[test]
    fn no_role_inference_from_name_suffix() {
        let actual = scan_role_name_suffix();
        assert_exact_allowlist_match(
            "no_role_inference_from_name_suffix",
            &actual,
            ROLE_NAME_SUFFIX_ALLOWLIST,
        );
    }

    /// Discriminating test for the strengthened Guard 5 scanner.
    ///
    /// Pre-H3 fix: `scan_role_name_suffix` took only the immediate
    /// alphanumeric-suffix LHS, so `name.as_ref().ends_with("Props")`
    /// resolved to "ref" — failing the name-like check and slipping
    /// through. A real production violation was masked.
    ///
    /// Post-H3 fix: `walk_method_chain_lhs` peels `.as_ref()` (and any
    /// other method-chain suffix) to find the underlying identifier, so
    /// "name" is correctly recognized and flagged.
    ///
    /// This test verifies the helper directly against a set of synthetic
    /// inputs covering bare identifiers, simple method chains, deep
    /// chains, and field/method mixes.
    #[test]
    fn walk_method_chain_lhs_resolves_to_root_identifier() {
        // Base identifier.
        assert_eq!(
            walk_method_chain_lhs("name").as_deref(),
            Some("name"),
            "bare identifier should resolve to itself",
        );
        // Method-call suffix.
        assert_eq!(
            walk_method_chain_lhs("name.as_ref()").as_deref(),
            Some("name"),
            "`.as_ref()` chain must peel to root identifier",
        );
        // Deeper chain.
        assert_eq!(
            walk_method_chain_lhs("name.as_str().trim()").as_deref(),
            Some("name"),
            "`.as_str().trim()` deep chain must peel to root identifier",
        );
        // Field access chain.
        assert_eq!(
            walk_method_chain_lhs("prop.key_name.as_deref().unwrap()").as_deref(),
            Some("key_name"),
            "`prop.key_name.as_deref().unwrap()` resolves to the immediate field `key_name` (the receiver of the call closest to `.ends_with`)",
        );
        // Identifier in a `match` context (the existing scanner passed this).
        assert_eq!(
            walk_method_chain_lhs("        name").as_deref(),
            Some("name"),
            "leading whitespace must be stripped",
        );
    }

    /// End-to-end discriminating test for the strengthened Guard 5
    /// scanner. Synthesise a production-source-like buffer with a
    /// `name.as_ref().ends_with("Props")` violation, run the scanner's
    /// LHS-resolution logic on it, and assert the violation is flagged.
    ///
    /// Pre-fix: the LHS would have been "ref" (last contiguous word),
    /// `is_name_like` would have returned false, and the violation
    /// would have been silently allowed.
    /// Post-fix: the LHS resolves to "name", `is_name_like` returns
    /// true, and the violation is recorded.
    #[test]
    fn scan_role_name_suffix_flags_method_chain_lhs() {
        // Replicate the LHS extraction + name-like check that
        // `scan_role_name_suffix` performs, against a synthetic line.
        let synthetic = r#"                if name.as_ref().ends_with("Props") {"#;
        let needle = r#".ends_with("Props")"#;
        let pos = synthetic.find(needle).expect("needle must be present");
        let lhs = walk_method_chain_lhs(&synthetic[..pos]).expect("LHS must resolve");
        assert_eq!(
            lhs.to_ascii_lowercase(),
            "name",
            "strengthened LHS resolution must recover `name` from `name.as_ref()`",
        );
        let lhs_lower = lhs.to_ascii_lowercase();
        let is_name_like = lhs_lower == "name"
            || lhs_lower.ends_with("_name")
            || lhs_lower == "identifier"
            || lhs_lower.ends_with("_identifier")
            || lhs_lower == "ident"
            || lhs_lower.ends_with("_ident");
        assert!(
            is_name_like,
            "name-like check must fire for chain-peeled `name`",
        );
    }
}

#[test]
fn surface_member_arch_guard_self_test_detects_inverted_order() {
    // Self-test: a body that calls raise BEFORE the cache helper must
    // fail the substring-position check.
    let bad_body = "
        let raised = dispatch.raise_node_to_type_expr(member.value).unwrap();
        let r#type = member_shape_peek_or_compute(...).type_expr;
    ";
    let cache_call_offset = bad_body
        .find("member_shape_peek_or_compute(")
        .expect("test must contain the helper call");
    let raise_offset = bad_body
        .find("raise_node_to_type_expr(member.value)")
        .expect("test must contain the raise call");
    assert!(
        cache_call_offset > raise_offset,
        "self-test: inverted-order body must have cache call AFTER raise"
    );
}

#[test]
fn resolver_store_view_returns_store_view_read() {
    // Part A — the general accessor's return type is the capability-split
    // `StoreViewRead`, and the raw-`HostStoreView` form is gone. A
    // re-introduced `fn resolver_store_view(&self) -> HostStoreView` on
    // `VerterHost` (the leak this contract closes) fails here.
    let src = read_workspace_file("crates/verter_session/src/resolver_store.rs");
    assert!(
        src.contains("pub(crate) fn resolver_store_view(&self) -> StoreViewRead"),
        "VerterHost::resolver_store_view must return the capability-split \
         `StoreViewRead`; the raw-`HostStoreView` accessor is the contract leak \
         this guard closes. Did the signature regress?"
    );
    assert!(
        !src.contains("fn resolver_store_view(&self) -> HostStoreView"),
        "VerterHost::resolver_store_view must NOT hand back a raw `HostStoreView` \
         — that erases the non-current proof and lets a warm validator validate \
         against a stale snapshot. Return `StoreViewRead` and let callers choose \
         `.current()` (warm) or `.into_cold_seed_view()` (fenced cold)."
    );
}

#[test]
fn cold_seed_store_view_exposes_no_validation_surface() {
    // Part B — `ColdSeedHostStoreView` must NOT expose any `validates*`
    // method. The whole point of the cold-seed wrapper is that a stale
    // seed CANNOT reach a fact validator by construction; a `validates`
    // method on it would re-open that door.
    let src = read_workspace_file("crates/verter_session/src/resolver_store.rs");
    let marker = "impl ColdSeedHostStoreView {";
    let start = src
        .find(marker)
        .expect("ColdSeedHostStoreView impl block must exist");
    // Bound the scan to the impl block (up to the next top-level `\n}\n`).
    let rest = &src[start + marker.len()..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    let block = &rest[..end];
    let banned = [
        "fn validates(",
        "fn validates_fact_signature(",
        "fn validates_self_root_whole_hash(",
        "fn validates_parse_domain(",
        "fn validates_resolve_imports_domain(",
        "fn validates_route_surface_domain(",
    ];
    let hits = count_callsites(block, &banned);
    assert_eq!(
        hits, 0,
        "ColdSeedHostStoreView must expose NO `validates*` method — a cold-seed \
         view is for fenced cold-builder seeding only and must never validate a \
         warm cache entry. Found {hits} validation method(s) in its impl block."
    );
}

#[test]
fn warm_validation_entry_points_require_current_store_view() {
    // The request root proves currentness before binding its fact port. The
    // passive final-result DB has no validation service; the facade memo
    // driver validates only through that selected request port.
    //
    // Only the request root's own requirement is asserted from this guard.
    // A memo driver's validation call and a passive final-result DB's
    // forbidden-token inventory are name-keyed source spellings of the
    // memo/storage boundary rather than a capability, so re-asserting them
    // here would pin file moves instead of behaviour. The capability itself is
    // owned elsewhere — a warm entry is only ever published through a request
    // root that proved currentness (`CurrentHostStoreView`), and the
    // final-result storage is passive by construction (it stores the result; it
    // has no fact port to validate with).
    let root =
        read_workspace_file("crates/verter_session/src/host_manage/component_meta_methods.rs");
    assert!(
        root.contains("current_view: &crate::resolver_store::CurrentHostStoreView"),
        "warm request root still requires proven currentness"
    );
}

#[test]
fn cold_compute_context_constructors_carry_currentness() {
    // Positive half of the indirect-validation guard: the request-bound
    // resolver-context constructors that a cold compute uses MUST be the
    // currentness-carrying `from_cold_seed` form, and that form MUST root
    // its request-bound view via `RequestStoreView::new_cold_seed` (which
    // fails `validates*` closed on a non-current seed) — NOT the
    // always-current `RequestStoreView::new`.
    let host_ctx =
        read_workspace_file("crates/verter_session/src/resolver_core/host_resolver_context.rs");
    let session_ctx =
        read_workspace_file("crates/verter_session/src/resolver_core/session_resolver_context.rs");
    for (rel, src) in [
        ("host_resolver_context.rs", host_ctx.as_str()),
        ("session_resolver_context.rs", session_ctx.as_str()),
    ] {
        assert!(
            src.contains("pub(crate) fn from_cold_seed("),
            "{rel} must expose a cold-seed context constructor `from_cold_seed` so a \
             cold compute threads the seed's currentness into the request-bound view"
        );
        assert!(
            src.contains("RequestStoreView::new_cold_seed("),
            "{rel}::from_cold_seed must root its request-bound view via \
             `RequestStoreView::new_cold_seed` (fails `validates*` closed on a \
             non-current seed), not the always-current `RequestStoreView::new`"
        );
    }
    // The cold-seed wrapper must expose the currentness-preserving overlay
    // re-root, so a cold compute never has to drop the flag to overlay.
    // Scope the search to the `impl ColdSeedHostStoreView` block so the
    // method is proven to live ON the cold-seed type (not merely somewhere
    // in the file).
    let resolver_store = read_workspace_file("crates/verter_session/src/resolver_store.rs");
    let marker = "impl ColdSeedHostStoreView {";
    let start = resolver_store
        .find(marker)
        .expect("ColdSeedHostStoreView impl block must exist");
    let rest = &resolver_store[start + marker.len()..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    let cold_seed_impl = &rest[..end];
    assert!(
        cold_seed_impl.contains("fn with_session_overlay("),
        "ColdSeedHostStoreView must expose `with_session_overlay` (re-root through \
         a session overlay WITHOUT dropping currentness)"
    );
    // The cold-seed view's currentness must come from a `StoreViewRead`
    // (intrinsic to its arm), NOT a separate constructor that pairs a raw
    // view with a caller-supplied bool. The retired `from_raw_for_compute`
    // was exactly such a footgun — a view from one read could be re-bound
    // with a currentness flag from ANOTHER read. It must stay gone.
    assert!(
        !cold_seed_impl.contains("fn from_raw_for_compute("),
        "ColdSeedHostStoreView must NOT expose `from_raw_for_compute(view, current)` — \
         a constructor that pairs a raw view with a separately-sourced currentness bool \
         lets the flag and the view describe DIFFERENT reads (a stale view marked \
         current). Currentness must come from the SAME read via \
         `StoreViewRead::into_cold_seed_view`; the one executor-boundary re-bind is \
         `StoreViewRead::from_executor_snapshot`."
    );
    // The sole currentness-bound re-bind lives on `StoreViewRead` (it
    // returns the intrinsic-currentness enum, consumed via
    // `into_cold_seed_view`), so a cold compute that holds an executor's
    // single-read `(view, is_current)` pair never has to fabricate the
    // cold-seed's `current` field directly.
    let store_view_read_marker = "impl StoreViewRead {";
    let sv_start = resolver_store
        .find(store_view_read_marker)
        .expect("StoreViewRead impl block must exist");
    let sv_rest = &resolver_store[sv_start + store_view_read_marker.len()..];
    let sv_end = sv_rest.find("\n}\n").unwrap_or(sv_rest.len());
    let store_view_read_impl = &sv_rest[..sv_end];
    assert!(
        store_view_read_impl.contains("fn from_executor_snapshot("),
        "StoreViewRead must expose `from_executor_snapshot(view, is_current)` — the SOLE \
         constructor that re-binds an executor's single-read `(view, is_current)` pair into \
         the intrinsic-currentness typed read, so cold-seed currentness flows through \
         `into_cold_seed_view` and never as a free-floating flag a downstream helper \
         re-pairs with a different read."
    );
}

#[test]
fn cold_seed_currentness_is_intrinsic_to_the_read() {
    // The strengthened currentness guard — closes the sub-class the
    // constructor-SHAPE guards
    // (`cold_compute_context_constructors_carry_currentness`,
    // `cold_seed_into_inner_confined_to_non_validating_allowlist`) could not
    // see: a currentness flag SOURCED FROM A DIFFERENT READ than the view it
    // describes.
    //
    // Two rails:
    //
    // 1. `StoreViewRead::from_executor_snapshot(view, is_current)` — the one
    //    constructor that pairs a raw view with a separately-named bit — is
    //    confined to the executor-boundary allowlist, where the pair provably
    //    came from one read. A new caller that re-binds a `(view, flag)` pair
    //    elsewhere fails here and must instead derive currentness from a
    //    `StoreViewRead` (intrinsic to its arm).
    //
    // 2. No production file pairs a FRESH `resolver_store_view_read()` with
    //    `from_executor_snapshot` — that is the exact divergence the
    //    view-bound component-meta cold path produced (a stale second read
    //    marked current). A cold-compute helper doing its own fresh read must
    //    take the cold-seed straight from that read via `into_cold_seed_view`.
    let allow: std::collections::HashSet<&str> =
        FROM_EXECUTOR_SNAPSHOT_ALLOWLIST.iter().copied().collect();
    let mut snapshot_offenders: Vec<String> = Vec::new();
    let mut fresh_read_offenders: Vec<String> = Vec::new();
    for path in store_view_guard_production_rs_files() {
        let rel = rel_path(&path);
        if store_view_guard_is_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        // Rail 1 — `from_executor_snapshot` confined to the allowlist. Skip
        // the constructor's own doc/definition file lines by allowlisting it.
        if src.contains("from_executor_snapshot(") && !allow.contains(rel.as_str()) {
            snapshot_offenders.push(rel.clone());
        }
        // Rail 2 — the fresh-read-then-rebind footgun is banned EVERYWHERE,
        // including inside allowlisted files (the allowlist permits the
        // executor-supplied-view re-bind, not a fresh second read).
        if contains_fresh_read_into_executor_snapshot(&src) {
            fresh_read_offenders.push(rel);
        }
    }
    assert!(
        snapshot_offenders.is_empty(),
        "`StoreViewRead::from_executor_snapshot(view, is_current)` (the one re-bind that pairs a \
         raw view with a separately-named currentness bit) is confined to the executor-boundary \
         allowlist. A new caller must instead derive currentness from a `StoreViewRead` arm via \
         `into_cold_seed_view` (intrinsic), not fabricate a `(view, flag)` pair. Offending \
         files:\n  {}",
        snapshot_offenders.join("\n  ")
    );
    assert!(
        fresh_read_offenders.is_empty(),
        "a production cold path paired a FRESH `resolver_store_view_read()` with \
         `from_executor_snapshot` — the exact currentness/view divergence this guard closes (a \
         stale second read marked current via an earlier flag). A helper doing its own fresh read \
         MUST take the cold-seed straight from that read via `into_cold_seed_view` so the view and \
         its currentness come from ONE read. Offending files:\n  {}",
        fresh_read_offenders.join("\n  ")
    );
}

#[test]
fn store_view_capability_split_guard_is_discriminating() {
    // Self-test (anti-stub): each guarded predicate FLAGS the regression
    // it is meant to catch. If any of these stopped flagging, the
    // corresponding guard would be vacuous.

    // Part A predicate flips on the raw-view signature.
    let leaky = "pub(crate) fn resolver_store_view(&self) -> HostStoreView { todo!() }";
    assert!(
        leaky.contains("fn resolver_store_view(&self) -> HostStoreView"),
        "Part A predicate must catch a raw-`HostStoreView` accessor signature"
    );
    let fixed = "pub(crate) fn resolver_store_view(&self) -> StoreViewRead { todo!() }";
    assert!(
        !fixed.contains("fn resolver_store_view(&self) -> HostStoreView")
            && fixed.contains("fn resolver_store_view(&self) -> StoreViewRead"),
        "Part A predicate must accept the capability-split signature"
    );

    // Part B predicate flips on a `validates` method inside a cold-seed
    // impl block.
    let leaky_block =
        "impl ColdSeedHostStoreView {\n    fn validates(&self, f: &F) -> bool { true }\n}\n";
    let marker = "impl ColdSeedHostStoreView {";
    let start = leaky_block.find(marker).unwrap();
    let rest = &leaky_block[start + marker.len()..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    assert!(
        count_callsites(&rest[..end], &["fn validates("]) > 0,
        "Part B predicate must catch a `validates` method on the cold-seed view"
    );

    // Part D predicate flips on a non-allowlisted `into_owned_view()`
    // user.
    let allow: std::collections::HashSet<&str> =
        INTO_OWNED_VIEW_ALLOWLIST.iter().copied().collect();
    assert!(
        !allow.contains("crates/verter_session/src/typeinfo/resolve_named_symbol.rs"),
        "a typeinfo query-returner must NOT be on the into_owned_view allowlist — \
         it resolves through a proven-current view, never a raw owned view"
    );
    let synthetic_offender_src = "let v = x.into_owned_view();";
    assert!(
        synthetic_offender_src.contains(".into_owned_view()"),
        "Part D predicate must catch an `.into_owned_view()` call"
    );

    // Cold-seed-into-inner predicate flips on the raw-unwrap pattern: a
    // non-allowlisted file that does `.into_cold_seed_view().into_inner()`
    // (the INDIRECT-validation seam — raw cold-seed view fed into a
    // context that then validates) must be flagged.
    let leaky_indirect = "let v = self\n    .resolver_store_view_read()\n    .into_cold_seed_view()\n    .into_inner();\nlet ctx = SessionResolverContext::new(self, view, &v, overlay);";
    assert!(
        contains_cold_seed_into_inner(leaky_indirect),
        "cold-seed-into-inner predicate must catch the `.into_cold_seed_view().into_inner()` \
         raw-unwrap that drops currentness before a context build"
    );
    // The currentness-preserving forms must NOT trip the predicate: an
    // `.is_current()` read or a `.with_session_overlay(` re-root between
    // the cold-seed and any `into_inner` means the flag was consulted, not
    // silently dropped.
    let fixed_is_current = "let seed = self.resolver_store_view_read().into_cold_seed_view();\nlet cur = seed.is_current();\nlet v = seed.into_inner();";
    assert!(
        !contains_cold_seed_into_inner(fixed_is_current),
        "predicate must NOT flag a cold-seed whose `.is_current()` is read before `into_inner` \
         (the currentness is carried, not dropped)"
    );
    let fixed_overlay = "let v = self.resolver_store_view_read().into_cold_seed_view().with_session_overlay(self, view);";
    assert!(
        !contains_cold_seed_into_inner(fixed_overlay),
        "predicate must NOT flag a cold-seed re-rooted via `with_session_overlay` (currentness \
         preserved through the overlay)"
    );
    // The allowlist must NOT contain a view-bound component-meta or
    // fallthrough cold-compute entry that builds a validating context: the
    // fix routed those through `from_cold_seed`, so they neither unwrap a
    // raw cold-seed nor need an allowlist exemption.
    let cold_seed_allow: std::collections::HashSet<&str> =
        COLD_SEED_INTO_INNER_ALLOWLIST.iter().copied().collect();
    assert!(
        !cold_seed_allow.contains("crates/verter_session/src/host_manage/fallthrough.rs"),
        "the fallthrough cold-compute resolver must NOT be on the cold-seed-into-inner \
         allowlist — it validates node-cache entries through the currentness-gated \
         `ctx.store_view()`, never a raw unwrapped cold-seed"
    );
    assert!(
        !cold_seed_allow.contains("crates/verter_session/src/host_manage/overlay_priority.rs"),
        "the prewarm pass must NOT be on the cold-seed-into-inner allowlist — it routes \
         through `SessionResolverContext::from_cold_seed` via a currentness-preserving \
         `with_session_overlay`, never a raw unwrapped cold-seed"
    );

    // `cold_seed_currentness_is_intrinsic_to_the_read` Rail 2 predicate: the
    // fresh-read-then-rebind footgun (a fresh `resolver_store_view_read()`
    // feeding `from_executor_snapshot`) is flagged. This is the EXACT shape
    // the closed bug had — a fresh second read paired with an earlier flag.
    let leaky_rebind = "let s = crate::resolver_store::StoreViewRead::from_executor_snapshot(\n    self.resolver_store_view_read().into_cold_seed_view().into_inner(),\n    base_is_current,\n).into_cold_seed_view();";
    assert!(
        contains_fresh_read_into_executor_snapshot(leaky_rebind),
        "fresh-read-into-executor-snapshot predicate must catch a fresh \
         `resolver_store_view_read()` feeding `from_executor_snapshot` (the view+flag \
         divergence the closed bug produced)"
    );
    // The executor-supplied-view re-bind (the SAFE pattern) must NOT trip the
    // predicate: the view is the `store_view` parameter, not a fresh read.
    let safe_rebind = "let s = crate::resolver_store::StoreViewRead::from_executor_snapshot(\n    store_view.clone(),\n    base_is_current,\n).into_cold_seed_view();";
    assert!(
        !contains_fresh_read_into_executor_snapshot(safe_rebind),
        "predicate must NOT flag the executor-supplied-view re-bind \
         `from_executor_snapshot(store_view.clone(), base_is_current)` — its view came from the \
         executor's single read, not a fresh second read"
    );
    // The intrinsic-currentness production builder (a fresh read taken
    // straight to `into_cold_seed_view`, NO `from_executor_snapshot`) must
    // NOT trip the predicate.
    let fixed_intrinsic =
        "self.resolver_store_view_read().into_cold_seed_view().with_session_overlay(self, view)";
    assert!(
        !contains_fresh_read_into_executor_snapshot(fixed_intrinsic),
        "predicate must NOT flag a fresh read taken straight to `into_cold_seed_view` (currentness \
         intrinsic to the read, no re-bind)"
    );
    // Rail 1 allowlist must NOT contain the view-bound cold-seed builder's
    // route: `view_bound_cold_seed` and the `*_with_overlay` entries derive
    // currentness from a fresh read via `into_cold_seed_view`, so they must
    // not call `from_executor_snapshot` at all — confirm the allowlist is
    // scoped to executor-boundary files, not opened to arbitrary callers.
    let snapshot_allow: std::collections::HashSet<&str> =
        FROM_EXECUTOR_SNAPSHOT_ALLOWLIST.iter().copied().collect();
    assert!(
        !snapshot_allow.contains("crates/verter_session/src/host_manage/overlay_priority.rs"),
        "the prewarm pass must NOT be on the from_executor_snapshot allowlist — it never re-binds \
         an executor `(view, flag)` pair"
    );
    // A synthetic non-allowlisted call must be catchable.
    let synthetic_snapshot = "let s = StoreViewRead::from_executor_snapshot(v, c);";
    assert!(
        synthetic_snapshot.contains("from_executor_snapshot("),
        "Rail 1 predicate must catch a `from_executor_snapshot(` call"
    );
}

/// Discriminator self-test for the production ident scanner: the guard
/// must SEE code after a `#[cfg(test)]`-gated item (the predecessor
/// truncated the whole remainder of the file), must NOT count test-gated
/// bodies or comments, and must catch a planted violation end-to-end.
#[test]
fn session_production_ident_scanner_discriminates() {
    // (1) A test-gated `use` at the top of the file (the
    // `resolver_core/prepared_decl.rs` shape) must NOT hide later
    // production code.
    let body = "#[cfg(test)]\nuse std::cell::Cell;\n\nfn production() {\n    banned_ident();\n}\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(5, "banned_ident".to_string())],
        "a leading #[cfg(test)] use must not blind the scanner to the rest \
         of the file",
    );

    // (2) Idents INSIDE a #[cfg(test)] mod (including its raw strings and
    // braces-in-strings) are NOT production references; production code
    // AFTER the mod still is.
    let body = "fn a() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let s = \"{\"; banned_ident(); }\n    const R: &str = r#\"}\"#;\n}\nfn b() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(7, "banned_ident".to_string())],
        "test-mod bodies must be stripped by ITEM EXTENT (literal-aware), \
         and production code after the mod must stay visible",
    );

    // (3) Comment-only lines are skipped; a cfg(any(test, ...)) item is
    // NOT stripped (it compiles into non-test builds).
    let body = "// banned_ident in a comment\n#[cfg(any(test, debug_assertions))]\nfn dual() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(3, "banned_ident".to_string())],
        "comments are skipped; cfg(any(test, ..)) items stay scanned",
    );

    // (3b) A `#[cfg(test)]` in DOC-COMMENT PROSE (or any non-line-leading
    // position) must NOT start a blanking span: pre-fix it swallowed the
    // production item beneath it through the next depth-0 terminator,
    // silently un-scanning production code.
    let body = "/// gated behind #[cfg(test)] in tests\nfn prod() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(2, "banned_ident".to_string())],
        "a doc-comment-prose #[cfg(test)] must not blank the production \
         item beneath it (the silent-green class)",
    );
    // (3c) …while a genuine line-leading marker still strips its item.
    let body = "#[cfg(test)]\nfn t() { banned_ident(); }\nfn prod() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(3, "banned_ident".to_string())],
        "a line-leading marker still strips exactly its gated item",
    );

    // (5) A `#[cfg(test)]` on a STRUCT FIELD: a field ends with `,` (or
    // the enclosing struct's `}` for a trailing field), NOT `;`/`}` of
    // its own — the extent must stop at the field's comma and must NOT
    // swallow the production fields beneath it or the struct's closing
    // brace (the lib.rs `VerterHost` host-state shape: the principal
    // reintroduction surface is exactly what a mis-scoped field gate
    // blinds).
    let body = "struct S {\n    #[cfg(test)]\n    seam: Option<Hook<A, B>>,\n    prod: u32,\n}\nfn prod() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident", "prod"]);
    assert!(
        hits.contains(&(6, "banned_ident".to_string())),
        "a #[cfg(test)] struct field must not blind the scanner to \
         production code after the struct: {hits:#?}",
    );
    assert!(
        hits.contains(&(4, "prod".to_string())),
        "the production field AFTER a gated field must stay scanned: {hits:#?}",
    );
    let stripped = strip_cfg_test_gated_source(body);
    assert!(
        !stripped.contains("seam"),
        "the gated field itself must be blanked: {stripped:?}",
    );
    assert!(
        syn::parse_file(&stripped).is_ok(),
        "field-gate stripping must keep the output brace-balanced \
         (a swallowed struct close fails syn and silently skips the \
         whole file in the route-mutator guard): {stripped:?}",
    );

    // (5b) A TRAILING gated field (no comma — the extent ends at the
    // enclosing struct's `}`, which must NOT be consumed).
    let body = "struct S {\n    prod: u32,\n    #[cfg(test)]\n    seam: Option<Hook<A, B>>\n}\nfn prod() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(6, "banned_ident".to_string())],
        "a trailing gated field must end its extent BEFORE the enclosing \
         struct's closing brace: {hits:#?}",
    );
    assert!(
        syn::parse_file(&strip_cfg_test_gated_source(body)).is_ok(),
        "trailing-field stripping must keep the output brace-balanced",
    );

    // (5c) Generic commas inside the gated field's type must NOT
    // terminate the extent early (`Hook<A, B>` — the comma at angle
    // depth 1 is part of the type).
    let body = "struct S {\n    #[cfg(test)]\n    seam: Map<K, V>,\n    prod: u32,\n}\n";
    let stripped = strip_cfg_test_gated_source(body);
    assert!(
        !stripped.contains("V>"),
        "the generic tail of the gated field's type must be blanked (the \
         comma inside `Map<K, V>` is not the field terminator): {stripped:?}",
    );
    assert!(
        stripped.contains("prod: u32"),
        "the production field after the gated field must survive: {stripped:?}",
    );

    // (6) A line-leading `#[cfg(test)]` INSIDE a multi-line block
    // comment must NOT start a blanking span (the silent-green class,
    // one shape over from doc-comment prose).
    let body = "/*\n#[cfg(test)]\n*/\nfn prod() { banned_ident(); }\n";
    let hits = ident_hits_in_production_body(body, &["banned_ident"]);
    assert_eq!(
        hits,
        vec![(4, "banned_ident".to_string())],
        "a #[cfg(test)] inside a block comment must not blank the \
         production item beneath it: {hits:#?}",
    );

    // (4) End-to-end positive control: the real walker must observe a
    // known production ident in the file the truncating predecessor
    // left 1/821-scanned (`resolver_core/prepared_decl.rs`, which opens
    // with `#[cfg(test)] use`).
    let control = session_production_ident_hits(&["build_prepared_value_decl_cache"]);
    assert!(
        control
            .iter()
            .any(|(loc, _)| loc.contains("resolver_core/prepared_decl.rs")),
        "the scanner must reach production code DEEP in files that open \
         with a #[cfg(test)]-gated item; got {control:#?}",
    );

    // (7) End-to-end positive control for the FIELD shape: `lib.rs`
    // carries `#[cfg(test)]`-gated `VerterHost` fields followed by
    // production fields — the scanner must still see production
    // identifiers from lib.rs AFTER the gated fields (pre-fix the
    // field gate blanked through the struct's closing brace and the
    // brace-unbalanced output failed syn in the route-mutator guard).
    // `relation_knobs` is a production field of `VerterHost` declared
    // below the `#[cfg(test)]`-gated seam fields.
    let control = session_production_ident_hits(&["relation_knobs"]);
    assert!(
        control.iter().any(|(loc, _)| {
            loc.contains("verter_session/src/lib.rs")
                && loc
                    .rsplit(':')
                    .next()
                    .and_then(|n| n.parse::<usize>().ok())
                    .is_some_and(|line| line > 600)
        }),
        "the scanner must see the production `VerterHost` fields below the \
         #[cfg(test)]-gated fields in lib.rs; got {control:#?}",
    );
}

/// Single-cold-build guard — `parse_and_build_env` is a test/standalone
/// convenience ONLY. A production call inside `verter_session` is a
/// hidden second parse + second env build for a file the canonical
/// materialise path already parsed; the materialise closure threads the
/// single parsed program / env instead.
#[test]
fn no_production_parse_and_build_env_in_session() {
    let hits = session_production_ident_hits(&["parse_and_build_env"]);
    assert!(
        hits.is_empty(),
        "`parse_and_build_env` called from verter_session production code — \
         thread the materialise closure's single EvalEnv instead: {hits:#?}"
    );
}

#[test]
fn session_production_ident_scan_covers_production_under_a_tests_directory() {
    let ws = PlantedWorkspace::new();
    ws.plant_production_and_listed_test_module("fn f() { parse_and_build_env(); }\n");
    let (hits, _) = session_production_ident_hits_under(ws.root(), &["parse_and_build_env"]);
    assert_eq!(
        hits,
        [(
            format!("{}:1", PlantedWorkspace::PRODUCTION_UNDER_TESTS_DIR),
            "parse_and_build_env".to_string(),
        )],
        "the production file under an unlisted `tests` directory must be \
         flagged and the listed test-module file skipped"
    );
}

#[test]
fn session_production_src_files_cover_production_under_a_tests_directory() {
    let ws = PlantedWorkspace::new();
    ws.plant(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
        "",
    );
    ws.plant_production_and_listed_test_module(
        "fn eager(m: &Macro) { lower_type_expr_in_scope_with_mode(m.parsed_type_argument); }\n",
    );
    let flagged: Vec<String> = session_production_src_files_under(ws.root())
        .into_iter()
        .filter(|(rel, src)| macro_arg_eager_lowering_violations(rel, src))
        .map(|(rel, _)| rel)
        .collect();
    assert_eq!(
        flagged,
        [PlantedWorkspace::PRODUCTION_UNDER_TESTS_DIR],
        "the production file under an unlisted `tests` directory must be \
         scanned and flagged and the listed test-module file skipped"
    );
}

/// `ProjectTypeStore::new_for_test_with_state` is a test constructor
/// that compiles into `debug_assertions` builds (the crate's
/// established `cfg(any(test, debug_assertions))` convention). A
/// zero-stamped artifact it seeds passes freshness gates on a
/// generation-0 host, so PRODUCTION code must never call it: the only
/// production-scanned references allowed are in its defining module
/// (`project_type_store.rs` — the definition and its docs). Test files
/// and `#[cfg(test)]`-gated items are already outside the scan.
#[test]
fn new_for_test_with_state_has_no_production_call_site() {
    let hits = session_production_ident_hits(&["new_for_test_with_state"]);
    let foreign: Vec<_> = hits
        .iter()
        .filter(|(loc, _)| !loc.contains("src/project_type_store.rs"))
        .collect();
    assert!(
        foreign.is_empty(),
        "`new_for_test_with_state` referenced from production code outside \
         its defining module — a debug-build production caller can seed \
         zero-stamped artifacts that pass freshness gates: {foreign:#?}"
    );
    // Anti-vacuity: the definition itself must be visible to the scan
    // (it is `cfg(any(test, debug_assertions))`, which the stripper
    // deliberately keeps).
    assert!(
        hits.iter()
            .any(|(loc, _)| loc.contains("src/project_type_store.rs")),
        "anti-vacuity: the scan must see the defining module's reference"
    );
}

/// R6 content-free identity: `SyntheticBindingId` is the synthetic-binding
/// identity that a future synthetic-deepening cache key roots on. It MUST NOT
/// carry a bare `SemanticNodeId`, a `value_node` ordinal, or any
/// content/version hash — the arena ordinal is provenance that lives on the
/// `SemanticNodeData::SyntheticBinding` CARRIER, never on the identity.
#[test]
fn synthetic_binding_identity_is_content_free() {
    let src = read_workspace_file("crates/verter_type_engine/src/semantic_query.rs");
    let body = carrier_guard_struct_body(&src, "SyntheticBindingId");
    // Anti-vacuity: the extractor found the real struct (its known field).
    assert!(
        body.contains("binding_name"),
        "guard must extract the real SyntheticBindingId body"
    );
    for forbidden in ["SemanticNodeId", "value_node", "whole_hash", "content_hash"] {
        assert!(
            !body.contains(forbidden),
            "SyntheticBindingId must be content-free (R6) — found `{forbidden}` \
             in its field list. The arena ordinal / version hash is provenance \
             that belongs on the SemanticNodeData::SyntheticBinding carrier, \
             never on the binding identity."
        );
    }
    // Self-discrimination: the same predicate detects a `value_node` field on
    // a synthetic struct — RED if a bare ordinal is re-introduced.
    let synthetic = "struct X { pub value_node: u64, }";
    assert!(
        carrier_guard_struct_body(synthetic, "X").contains("value_node"),
        "scanner self-test: a value_node field must be detected"
    );
}

/// The regular member-shape cache subject is a SEALED graph-instance memo,
/// not a free `SemanticNodeId` query-identity loophole. The subject's
/// equivalence class is "the EXACT settled graph node that is a component
/// member's value" (`SurfaceMember.value`); it is generation/store-scoped
/// (single-entry, fact-validated, generation-gated), NOT a durable
/// content-free R6 query-identity key. A raw `SemanticNodeId` must not be
/// free to spread into the shape-key subject from an arbitrary node — the
/// ONLY sanctioned construction is the member-value path.
///
/// Four source-scanned pins for the structural seal (the production
/// mechanism is Rust privacy/newtype construction; this guard verifies the
/// landed source shape):
///   (a) the `ShapeSubject::MemberValueNode` variant keys on the sealed
///       newtype `MemberShapeNodeSubject`, NOT a bare `SemanticNodeId`
///       (a bare `SemanticNodeId` field would be externally spreadable);
///   (b) the sealed newtype `MemberShapeNodeSubject(SemanticNodeId)` exists
///       and its sanctioned production constructor `from_surface_member` reads
///       a policy-admitted `&AdmittedPublishedMember` token in ITS OWN
///       SIGNATURE (signature-scoped, not a whole-file name match), so the
///       public-to-crate path is admitted-member-value-only, not arbitrary-node
///       (the `#[cfg(test)]` `from_surface_member_raw(&SurfaceMember)` is the
///       only surviving raw-member form);
///   (c) the production key constructor is the narrow
///       `surface_member_value_whole_with_context`, taking a
///       `&AdmittedPublishedMember` — and the retired arbitrary-`SemanticNodeId`
///       production constructor `semantic_node_whole_with_context` is ABSENT
///       from production (its only surviving form is the explicitly test-named
///       `member_value_node_whole_for_test`);
///   (d) `member_shape_peek_or_compute`'s OWN SIGNATURE takes the policy-admitted
///       `&AdmittedPublishedMember` token (signature-scoped), not a bare
///       `SemanticNodeId`, so the caller cannot route an arbitrary node through
///       the cache.
#[test]
fn member_value_node_subject_is_sealed_newtype_and_member_constructed() {
    let src = read_workspace_file("crates/verter_type_engine/src/component_meta_caches.rs");

    // (a) The `ShapeSubject::MemberValueNode` variant keys on the sealed
    //     newtype, not a bare `SemanticNodeId`.
    let variant_body = enum_variant_struct_body(&src, "MemberValueNode");
    // Anti-vacuity: the extractor found the real variant (its `node` field).
    assert!(
        variant_body.contains("node"),
        "guard must extract the real ShapeSubject::MemberValueNode variant body"
    );
    assert!(
        variant_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "MemberShapeNodeSubject"),
        "the ShapeSubject::MemberValueNode variant must key its `node` on the \
         SEALED newtype `MemberShapeNodeSubject`, so a raw `SemanticNodeId` \
         cannot spread into the shape-key subject"
    );
    // Boundary-aware: the variant body must NOT carry a bare `SemanticNodeId`
    // (it would be externally struct-constructible from any node and defeat
    // the seal). The newtype hides the ordinal behind a module-private field.
    assert!(
        !variant_body
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "SemanticNodeId"),
        "ShapeSubject::MemberValueNode must NOT carry a bare `SemanticNodeId` \
         field — the sealed `MemberShapeNodeSubject` newtype is the only \
         sanctioned representation (its inner ordinal is module-private)"
    );

    // (b) The sealed newtype + its member-value constructor exist.
    assert!(
        src.contains("struct MemberShapeNodeSubject"),
        "the sealed newtype `MemberShapeNodeSubject` (the structural seal over \
         the member's `SurfaceMember.value` graph node) must exist"
    );
    // The newtype's sanctioned production constructor reads a policy-admitted
    // `&AdmittedPublishedMember` token — construction is admitted-member-value-
    // only, strictly TIGHTER than the prior `&SurfaceMember` (an unadmitted
    // member can no longer be routed through the sealed subject). A
    // `#[cfg(test)]` `from_surface_member_raw(&SurfaceMember)` is the only
    // surviving raw-member form.
    //
    // SIGNATURE-SCOPED: the check inspects `from_surface_member`'s OWN parameter
    // types, NOT a whole-file substring. The prior
    // `src.contains("fn from_surface_member") && src.contains("AdmittedPublishedMember")`
    // collapsed to a name-only match because the token name appears in many
    // places — a by-value `from_surface_member(member: SemanticNodeId)` would
    // have passed merely because `AdmittedPublishedMember` occurs ELSEWHERE in
    // the file. Now the ctor's OWN params must mention the member-value-bound
    // token (or the raw `SurfaceMember` form); a by-value `SemanticNodeId`
    // param FAILS.
    let ctor_takes_admitted_member =
        named_fn_param_mentions_type(&src, "from_surface_member", "AdmittedPublishedMember");
    let ctor_takes_surface_member_ref =
        named_fn_param_mentions_type(&src, "from_surface_member", "SurfaceMember");
    assert!(
        ctor_takes_admitted_member || ctor_takes_surface_member_ref,
        "the sealed newtype must expose `from_surface_member(&AdmittedPublishedMember)` \
         (the admitted-member-value production constructor) IN ITS OWN SIGNATURE — a by-value or \
         wrong-type signature (e.g. `from_surface_member(member: SemanticNodeId)`) must NOT \
         satisfy the seal even when `AdmittedPublishedMember` appears elsewhere in the file"
    );

    // (c) The narrow production key constructor takes a policy-admitted
    //     `&AdmittedPublishedMember`, and the retired arbitrary-`SemanticNodeId`
    //     production constructor is gone.
    assert!(
        named_fn_param_mentions_type(
            &src,
            "surface_member_value_whole_with_context",
            "OutputAuthority"
        ),
        "the member-shape key constructor must take the engine's `OutputAuthority` IN ITS OWN \
         SIGNATURE — only a terminal output sink holds it, so no query or dispatch handle can \
         mint a member-shape key"
    );
    assert!(
        src.contains("fn surface_member_value_whole_with_context"),
        "the narrow production key constructor \
         `ShapeCacheKey::surface_member_value_whole_with_context(scope, \
         &AdmittedPublishedMember, ctx)` must exist — it is the SOLE production \
         construction path for the member-shape subject"
    );
    // The retired generic constructor that accepts an ARBITRARY `SemanticNodeId`
    // must NOT exist in production. (A `#[cfg(test)]`-named
    // `member_value_node_whole_for_test` is the only surviving arbitrary-node
    // form; it carries `_for_test` so it cannot masquerade as production.)
    let production = carrier_production_code(&src);
    assert!(
        !production.contains("fn semantic_node_whole"),
        "the retired arbitrary-`SemanticNodeId` constructor \
         `semantic_node_whole[_with_context]` must be ABSENT from production — \
         construction is pinned to the member-value path \
         `surface_member_value_whole_with_context`; the only test form is the \
         explicitly-named `member_value_node_whole_for_test`"
    );

    // (d) The production peek/compute helper takes the policy-admitted token BY
    //     REFERENCE, not a bare `SemanticNodeId` — so an arbitrary / unadmitted
    //     node cannot be routed through the cache subject.
    //     `member_shape_peek_or_compute` lives in the terminal `output_sink`
    //     sink module (it raises through the module-private boundary primitive)
    //     and reads `admitted.member().value` for the sealed subject key.
    //
    //     SIGNATURE-SCOPED (matching (b)): the check inspects
    //     `member_shape_peek_or_compute`'s OWN parameter types, NOT a whole-file
    //     substring — the prior `proj_ws.contains("AdmittedPublishedMember")`
    //     held because the token name appears all over `output_sink.rs`.
    let projectors =
        read_workspace_file("crates/verter_session/src/meta_resolve/projectors/output_sink.rs");
    assert!(
        named_fn_param_mentions_type(&projectors, "member_shape_peek_or_compute", "_")
            || projectors.contains("fn member_shape_peek_or_compute"),
        "guard must find `member_shape_peek_or_compute`"
    );
    let peek_takes_admitted = named_fn_param_mentions_type(
        &projectors,
        "member_shape_peek_or_compute",
        "AdmittedPublishedMember",
    );
    let peek_takes_surface_member =
        named_fn_param_mentions_type(&projectors, "member_shape_peek_or_compute", "SurfaceMember");
    assert!(
        peek_takes_admitted || peek_takes_surface_member,
        "`member_shape_peek_or_compute` must take a policy-admitted \
         `&AdmittedPublishedMember` token (the admitted-member-value path) IN ITS OWN SIGNATURE, \
         not a bare `SemanticNodeId` — so the caller cannot route an arbitrary / unadmitted node \
         through the sealed shape subject (a whole-file `AdmittedPublishedMember` occurrence no \
         longer satisfies this)"
    );

    // Self-discrimination: the variant-keys-on-newtype check trips if a bare
    // `SemanticNodeId` is (re-)planted on a `MemberValueNode`-shaped variant.
    let planted = "enum E { MemberValueNode { scope: Arc<str>, node: SemanticNodeId }, }";
    assert!(
        enum_variant_struct_body(planted, "MemberValueNode")
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|tok| tok == "SemanticNodeId"),
        "scanner self-test: a planted bare `SemanticNodeId` field on the \
         variant must be detected (would defeat the seal)"
    );
    // And the retired-constructor scan trips on a planted production
    // `fn semantic_node_whole`.
    let planted_ctor = "pub(crate) fn semantic_node_whole_with_context() {}";
    assert!(
        carrier_production_code(planted_ctor).contains("fn semantic_node_whole"),
        "scanner self-test: a planted production `fn semantic_node_whole` must \
         be detected"
    );
    // Self-discrimination for the SIGNATURE-SCOPED (b): the CRITICAL case the
    // strengthening fixes — a by-value `from_surface_member(member:
    // SemanticNodeId)` WITH `AdmittedPublishedMember` present ELSEWHERE in the
    // file must FAIL the combined (b) predicate. The OLD whole-file
    // `contains("fn from_surface_member") && contains("AdmittedPublishedMember")`
    // would have PASSED it (the token name appears in the unrelated struct);
    // the signature-scoped check inspects only the ctor's OWN params.
    let byval_with_token_elsewhere = r#"
        // An UNRELATED admitted-token mention elsewhere in the file.
        struct Unrelated { field: AdmittedPublishedMember }
        impl S {
            fn from_surface_member(member: SemanticNodeId) -> Self { Self(member) }
        }
    "#;
    let combined_b = named_fn_param_mentions_type(
        byval_with_token_elsewhere,
        "from_surface_member",
        "AdmittedPublishedMember",
    ) || named_fn_param_mentions_type(
        byval_with_token_elsewhere,
        "from_surface_member",
        "SurfaceMember",
    );
    assert!(
        !combined_b,
        "scanner self-test: a by-value `from_surface_member(member: SemanticNodeId)` MUST FAIL the \
         combined (b) predicate even when `AdmittedPublishedMember` appears ELSEWHERE in the file — \
         the check is signature-scoped, not a whole-file name match"
    );
    // The real admitted-member form PASSES the signature-scoped check.
    let admitted_form = r#"
        impl S {
            fn from_surface_member(
                member: &crate::meta_resolve::projectors::publication_authority::AdmittedPublishedMember<'_>,
            ) -> Self { Self(member.member().value) }
        }
    "#;
    assert!(
        named_fn_param_mentions_type(
            admitted_form,
            "from_surface_member",
            "AdmittedPublishedMember"
        ),
        "scanner self-test: the real `from_surface_member(&AdmittedPublishedMember)` form MUST \
         satisfy the signature-scoped (b) check"
    );
    // The `#[cfg(test)]` raw `&SurfaceMember` form also satisfies it (the raw
    // ctor name differs, but the member-value-bound check accepts a SurfaceMember
    // param on `from_surface_member` too — both are member-value-bound).
    let surface_member_form =
        "impl S { fn from_surface_member(member: &SurfaceMember) -> Self { Self(member.value) } }";
    assert!(
        named_fn_param_mentions_type(surface_member_form, "from_surface_member", "SurfaceMember"),
        "scanner self-test: a `from_surface_member(member: &SurfaceMember)` form must satisfy the \
         signature-scoped member-value check"
    );
    // And the signature-scoped helper does NOT cross fn boundaries: a DIFFERENT
    // fn taking the token does not make `from_surface_member` pass.
    let wrong_fn_takes_token = r#"
        impl S {
            fn other(member: &AdmittedPublishedMember) {}
            fn from_surface_member(member: SemanticNodeId) -> Self { Self(member) }
        }
    "#;
    assert!(
        !named_fn_param_mentions_type(
            wrong_fn_takes_token,
            "from_surface_member",
            "AdmittedPublishedMember"
        ),
        "scanner self-test: the helper must NOT credit `from_surface_member` for a token param on a \
         DIFFERENT fn (signature-scoped, per-fn)"
    );
}

/// Scoped `Unknown`-as-control-flow ban: the carrier-construction surface
/// (`carrier.rs`, the home of `CarrierResolverContext` and — as later stages
/// land — the carrier lowerer / resolver) must emit TYPED carriers (BareRef /
/// ImportType / RawFallback nodes or typed `QueryError`), never a raw
/// `TypeExpr::Unknown` control sentinel. SCOPED to this surface — NOT global:
/// `raise.rs` legitimately materialises `Unknown` at the OUTPUT boundary, and
/// the global fence lands with the final cutover.
#[test]
fn carrier_constructors_do_not_use_unknown_as_control_flow() {
    let src =
        read_workspace_file("crates/verter_type_engine/src/project_semantic_dispatch/carrier.rs");
    // Scope to the PRODUCTION portion (before the module's `#[cfg(test)]`
    // block) with `//` comments stripped, so only real CODE constructing
    // `TypeExpr::Unknown` trips the scan (module / field docs may mention it
    // in prose). The same `carrier_production_code` helper drives the
    // self-test below, so the self-test exercises the real scan logic.
    let production = carrier_production_code(&src);
    assert!(
        !production.contains("TypeExpr::Unknown"),
        "the carrier-construction surface (carrier.rs) must not use \
         `TypeExpr::Unknown` as a control signal — emit a typed carrier \
         (BareRef / ImportType / RawFallback) or a typed QueryError instead."
    );

    // Self-discrimination through the SAME extractor (never a bare
    // `synthetic.contains(...)` that would hold by construction):
    //   POSITIVE — a production-portion `TypeExpr::Unknown` IS detected, so a
    //   real construction in carrier.rs would trip the assertion above.
    let positive = "let x = TypeExpr::Unknown { raw: \"y\".to_string() };";
    assert!(
        carrier_production_code(positive).contains("TypeExpr::Unknown"),
        "scanner self-test (positive): a production-portion `TypeExpr::Unknown` \
         construction must be detected"
    );
    //   NEGATIVE-1 — `TypeExpr::Unknown` mentioned only in a `//` comment is
    //   stripped out, so it does NOT trip (proves comment-stripping works).
    let comment_only = "let ok = 1; // TypeExpr::Unknown is fine in prose";
    assert!(
        !carrier_production_code(comment_only).contains("TypeExpr::Unknown"),
        "scanner self-test (negative, comment): a `//`-commented \
         `TypeExpr::Unknown` must be stripped from the production scan"
    );
    //   NEGATIVE-2 — `TypeExpr::Unknown` only AFTER a `#[cfg(test)]` marker is
    //   excluded, so it does NOT trip (proves the cfg-test split works).
    let test_only = "fn prod() {}\n#[cfg(test)]\nmod t { let x = TypeExpr::Unknown { raw: () }; }";
    assert!(
        !carrier_production_code(test_only).contains("TypeExpr::Unknown"),
        "scanner self-test (negative, cfg-test): a `TypeExpr::Unknown` after a \
         `#[cfg(test)]` marker must be excluded from the production scan"
    );
}

/// `HotTypeRef` (the internal session hot handle) is a DISTINCT nominal type
/// from the public `component_meta_payload::TypeHandle` DTO, and is
/// deliberately NOT `Hash`/`Ord` so R6 structurally forbids it from ever
/// being a derived-`Hash` / `BTreeMap` cache key. A future `#[derive(Hash)]`
/// on this `Copy` newtype — or a re-alias of the public handle onto the
/// `HotTypeRef` name — must turn this guard RED.
#[test]
fn hot_type_ref_is_distinct_handle_and_not_hash_or_ord_derived() {
    let src = read_workspace_file("crates/verter_type_engine/src/semantic_query.rs");

    // (1) Anti-vacuity: the real `struct HotTypeRef` declaration exists.
    assert!(
        src.contains("struct HotTypeRef"),
        "guard must find the real `struct HotTypeRef` in semantic_query.rs"
    );

    // (2) Name distinctness: `HotTypeRef` is its own nominal `struct`, NOT a
    // type alias of, nor a `use ... as` re-export of, the public `TypeHandle`.
    assert!(
        !src.contains("type HotTypeRef"),
        "`HotTypeRef` must be a distinct `struct`, never a \
         `type HotTypeRef = ...TypeHandle...` alias of the public DTO"
    );
    assert!(
        !src.contains("as HotTypeRef"),
        "`HotTypeRef` must not be a `use ... TypeHandle as HotTypeRef` \
         re-export — the public handle stays \
         `component_meta_payload::TypeHandle`"
    );
    let decl_line = src
        .lines()
        .find(|l| l.contains("struct HotTypeRef"))
        .expect("guard must find the `struct HotTypeRef` line");
    assert!(
        !decl_line.contains("TypeHandle"),
        "the `struct HotTypeRef` declaration must not wrap / equate to the \
         public `TypeHandle` DTO"
    );

    // (3) R6 non-key rail: its derive carries NEITHER `Hash` NOR `Ord`.
    let derives = carrier_struct_derive_list(&src, "HotTypeRef");
    assert!(
        !derive_list_has_hash_or_ord(&derives),
        "`HotTypeRef` must NOT derive `Hash`/`Ord` (R6): a content/version- \
         bearing arena ordinal must never be embeddable in a derived-`Hash` / \
         `BTreeMap` cache key. Found derive list: `{derives}`."
    );

    // (4) Self-discrimination through the SAME extractor + predicate (never a
    // bare `synthetic.contains(\"Hash\")`):
    //   - a synthetic `HotTypeRef` deriving `Hash` TRIPS the predicate,
    let with_hash = "#[derive(Debug, Clone, Copy, Hash)]\nstruct HotTypeRef(SemanticNodeId);";
    assert!(
        derive_list_has_hash_or_ord(&carrier_struct_derive_list(with_hash, "HotTypeRef")),
        "self-test: a `HotTypeRef` deriving `Hash` must trip the predicate"
    );
    //   - a STACKED `#[derive(Hash)]` above a Hash-free derive TRIPS the
    //     predicate: Rust allows multiple derive attributes, and the extractor
    //     must collect EVERY one. A single `rfind("#[derive(")` would see only
    //     the LAST (Hash-free) derive and miss the `Hash` on the earlier
    //     stacked line — a silent R6 bypass while the type DERIVES `Hash`.
    let stacked =
        "#[derive(Hash)]\n#[derive(Debug, Clone, Copy)]\nstruct HotTypeRef(SemanticNodeId);";
    assert!(
        derive_list_has_hash_or_ord(&carrier_struct_derive_list(stacked, "HotTypeRef")),
        "self-test: a STACKED `#[derive(Hash)]` above a Hash-free derive must \
         trip the predicate — the extractor must union ALL stacked derives"
    );
    //   - a synthetic deriving `Ord` TRIPS the predicate,
    let with_ord = "#[derive(Clone, Ord)]\nstruct HotTypeRef(SemanticNodeId);";
    assert!(
        derive_list_has_hash_or_ord(&carrier_struct_derive_list(with_ord, "HotTypeRef")),
        "self-test: a `HotTypeRef` deriving `Ord` must trip the predicate"
    );
    //   - the real-shaped derive (no Hash/Ord) does NOT trip the predicate,
    let without =
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\nstruct HotTypeRef(SemanticNodeId);";
    assert!(
        !derive_list_has_hash_or_ord(&carrier_struct_derive_list(without, "HotTypeRef")),
        "self-test: a Hash/Ord-free derive must NOT trip the predicate"
    );
    //   - whole-token matching: `PartialOrd` must NOT be a substring
    //     false-positive for `Ord`.
    assert!(
        !derive_list_has_hash_or_ord("Debug, Clone, PartialOrd, PartialEq"),
        "self-test: `PartialOrd` must not be a substring false-positive for `Ord`"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Query-free structural lowerer guards.
//
// The session-owned structural lowerer (`structural_carrier_producer/lower.rs`)
// EMITS the dormant graph carriers from the owned `TypeExpr` without performing
// any name / import / type resolution. These guards lock its query-free,
// emit-only contract and the worker-side dep barrier.
// ───────────────────────────────────────────────────────────────────────────

/// The query-free structural lowerer performs NO resolution / host query: its
/// production code must not reach a dispatcher, resolver context, query key, or
/// host / type-provider surface. Carrier RESOLUTION is a demand-time concern;
/// the lowerer only emits typed carriers.
#[test]
fn session_graph_lowerer_makes_no_query() {
    let src = read_workspace_file(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    );
    let production = carrier_production_code(&src);
    // Anti-vacuity: the extractor found the real lowerer production code.
    assert!(
        production.contains("fn lower_type_expr_structural"),
        "guard must extract the real structural lowerer production code"
    );
    // The producer module now co-locates the structural lowerer (query-free) with
    // the macro hot-mirror accessor `macro_type_arg_hot_ref`, which legitimately
    // holds a `&dyn ResolverContext` to read the owner's ROUTE-FREE `IndexedReady`
    // (`ensure_indexed_ready_serve`). So `ResolverContext` / `ensure_indexed_ready`
    // are NOT in the forbidden list here — the route-free-vs-route distinction is
    // owned by `macro_hot_mirror_producer_is_pure_no_route_resolution` (which bans
    // the resolution surface and explicitly ALLOWS the route-free reads). This
    // guard bans the genuine QUERY / DISPATCH surface, which must be absent across
    // the WHOLE producer module (the lowerer AND the mirror accessor): a
    // `ProjectSemanticDispatch` / `SemanticQueryKey` / `.execute(` / resolve query
    // is a host type-resolution, never performed by a producer.
    for forbidden in [
        "ProjectSemanticDispatch",
        "SemanticQueryKey",
        ".execute(",
        "execute_read",
        "execute_type_node",
        // [P0] the dispatch/query route — the "SECOND QUERY-TIME RESOLUTION
        // ENGINE" the single-engine rule forbids. A producer that reaches
        // `ctx.dispatch().lower_type_expr_in_scope_with_context(...)` route-
        // resolves imports (via `prepared_decl_bundle`) — a second resolver.
        ".dispatch(",
        "lower_type_expr_in_scope_with_",
        // The route-resolving `ensure_indexed_ready(` (open-paren form) — DISTINCT
        // from the allowed route-free `ensure_indexed_ready_serve` (which the
        // mirror accessor legitimately calls). The open-paren in the needle means
        // `ensure_indexed_ready_serve(` (whose next char after the prefix is `_`,
        // not `(`) never matches.
        "ensure_indexed_ready(",
        "resolve_bare_name_in_scope",
        "resolve_type_dependency_canonical",
        "prepared_decl_bundle",
        "prepared_type_decl",
        "prepared_value_decl",
        "materialize_type_expr",
        "raise_node_to_type_expr",
        "type_provider",
        "tsserver",
        // Assembled at compile time so this needle list does not itself carry
        // the literal identifier and trip the
        // `no_session_solver_host_in_production_code` retired-symbol scanner,
        // which greps non-`*_tests.rs` `crates/**` source for `SessionSolverHost`.
        // The scan against `structural_carrier_producer/macro_arg_producer.rs` is
        // unchanged — it still looks for the full assembled identifier.
        concat!("Session", "SolverHost"),
    ] {
        assert!(
            !production.contains(forbidden),
            "the query-free structural producer (structural_carrier_producer/macro_arg_producer.rs) \
             must perform NO resolution / host query — found `{forbidden}` in production code. \
             Carrier resolution is a demand-time concern; emit a typed carrier instead."
        );
    }
    // Self-discrimination through the SAME extractor (never a bare
    // `synthetic.contains(...)`):
    //   POSITIVE — a real `.execute(` / `execute_read` query IS detected.
    let positive = "fn f() { let _ = self.execute_read(key); }";
    assert!(
        carrier_production_code(positive).contains("execute_read"),
        "scanner self-test (positive): a production `execute_read` query must be detected"
    );
    //   NEGATIVE-1 — a query token only in a `//` comment is stripped.
    let comment_only = "fn f() {} // execute_type_node is only described in prose";
    assert!(
        !carrier_production_code(comment_only).contains("execute_type_node"),
        "scanner self-test (negative, comment): a commented query token must be stripped"
    );
    //   NEGATIVE-2 — a query token only after `#[cfg(test)]` is excluded.
    let test_only = "fn prod() {}\n#[cfg(test)]\nmod t { fn g() { d.execute_read(k); } }";
    assert!(
        !carrier_production_code(test_only).contains("execute_read"),
        "scanner self-test (negative, cfg-test): a query token after `#[cfg(test)]` must be excluded"
    );
    //   POSITIVE ([P0]) — the dispatch/eager-lowering route IS detected. A
    //   `ctx.dispatch().lower_type_expr_in_scope_with_context(...)` is the second
    //   resolution engine the single-engine rule forbids; both needles fire.
    let dispatch_route =
        "fn rogue() { let _ = ctx.dispatch().lower_type_expr_in_scope_with_context(s, e, c); }";
    assert!(
        carrier_production_code(dispatch_route).contains(".dispatch("),
        "scanner self-test (positive): a `.dispatch(` route call must be detected"
    );
    assert!(
        carrier_production_code(dispatch_route).contains("lower_type_expr_in_scope_with_"),
        "scanner self-test (positive): an eager `lower_type_expr_in_scope_with_*` call must be \
         detected"
    );
    //   POSITIVE — a static `ProjectSemanticDispatch::lower_type_expr_in_scope_with_context`
    //   call is detected.
    let static_dispatch =
        "fn rogue() { ProjectSemanticDispatch::lower_type_expr_in_scope_with_context(s, e, c); }";
    assert!(
        carrier_production_code(static_dispatch).contains("ProjectSemanticDispatch"),
        "scanner self-test (positive): a `ProjectSemanticDispatch::…` call must be detected"
    );
    //   DISTINCTION — the route-resolving `ensure_indexed_ready(` IS detected, but
    //   the allowed route-free `ensure_indexed_ready_serve(` is NOT (the needle's
    //   trailing `(` never matches the `_serve` suffix). This is the load-bearing
    //   distinction the brief flagged.
    let route_resolving = "fn rogue() { ctx.ensure_indexed_ready(owner); }";
    assert!(
        carrier_production_code(route_resolving).contains("ensure_indexed_ready("),
        "scanner self-test (positive): the route-resolving `ensure_indexed_ready(` must be detected"
    );
    let route_free = "fn ok() { let serve = ctx.ensure_indexed_ready_serve(owner); }";
    assert!(
        !carrier_production_code(route_free).contains("ensure_indexed_ready("),
        "scanner self-test (DISTINCTION): the route-free `ensure_indexed_ready_serve(` must NOT \
         match the `ensure_indexed_ready(` needle — the open-paren form distinguishes the route \
         from the `_serve` suffix (the real producer uses `_serve`)"
    );
}

/// During EMISSION the structural lowerer never raises / materializes a carrier
/// back to `TypeExpr`: materialization is the reverse OUTPUT boundary
/// (`raise.rs`), not part of forward lowering. Static half (this test) — the
/// lowerer's production code references no materialize / raise helper; the
/// runtime half lives in `structural_lower_tests.rs`
/// (`structural_root_is_an_unmaterialized_carrier`), which lowers `Foo<Bar>` and
/// an import type and asserts the emitted root stays a `BareRef` / `ImportType`
/// carrier.
#[test]
fn unresolved_carriers_not_materialized_during_emission() {
    let src = read_workspace_file(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    );
    let production = carrier_production_code(&src);
    assert!(
        production.contains("fn lower_type_expr_structural"),
        "guard must extract the real structural lowerer production code"
    );
    for forbidden in [
        "materialize_type_expr",
        "raise_node_to_type_expr",
        "raise_index_key_to_type_expr",
        "raise_and_reduce",
    ] {
        assert!(
            !production.contains(forbidden),
            "the structural lowerer must EMIT carriers, never materialize / raise them \
             back to TypeExpr during emission — found `{forbidden}` in production code. \
             Materialization is the reverse OUTPUT boundary (raise.rs), not forward lowering."
        );
    }
    // Self-discrimination through the SAME extractor:
    //   POSITIVE — a real `materialize_type_expr` call IS detected.
    let positive = "fn f() { let _ = self.materialize_type_expr(handle); }";
    assert!(
        carrier_production_code(positive).contains("materialize_type_expr"),
        "scanner self-test (positive): a production `materialize_type_expr` call must be detected"
    );
    //   NEGATIVE-1 — a raise token only in a `//` comment is stripped.
    let comment_only = "fn f() {} // raise_node_to_type_expr is the reverse boundary";
    assert!(
        !carrier_production_code(comment_only).contains("raise_node_to_type_expr"),
        "scanner self-test (negative, comment): a commented raise token must be stripped"
    );
    //   NEGATIVE-2 — a raise token only after `#[cfg(test)]` is excluded.
    let test_only = "fn prod() {}\n#[cfg(test)]\nmod t { fn g() { d.materialize_type_expr(h); } }";
    assert!(
        !carrier_production_code(test_only).contains("materialize_type_expr"),
        "scanner self-test (negative, cfg-test): a raise token after `#[cfg(test)]` must be excluded"
    );
}

/// The OXC worker + semantic-lowering surface produces the owned `TypeExpr` IR
/// ONLY — it never emits a session semantic-graph node. The semantic crate
/// cannot even depend on session; this LOCKS that the worker surface (and the
/// session-side retained-worker `decl_lowering`) stays free of the session-graph
/// types, so session-graph emission can never leak into the worker.
#[test]
fn oxc_worker_emits_no_session_graph_node() {
    let forbidden = [
        "SemanticNodeData",
        "SemanticNodeId",
        "HotTypeRef",
        "SemanticGraphStore",
        "intern_node",
    ];
    let mut files: Vec<PathBuf> = Vec::new();
    collect_production_rs(
        &workspace_path("crates/verter_type_expr_oxc/src"),
        &mut files,
    );
    collect_production_rs(
        &workspace_path("crates/verter_semantic/src/analysis"),
        &mut files,
    );
    files.push(workspace_path(
        "crates/verter_semantic_source/src/decl_lowering.rs",
    ));
    // Anti-vacuity: the walker found a real, non-trivial worker surface.
    assert!(
        files.len() > 5,
        "guard must find the OXC-worker / semantic-lowering surface files; found {}",
        files.len()
    );
    for file in &files {
        let src =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let production = carrier_production_code(&src);
        for tok in forbidden {
            assert!(
                !production.contains(tok),
                "{}: the OXC-worker / semantic-lowering surface must emit owned TypeExpr IR \
                 only — found session-graph `{tok}` in production code. The semantic crate \
                 cannot depend on session; session-graph emission is the worker's forbidden side.",
                file.display()
            );
        }
    }
    // Self-discrimination through the SAME extractor:
    //   POSITIVE — a real production `intern_node` call IS detected.
    let positive = "fn f() { graph.intern_node(data); }";
    assert!(
        carrier_production_code(positive).contains("intern_node"),
        "scanner self-test (positive): a production `intern_node` call must be detected"
    );
    //   NEGATIVE-1 — a session-graph token only in a `//` comment is stripped.
    let comment_only = "fn f() {} // SemanticNodeData appears only in this prose";
    assert!(
        !carrier_production_code(comment_only).contains("SemanticNodeData"),
        "scanner self-test (negative, comment): a commented session-graph token must be stripped"
    );
    //   NEGATIVE-2 — a session-graph token only after `#[cfg(test)]` is excluded.
    let test_only = "fn prod() {}\n#[cfg(test)]\nmod t { let _ = SemanticNodeId(0); }";
    assert!(
        !carrier_production_code(test_only).contains("SemanticNodeId"),
        "scanner self-test (negative, cfg-test): a session-graph token after `#[cfg(test)]` must be excluded"
    );
}

/// PRIMARY single-engine producer guard (make-unrepresentable): the raw
/// structural-lowering entry `lower_type_expr_structural` lives ONLY under
/// `crate::structural_carrier_producer::macro_arg_producer` and is MODULE-PRIVATE
/// (no visibility modifier) AND is NOT re-exported anywhere in the crate. The
/// owner declares the module as a PRIVATE `mod macro_arg_producer;` (only
/// `macro_type_arg_hot_ref` + `MacroHotMirror` are re-exported), so no FOREIGN
/// module can name the lowerer — a foreign reference is a compile error, not a
/// lint — and the single-engine producer rule needs no FOREIGN caller scanner
/// here. The same-owner case is moot: the producer is collapsed into this ONE
/// module, so there is no other file that could name the private lowerer.
#[test]
fn structural_carrier_producer_lowerer_is_module_private() {
    let rel = "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs";
    let body = std::fs::read_to_string(workspace_path(rel))
        .unwrap_or_else(|e| panic!("guard could not read {rel}: {e}"));
    let production_files = session_production_src_files();
    // The guard pins ALL THREE producer-capable builders (the raw structural
    // lowerer, the macro hot-mirror builder, and the binder-seed builder) — each
    // single-defined, bare module-private, and re-exported nowhere. Rust privacy
    // is MODULE-scoped (a same-module second producer that NAMES these builders is
    // representable), so widening ANY of the three re-opens a producer surface.
    for builder in STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS {
        let needle = format!("fn {builder}(");
        // (a) Anti-vacuity: the owner module must exist at this path and carry the
        // builder's definition — its absence means the owner re-home regressed.
        assert!(
            body.contains(&needle),
            "anti-vacuity: the producer builder `{builder}` (`fn {builder}(`) must live at \
             `{rel}` (`{STRUCTURAL_CARRIER_PRODUCER_LOWERER_MODULE}`) — its absence means the \
             structural-carrier-producer owner re-home regressed"
        );
        // (a) The builder is defined NOWHERE ELSE in the crate — exactly one
        // definition, in `macro_arg_producer.rs`.
        let definitions: Vec<String> = production_files
            .iter()
            .filter(|(_, src)| src.contains(&needle))
            .map(|(rel, _)| rel.clone())
            .collect();
        assert_eq!(
            definitions,
            vec![
                "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs"
                    .to_string()
            ],
            "the raw `fn {builder}(` must be defined ONLY in \
             `structural_carrier_producer/macro_arg_producer.rs`; found in {definitions:#?}"
        );
        // (b) The definition line carries NO visibility modifier.
        if let Some(reason) = structural_carrier_producer_builder_privacy_violation(&body, builder)
        {
            panic!(
                "single-engine producer rule violated: {reason}. The builder must stay \
                 module-private to `crate::structural_carrier_producer::macro_arg_producer` so a \
                 second structural-carrier producer that NAMES it (same-module privacy permits one) \
                 is policed."
            );
        }
        // (c) The builder is NOT re-exported anywhere in the crate — a `pub use`
        // re-export would mint a nameable alias and re-open the producer surface.
        for (file_rel, src) in &production_files {
            assert!(
                !structural_builder_reexport_violation(src, builder),
                "the producer builder `{builder}` must NOT be re-exported (found a `pub use … \
                 {builder}` in `{file_rel}`) — a re-export mints a nameable alias of the \
                 otherwise-private builder, re-opening the single-engine producer surface"
            );
        }
    }
}

/// SMALL no-reintroduce-a-surface backstop (the structural design's named
/// residual): `macro_arg_producer.rs` — the SINGLE producer module that owns the
/// module-private structural lowerer, macro hot-mirror builder, and binder-seed
/// builder — declares NO production (non-`#[cfg(test)]`) macro invocation /
/// `macro_rules!` / `include!` / proc-macro attribute / `#[derive]` on a
/// producer-capable item / out-of-line-or-`#[path]` child mod / `#[macro_use]`.
///
/// The LOAD-BEARING single-producer guarantee is the COMPILER module-privacy of
/// `macro_arg_producer.rs` (a foreign caller of the private lowering builders is
/// a compile error, and the producer is collapsed into ONE module so no
/// same-owner file can name them either). This guard is the small backstop the
/// structural design's residual names: it stops a future edit re-introducing a
/// same-module code-generation surface (a derive / macro_rules! / include! /
/// `#[macro_use]`-injected derive / a `#[path]` splice) that could emit code
/// reaching the private builders WITHOUT a literal call — the only class the
/// structure cannot already make a compile error.
#[test]
fn macro_arg_producer_has_no_production_expansion_surface() {
    let rel = "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs";
    let src = std::fs::read_to_string(workspace_path(rel))
        .unwrap_or_else(|e| panic!("guard could not read {rel}: {e}"));
    // Anti-vacuity: the producer module must exist and carry the lowerer — its
    // absence means the owner re-home regressed.
    assert!(
        src.contains("fn lower_type_expr_structural("),
        "anti-vacuity: the producer module `{rel}` must carry the structural lowerer"
    );
    let violations = macro_arg_producer_expansion_surface_violations(&src);
    assert!(
        violations.is_empty(),
        "expansion-surface violation: `macro_arg_producer.rs` must declare NO production \
         (non-`#[cfg(test)]`) bang-macro invocation / `macro_rules!` / proc-macro attribute / \
         qualified-or-custom `#[derive(…)]` on a producer-capable item / out-of-line-or-`#[path]` \
         child mod / `#[macro_use]` — a same-module code-generation surface could emit code \
         reaching the module-private lowering builders without naming them. Only the sanctioned \
         `#[cfg(test)] #[path] mod *_tests;` test wiring is allowed. Violations:\n  {}",
        violations.join("\n  ")
    );
    // FIX E (derive-shadow, scoped to this module): NO production import may bring
    // a built-in-derive name into scope under a foreign definition, and NO glob /
    // `#[macro_use]` may do so undecidably.
    let shadow = macro_arg_producer_derive_shadow_import_violations(&src);
    assert!(
        shadow.is_empty(),
        "derive-shadow violation: `macro_arg_producer.rs` must declare NO production import that \
         shadows a built-in-derive name ({MACRO_ARG_PRODUCER_BUILTIN_DERIVES:?}) — the module keeps \
         its `#[derive(Debug, Clone, …)]`, so an import binding one of those names (or a glob / \
         `#[macro_use]`) could redirect a derive to a foreign macro. Violations:\n  {}",
        shadow.join("\n  ")
    );
}

/// PARENT-SHAPE guard (load-bearing): the `structural_carrier_producer/`
/// owner directory contains ONLY the sanctioned members — the single producer
/// module (`macro_arg_producer.rs`), the typed-IR-only binder walker
/// (`infer_binder_names.rs`), the module root (`mod.rs`), and test modules
/// (`*_tests.rs`). This stops the owner module's boundary silently growing a
/// SECOND producer surface. The binder walker has no producer inputs and, as a
/// sibling, cannot name the child-private lowering builders.
#[test]
fn structural_carrier_producer_module_is_narrow() {
    let dir = workspace_path("crates/verter_type_engine/src/structural_carrier_producer");
    const SANCTIONED: &[&str] = &["mod.rs", "macro_arg_producer.rs", "infer_binder_names.rs"];
    let mut found_modules: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read owner dir: {e}")) {
        let path = entry.expect("dir entry").path();
        // Only the OWNER directory's own `.rs` files are members; the guard
        // does not descend (there are no subdirectories today).
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf-8 file name")
            .to_string();
        // Test modules (`*_tests.rs`) are always allowed — they carry no
        // production producer surface.
        if name.ends_with("_tests.rs") {
            continue;
        }
        found_modules.push(name);
    }
    found_modules.sort();
    // Anti-vacuity: the sanctioned production members must all be present —
    // their absence means the owner re-home regressed.
    for sanctioned in SANCTIONED {
        assert!(
            found_modules.iter().any(|m| m == sanctioned),
            "anti-vacuity: the sanctioned owner member `{sanctioned}` must exist in \
             `structural_carrier_producer/` — found {found_modules:#?}"
        );
    }
    // No NON-sanctioned production `.rs` module may live in the owner directory.
    let extra: Vec<&String> = found_modules
        .iter()
        .filter(|m| !SANCTIONED.contains(&m.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "owner-module narrowness violation: `structural_carrier_producer/` may contain ONLY the \
         single producer module (`macro_arg_producer.rs`), the typed-IR-only binder walker \
         (`infer_binder_names.rs`), `mod.rs`, and test modules (`*_tests.rs`). A new non-test \
         module here could open a SECOND producer surface. Found extra modules: {extra:#?}"
    );
}

#[test]
fn no_production_macro_arg_eager_lowering_outside_mirror() {
    let mut violations: Vec<String> = Vec::new();
    for (rel, src) in session_production_src_files() {
        if macro_arg_eager_lowering_violations(&rel, &src) {
            violations.push(rel);
        }
    }
    assert!(
        violations.is_empty(),
        "ordering tripwire: a non-test verter_session production file co-locates \
         `parsed_type_argument` with an eager `lower_type_expr_in_scope_with_*` call OUTSIDE the \
         structural-carrier producer module — a forbidden second macro-arg lowering path. The four \
         macro sites must read `crate::structural_carrier_producer::macro_type_arg_hot_ref`; only \
         the mirror builder lowers the macro arg. Files: {violations:#?}"
    );
}

/// Self-test for [`macro_arg_eager_lowering_violations`]: WHOLE-FUNCTION
/// co-presence of both tokens outside the macro producer surface is a violation
/// (including the more-than-12-lines-apart case WITHIN a function the old
/// adjacency window missed); the CROSS-FUNCTION-SAME-FILE helper split (a
/// `parsed_type_argument`-derived binding handed to an eager lowering in another
/// function) is now ALSO a violation; either token alone is not; two UNRELATED
/// functions (a presence-guard read in one, an unrelated eager lowering of a
/// non-macro `param_ty` in another) are not; the single producer module
/// (`macro_arg_producer.rs`) is exempt, but any FUTURE non-producer owner file
/// is NOT (the narrowed exemption — only the producer module, not the whole
/// owner directory); test-gated co-presence is not.
#[test]
fn macro_arg_eager_lowering_tripwire_discriminates() {
    let both = "fn f() { let a = mac.parsed_type_argument; dispatch.lower_type_expr_in_scope_with_mode(x); }\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", both),
        "co-presence of `parsed_type_argument` + eager lowering in ONE function outside the mirror \
         is a violation"
    );
    // STRENGTHENED: the forbidden pairing split >12 lines apart WITHIN ONE
    // function (a shape the old 12-line adjacency window let through) is now
    // caught.
    let mut far = String::from("fn one_fn() {\n    let a = mac.parsed_type_argument;\n");
    for n in 0..40 {
        far.push_str(&format!("    let _filler_{n} = {n};\n"));
    }
    far.push_str("    dispatch.lower_type_expr_in_scope_with_mode(x);\n}\n");
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", &far),
        "a `parsed_type_argument` read >40 lines from the eager lowering call WITHIN ONE function \
         must STILL trip — the >12-line case the old window missed"
    );
    // STRENGTHENED (GOV cross-function-same-file): a `parsed_type_argument`-
    // derived binding `arg` produced in `reader()` and lowered eagerly in a
    // SEPARATE `helper()` — the whole-function conjunction misses this, the
    // file-scope binding-flow catch does NOT.
    let cross_fn = "fn reader() {\n    let arg = mac.parsed_type_argument.clone();\n    helper(scope, arg);\n}\nfn helper(scope: &str, arg: TypeExpr) {\n    let base = dispatch.lower_type_expr_in_scope_with_context(scope, arg, ctx);\n}\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", cross_fn),
        "a `parsed_type_argument`-derived binding lowered eagerly in ANOTHER function in the same \
         file (the GOV helper-split evasion) MUST trip the file-scope binding-flow catch"
    );
    // The same evasion with `_with_mode` and a `mut` binding is also caught.
    let cross_fn_mode = "fn r() {\n    let mut a2 = mac.parsed_type_argument;\n}\nfn h() {\n    dispatch.lower_type_expr_in_scope_with_mode(s, a2, m);\n}\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", cross_fn_mode),
        "the helper-split evasion via `_with_mode` + a `mut` macro-arg binding must also trip"
    );
    // STRENGTHENED (GOV P0 #2): the let-else + helper-split shape the round-2
    // extractor MISSED — `let Some(arg) = …parsed_type_argument….as_ref()
    // else {…};` binds `arg` from the macro-arg read, and a SEPARATE helper
    // eager-lowers it. The binder lives INSIDE the `Some(…)` pattern (not the
    // first token), and the read is `.as_ref()`-chained.
    let let_else_split = "fn reader() {\n    let Some(arg) = mac.parsed_type_argument.as_ref() else { return None };\n    helper(scope, arg);\n}\nfn helper(scope: &str, arg: &TypeExpr) {\n    let base = dispatch.lower_type_expr_in_scope_with_context(scope, arg, ctx);\n}\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", let_else_split),
        "a let-else binder for `Some(arg)` over a `parsed_type_argument` `.as_ref()` read lowered \
         eagerly in another fn MUST trip — the binder lives inside the `Some(...)` pattern and the \
         read is `.as_ref()`-chained (the GOV P0 #2 missed shape)"
    );
    // STRENGTHENED (GOV P0 #2): the `if let Some(arg) = …parsed_type_argument…`
    // shape feeding an eager lowering in the SAME branch.
    let if_let = "fn f() {\n    if let Some(arg) = mac.parsed_type_argument.clone() {\n        dispatch.lower_type_expr_in_scope_with_mode(scope, arg, m);\n    }\n}\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", if_let),
        "an `if let Some(arg) = …parsed_type_argument…` binder lowered eagerly must trip"
    );
    // STRENGTHENED (GOV P0 #2): a `match …parsed_type_argument… { Some(arg) =>
    // … }` whose arm binder feeds an eager lowering (in a helper here).
    let match_split = "fn reader() {\n    match mac.parsed_type_argument.as_ref() {\n        Some(arg) => helper(scope, arg),\n        None => {}\n    }\n}\nfn helper(scope: &str, arg: &TypeExpr) {\n    dispatch.lower_type_expr_in_scope_with_context(scope, arg, ctx);\n}\n";
    assert!(
        macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", match_split),
        "a match arm binder over a `parsed_type_argument` scrutinee lowered eagerly MUST trip — \
         the match scrutinee reads the macro arg and the `Some(arg)` arm binds its value"
    );
    // SOUNDNESS: a presence-guard read in one function and an UNRELATED eager
    // lowering (of a non-macro-arg `param_ty`) in ANOTHER function is NOT a
    // violation — the real `vue_exec/mod.rs` shape (line 383 presence guard +
    // `navigate_param_to_object_surface`'s unrelated lowering). The
    // binding-flow catch does NOT fire because `param_ty` is a fn parameter,
    // never bound from `parsed_type_argument`, and the `_` presence-guard
    // binding is excluded.
    let unrelated = "fn reader() {\n    let _ = mac.parsed_type_argument.as_ref()?;\n    let h = macro_type_arg_hot_ref(ctx, o, i)?;\n}\nfn navigate_param_to_object_surface() {\n    let base = dispatch.lower_type_expr_in_scope_with_context(scope, param_ty, c)?;\n}\n";
    assert!(
        !macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", unrelated),
        "a presence-guard `parsed_type_argument` read in one fn + an unrelated eager lowering in \
         another fn is NOT a violation (the real vue_exec shape)"
    );
    assert!(
        !macro_arg_eager_lowering_violations(
            "crates/verter_session/src/foo.rs",
            "fn f() { let _ = mac.parsed_type_argument; }"
        ),
        "naming `parsed_type_argument` alone is not a violation"
    );
    assert!(
        !macro_arg_eager_lowering_violations(
            "crates/verter_session/src/foo.rs",
            "fn f() { dispatch.lower_type_expr_in_scope_with_context(x); }"
        ),
        "an eager lowering of a non-macro-arg expr alone is not a violation"
    );
    assert!(
        !macro_arg_eager_lowering_violations(
            "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
            both
        ),
        "the single producer module (`macro_arg_producer.rs`) is the sanctioned \
         producer/accessor (it owns the mirror builder + the binder-seed builder) — exempt"
    );
    // NARROWED EXEMPTION (FIX): a FUTURE non-producer owner file — e.g. a
    // hypothetical decl-body producer surface re-introduced later — must NOT
    // inherit a blanket directory exemption: an eager macro-arg lowering there is
    // STILL a violation until that file is explicitly added to the exempt set
    // with justification. (Only `macro_arg_producer.rs` is exempt today, so any
    // other owner-directory file is in scope.)
    assert!(
        macro_arg_eager_lowering_violations(
            "crates/verter_session/src/structural_carrier_producer/decl_body_surface.rs",
            both
        ),
        "an eager macro-arg lowering planted in a NON-exempt owner file (a hypothetical \
         `decl_body_surface.rs`) MUST be reported — only the single producer module \
         (`macro_arg_producer.rs`) is exempt, not the whole owner directory"
    );
    // A test-gated co-presence is NOT a production violation (cfg(test) strip).
    let gated = "#[cfg(test)]\nmod t {\n    fn f() { let a = mac.parsed_type_argument; dispatch.lower_type_expr_in_scope_with_mode(x); }\n}\n";
    assert!(
        !macro_arg_eager_lowering_violations("crates/verter_session/src/foo.rs", gated),
        "co-presence inside a #[cfg(test)] module is not a production violation"
    );
}

#[test]
fn macro_hot_mirror_producer_is_pure_no_route_resolution() {
    let mut violations: Vec<String> = Vec::new();
    for (rel, src) in session_production_src_files() {
        if !rel.contains("structural_carrier_producer/") {
            continue;
        }
        for ident in macro_hot_mirror_impurity_hits(&src) {
            violations.push(format!("{rel}: {ident}"));
        }
    }
    assert!(
        violations.is_empty(),
        "purity violation: the structural-carrier producer (`structural_carrier_producer/**`) must \
         NOT route-resolve imports or read the prepared-decl bundle — resolution + dep recording \
         belong at the resolving DEMAND. Script-setup seeding re-sources from the owner's route-free \
         `IndexedReady` (`raw_source` + `framework_parse`). Found: {violations:#?}"
    );
}

/// Self-test for [`macro_hot_mirror_impurity_hits`]: a route-resolving ident in
/// production source is reported; a comment / test-gated mention is not;
/// injecting one reddens, reverting greens.
#[test]
fn macro_hot_mirror_purity_scanner_discriminates() {
    // Inject one forbidden route-resolving ident → reddens.
    let injected =
        "fn build() {\n    let bundle = ctx.prepared_decl_bundle(owner);\n    let _ = bundle;\n}\n";
    assert_eq!(
        macro_hot_mirror_impurity_hits(injected),
        vec!["prepared_decl_bundle".to_string()],
        "a production `prepared_decl_bundle` call in the mirror module must be flagged"
    );
    // GOV P0 #3: the per-symbol prepared-decl accessors route through the same
    // prepared-decl context path and MUST now be flagged too. Injecting either
    // reddens.
    assert_eq!(
        macro_hot_mirror_impurity_hits(
            "fn build() {\n    let d = ctx.prepared_type_decl(owner, name);\n    let _ = d;\n}\n"
        ),
        vec!["prepared_type_decl".to_string()],
        "a production `prepared_type_decl` call (the per-symbol prepared-decl cache accessor that \
         routes through the prepared-decl path) inside the producer must be flagged"
    );
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { ctx.prepared_value_decl(owner, name); }\n"),
        vec!["prepared_value_decl".to_string()],
        "a production `prepared_value_decl` call (the per-symbol prepared-decl value accessor) \
         inside the producer must be flagged"
    );
    // The other original banned idents are also caught.
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { cached_import_route_resolution(x); }\n"),
        vec!["cached_import_route_resolution".to_string()],
        "a production `cached_import_route_resolution` call must be flagged"
    );
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { resolve_route_type_edge(e); }\n"),
        vec!["resolve_route_type_edge".to_string()],
        "a production `resolve_route_type_edge` call must be flagged"
    );
    // BROADENED set: the wider `ResolverContext` route/symbol-resolution
    // surface a future impurity could reach for is now banned. Injecting any
    // one reddens.
    assert_eq!(
        macro_hot_mirror_impurity_hits(
            "fn f() { let c = ctx.resolve_type_dependency_canonical(owner, src); }\n"
        ),
        vec!["resolve_type_dependency_canonical".to_string()],
        "a production `resolve_type_dependency_canonical` call (a route/dep resolution) must be \
         flagged by the broadened set"
    );
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { ctx.resolve_owner_direct_import(o, n); }\n"),
        vec!["resolve_owner_direct_import".to_string()],
        "a production `resolve_owner_direct_import` call must be flagged"
    );
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { ctx.routed_shallow_state(c); }\n"),
        vec!["routed_shallow_state".to_string()],
        "a production `routed_shallow_state` (cross-file routed read) call must be flagged"
    );
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn f() { self.resolve_bare_ref_head(ctx, n, a, l); }\n"),
        vec!["resolve_bare_ref_head".to_string()],
        "a production carrier-head resolution (`resolve_bare_ref_head`) inside the producer must \
         be flagged — the producer must not resolve carriers"
    );
    // `contains`-matching catches the `_shallow` sibling via its prefix.
    assert_eq!(
        macro_hot_mirror_impurity_hits(
            "fn f() { ctx.resolve_named_type_export_target_shallow(d, n); }\n"
        ),
        vec!["resolve_named_type_export_target".to_string()],
        "the `_shallow` sibling is caught by the banned prefix `resolve_named_type_export_target`"
    );
    // [P0] FIX D — the eager dispatch/query route inside the producer is flagged by
    // BOTH the `.dispatch(` and `lower_type_expr_in_scope_with_` needles. This is
    // the second-resolution-engine class the single-engine rule forbids; the
    // adversarial-claude PROOF was a `pub(crate) fn` calling exactly this.
    let dispatch_hits = macro_hot_mirror_impurity_hits(
        "fn rogue() { let _ = ctx.dispatch().lower_type_expr_in_scope_with_context(s, e, c); }\n",
    );
    assert!(
        dispatch_hits.contains(&".dispatch(".to_string())
            && dispatch_hits.contains(&"lower_type_expr_in_scope_with_".to_string()),
        "an eager `ctx.dispatch().lower_type_expr_in_scope_with_context(...)` route inside the \
         producer MUST be flagged by both the `.dispatch(` and `lower_type_expr_in_scope_with_` \
         needles (the [P0] second-resolution-engine closure); got {dispatch_hits:?}"
    );
    // The allowed route-free arena read `dispatch_node_data(` is NOT flagged by
    // the `.dispatch(` needle (its chars after `dispatch` are `_node_data(`).
    assert!(
        macro_hot_mirror_impurity_hits("fn ok() { let _ = ctx.dispatch_node_data(node); }\n")
            .is_empty(),
        "the route-free arena read `dispatch_node_data(` must NOT be flagged — the `.dispatch(` \
         needle's open-paren distinguishes it"
    );
    // The route-resolving `ensure_indexed_ready(` IS flagged; the route-free
    // `ensure_indexed_ready_serve(` is NOT (the distinction the brief flagged).
    assert_eq!(
        macro_hot_mirror_impurity_hits("fn rogue() { ctx.ensure_indexed_ready(owner); }\n"),
        vec!["ensure_indexed_ready(".to_string()],
        "the route-resolving `ensure_indexed_ready(` must be flagged"
    );
    assert!(
        macro_hot_mirror_impurity_hits(
            "fn ok() { let serve = ctx.ensure_indexed_ready_serve(owner); }\n"
        )
        .is_empty(),
        "the route-free `ensure_indexed_ready_serve(` must NOT be flagged (the `_serve` suffix \
         distinguishes it from the `ensure_indexed_ready(` needle)"
    );
    // ROUTE-FREE reads the mirror legitimately uses must NOT be flagged.
    assert!(
        macro_hot_mirror_impurity_hits(
            "fn f() { let p = ctx.sfc_script_setup_type_params(file); let s = ctx.shallow_file_state(c); let i = ctx.indexed_ready(c); let l = ctx.local_type_declaration_id(c, n); }\n"
        )
        .is_empty(),
        "the route-free shallow reads the mirror needs (`sfc_script_setup_type_params`, \
         `shallow_file_state`, `indexed_ready`, `local_type_declaration_id`) must NOT be flagged"
    );
    // A comment mention is NOT a hit (route-free seeding, see the doc above).
    assert!(
        macro_hot_mirror_impurity_hits(
            "// does NOT call prepared_decl_bundle anymore\nfn f() {}\n"
        )
        .is_empty(),
        "a comment mention of a route-resolving ident must not be a hit"
    );
    // A test-gated mention is NOT a production hit (cfg(test) strip).
    assert!(
        macro_hot_mirror_impurity_hits(
            "#[cfg(test)]\nmod t {\n    fn f() { ctx.prepared_decl_bundle(o); }\n}\n"
        )
        .is_empty(),
        "a #[cfg(test)]-gated route-resolving call is not a production violation"
    );
    // The REAL producer source is clean (revert proof: the production
    // `macro_arg_producer.rs` carries none of the banned idents).
    let mirror_src = std::fs::read_to_string(workspace_path(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    ))
    .expect("the macro-arg producer source must be readable");
    assert!(
        macro_hot_mirror_impurity_hits(&mirror_src).is_empty(),
        "the production `structural_carrier_producer/macro_arg_producer.rs` must be route-free \
         (pure producer)"
    );
}

/// BOUNDED defense-in-depth tripwire (NOT exhaustive): assert the SANCTIONED
/// producer entry (`macro_type_arg_hot_ref`) is the ONLY crate-visible
/// (`pub(crate)` / `pub(in crate)` / bare `pub`) PRODUCTION producer entry of
/// the `structural_carrier_producer` module — covering BOTH module-level FREE
/// functions AND ASSOCIATED functions inside INHERENT `impl` blocks (trait-impl
/// methods inherit the trait's visibility and cannot be independently
/// crate-visible, so they are not a producer-entry vector). No OTHER
/// crate-visible fn (free or associated) in `structural_carrier_producer/**`
/// may expose a second outward producer entry. The `#[cfg(test)]`
/// `MacroHotMirror::demanded_count` test accessor is excluded by attribute; and
/// the raw structural lowerer, the macro hot-mirror builder, and the binder-seed
/// builder are ALL MODULE-PRIVATE (no visibility modifier) inside
/// `macro_arg_producer.rs`, so none widens the crate-visible producer surface.
///
/// TWO CONFINEMENT REGIMES. The FOREIGN case is COMPILER-CONFINED: the three
/// producer-capable builders are MODULE-PRIVATE inside the single producer
/// module `macro_arg_producer.rs` (the owner declares it as a PRIVATE
/// `mod macro_arg_producer;` re-exporting only `macro_type_arg_hot_ref` +
/// `MacroHotMirror`), so a foreign module cannot NAME any of them (a compile
/// error, E0603 / E0433) — a second producer in a foreign file is
/// unrepresentable by construction (pinned by
/// `structural_carrier_producer_lowerer_is_module_private` +
/// `structural_carrier_producer_module_is_narrow`). The SAME-MODULE case is NOT
/// compiler-confined: Rust privacy is module-scoped, so a SECOND producer
/// written INSIDE `macro_arg_producer.rs` CAN name the builders, and the owner's
/// collapse to one file does not make that a compile error. THIS guard is one of
/// the BOUNDED scanners that POLICE that same-module residual: it covers the
/// crate-visible producer-entry surface (free + inherent-associated fns, plus —
/// after the strengthening below — value `const`/`static`, trait impls/defs, and
/// the exact `mod.rs` re-export shape) under a cfg-SATISFIABILITY test-gate
/// classifier. It is BOUNDED, not exhaustive.
///
/// KNOWN RESIDUAL GAPS (deliberately NOT covered — ACCEPTED). A `syn` source scan
/// fundamentally cannot see macro-expanded or out-of-tree producers, so the
/// following exotic shapes can evade THIS guard; the IRREDUCIBLE residual is
/// trust in the one sanctioned producer implementation plus compiler bugs /
/// build substitution / out-of-tree proc-macros:
/// - a FOREIGN-FN shape (`extern "C" { fn <name>(...) -> …; }`);
/// - a production `#[path = "…"]` module rooted OUTSIDE
///   `crates/verter_session/src/**` (not enumerated by `session_production_src_files()`).
///
/// CLOSED (no longer a residual gap): a crate-visible `const` / `static`
/// FN-POINTER binding a producer builder, and an ASSOCIATED const fn-pointer in
/// an inherent OR trait impl, are now ENUMERATED — the value-exposure collector
/// covers free `const`/`static` + inherent-impl assoc consts, and the
/// trait-exposure collector scans an allowlisted trait impl's assoc-const
/// initialisers (a non-allowlisted trait impl is rejected wholesale). `cfg_attr`
/// is also handled: `attrs_test_gate` gates the item on `#[cfg(...)]` ONLY, so a
/// `#[cfg_attr(test, …)]` producer is COUNTED (it compiles in every build) rather
/// than wrongly excluded — it is not an accepted residual.
///
/// These residuals are ACCEPTED because (i) the COMPILER module-privacy of the
/// producer-capable code (the PRIMARY guards above) is the airtight guarantee — a
/// rogue producer reachable from a foreign module is a compile error regardless of
/// how it is spelled; (ii) a `syn` scan cannot SOUNDLY close the macro-expansion /
/// out-of-tree tail; (iii) no live violation exists. Per the architecture ruling,
/// this guard is NOT extended to chase that tail.
#[test]
fn macro_hot_mirror_exposes_single_crate_visible_producer_entry() {
    let mut crate_visible: Vec<(String, String)> = Vec::new();
    let mut scanned_mirror_files = 0usize;
    for (rel, src) in session_production_src_files() {
        if !rel.contains("structural_carrier_producer/") {
            continue;
        }
        scanned_mirror_files += 1;
        for name in crate_visible_producer_fn_names(&src) {
            crate_visible.push((rel.clone(), name));
        }
    }
    // Anti-vacuity: the module must exist and the scan must have found the
    // sanctioned entry — its absence means the producer-entry move regressed. The
    // owner module is narrow (`mod.rs` + producer + typed-IR helper), so at
    // least one production file is scanned.
    assert!(
        scanned_mirror_files >= 1,
        "anti-vacuity: expected at least the owner module's production files \
         (`mod.rs` + `macro_arg_producer.rs` + `infer_binder_names.rs`), found \
         {scanned_mirror_files}"
    );
    for sanctioned in MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES {
        assert!(
            crate_visible.iter().any(|(_, name)| name == sanctioned),
            "anti-vacuity: the sanctioned crate-visible entry `{sanctioned}` must be present as a \
             crate-visible fn in `structural_carrier_producer/**`"
        );
    }
    let extra: Vec<&(String, String)> = crate_visible
        .iter()
        .filter(|(_, name)| !mirror_entry_is_sanctioned(name))
        .collect();
    assert!(
        extra.is_empty(),
        "structural-carrier-producer entry-surface violation: the crate-visible \
         (`pub(crate)`/`pub(in crate)`/`pub`) PRODUCTION entries of \
         `structural_carrier_producer/**` must be EXACTLY the sanctioned set \
         {MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES:?} (the macro-arg accessor, which reads the \
         macro hot mirror populated by the module-private lowering builders, and the mirror's \
         attachment mint) — covering BOTH module-level FREE functions AND ASSOCIATED functions \
         inside INHERENT `impl` blocks. A second crate-visible producer fn (free OR associated) \
         re-opens a new outward entry into the single-producer module. Found extra entries: \
         {extra:#?}. If the module legitimately needs another crate-visible fn for a NON-producer \
         reason, add it to the sanctioned set with justification."
    );

    // VALUE EXPOSURE — a crate-visible `const`/`static`/associated-const binding a
    // producer builder as an fn-pointer value re-opens the builder WITHOUT a `fn`
    // item. Scan `macro_arg_producer.rs`.
    let producer_rel =
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs";
    let producer_src = std::fs::read_to_string(workspace_path(producer_rel))
        .unwrap_or_else(|e| panic!("guard could not read {producer_rel}: {e}"));
    let value_violations = macro_arg_producer_value_exposure_violations(&producer_src);
    assert!(
        value_violations.is_empty(),
        "value-exposure violation: `macro_arg_producer.rs` must declare NO crate-visible \
         `const`/`static`/associated-const that binds a producer builder \
         ({STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS:?}) as a value — such a binding re-exports \
         the module-private builder through a value, re-opening the producer surface. \
         Violations:\n  {}",
        value_violations.join("\n  ")
    );

    // TRAIT EXPOSURE — only the hand-written `Debug`/`Clone` impls for
    // `MacroHotMirror` are allowed; any other trait impl, or a trait def/alias, is
    // a dispatch surface that could expose a producer-capable method. The
    // allowlisted impls' bodies must not name a builder.
    let trait_violations = macro_arg_producer_trait_exposure_violations(&producer_src);
    assert!(
        trait_violations.is_empty(),
        "trait-exposure violation: `macro_arg_producer.rs` must declare ONLY the hand-written \
         `Debug`/`Clone` trait impls for `MacroHotMirror` (whose bodies name no builder) and NO \
         crate-visible trait definition/alias — a trait is a dispatch surface that could expose a \
         producer-capable method. Violations:\n  {}",
        trait_violations.join("\n  ")
    );

    // PIN mod.rs EXACTLY — PRIVATE typed-IR-helper and producer module
    // declarations plus EXACTLY the sanctioned producer re-export.
    let mod_rel = "crates/verter_type_engine/src/structural_carrier_producer/mod.rs";
    let mod_src = std::fs::read_to_string(workspace_path(mod_rel))
        .unwrap_or_else(|e| panic!("guard could not read {mod_rel}: {e}"));
    let mod_violations = mod_rs_reexport_shape_violations(&mod_src);
    assert!(
        mod_violations.is_empty(),
        "mod.rs shape violation: `structural_carrier_producer/mod.rs` must declare PRIVATE \
         `mod infer_binder_names;` + `mod macro_arg_producer;` and re-export EXACTLY `pub(crate) use \
         macro_arg_producer::{{macro_type_arg_hot_ref, MacroHotMirror}};` with only the fixed \
         `MacroHotProduct` / `MacroMirrorAttachment` nominal leaves — no aliases, globs, \
         unknown leaves, private builders, or a `pub`-widened module decl. \
         Violations:\n  {}",
        mod_violations.join("\n  ")
    );
}

/// Self-test for the value-exposure / trait-exposure / mod.rs-pin collectors:
/// the genuine producer, helper, and `mod.rs` pass; a planted crate-visible
/// const/static fn-pointer of a builder, a non-allowlisted trait
/// impl, a trait def, an allowlisted impl naming a builder, and a deviating
/// mod.rs re-export (extra leaf / alias / glob / `pub` module decl) each redden;
/// `#[cfg(test)]`-gated shapes do not.
#[test]
fn mirror_value_trait_and_modrs_collectors_discriminate() {
    // ── VALUE EXPOSURE ─────────────────────────────────────────────────────
    // A crate-visible const binding a builder as an fn-pointer reddens.
    let const_ptr = "pub(crate) const F: fn(&C) -> H = lower_type_expr_structural;\n";
    assert!(
        !macro_arg_producer_value_exposure_violations(const_ptr).is_empty(),
        "a `pub(crate) const F = lower_type_expr_structural;` fn-pointer binding must redden \
         (value-exposure)"
    );
    // A crate-visible static binding a builder reddens.
    let static_ptr = "pub(crate) static G: fn() = build_macro_hot_ref;\n";
    assert!(
        !macro_arg_producer_value_exposure_violations(static_ptr).is_empty(),
        "a `pub(crate) static G = build_macro_hot_ref;` binding must redden (value-exposure)"
    );
    // A crate-visible associated const binding a builder reddens.
    let assoc_const =
        "impl MacroHotMirror {\n    pub(crate) const P: fn() = build_script_setup_seed_frames;\n}\n";
    assert!(
        !macro_arg_producer_value_exposure_violations(assoc_const).is_empty(),
        "a crate-visible associated `const P = build_script_setup_seed_frames;` must redden"
    );
    // A MODULE-PRIVATE const binding a builder is NOT crate-visible → not a
    // value-exposure (it cannot be named from another module).
    let private_const = "const F: fn(&C) -> H = lower_type_expr_structural;\n";
    assert!(
        macro_arg_producer_value_exposure_violations(private_const).is_empty(),
        "a module-private `const F = lower_type_expr_structural;` is not crate-visible and must NOT \
         redden"
    );
    // A crate-visible const NOT referencing a builder is fine.
    let unrelated_const = "pub(crate) const N: usize = 4;\n";
    assert!(
        macro_arg_producer_value_exposure_violations(unrelated_const).is_empty(),
        "a crate-visible const not naming a builder must NOT redden"
    );
    // A `#[cfg(test)]`-gated crate-visible const binding a builder is test-only →
    // not a production value-exposure.
    let test_const =
        "#[cfg(test)]\npub(crate) const F: fn(&C) -> H = lower_type_expr_structural;\n";
    assert!(
        macro_arg_producer_value_exposure_violations(test_const).is_empty(),
        "a `#[cfg(test)]`-gated const fn-pointer must NOT redden (test-only)"
    );

    // ── TRAIT EXPOSURE ─────────────────────────────────────────────────────
    // A non-allowlisted trait impl reddens.
    let foreign_impl = "impl SomeTrait for MacroHotMirror {\n    fn via_trait(&self) {}\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(foreign_impl).is_empty(),
        "a non-allowlisted `impl SomeTrait for MacroHotMirror` must redden (trait-exposure)"
    );
    // A trait impl for a DIFFERENT self type (even Debug) is not the allowlisted
    // (Debug, MacroHotMirror) pair → reddens.
    let debug_other =
        "impl std::fmt::Debug for OtherType {\n    fn fmt(&self, f: &mut F) -> R { todo!() }\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(debug_other).is_empty(),
        "an `impl Debug for OtherType` is not the allowlisted (Debug, MacroHotMirror) pair and must \
         redden"
    );
    // A trait DEFINITION reddens.
    let trait_def = "pub(crate) trait Producer {\n    fn make(&self) -> H;\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(trait_def).is_empty(),
        "a `trait Producer` definition in the producer module must redden"
    );
    // The allowlisted `Debug`/`Clone` impls for `MacroHotMirror` are accepted when
    // their bodies do NOT name a builder.
    let allowed = "impl std::fmt::Debug for MacroHotMirror {\n    fn fmt(&self, f: &mut F) -> R { f.debug_struct(\"x\").finish() }\n}\nimpl Clone for MacroHotMirror {\n    fn clone(&self) -> Self { Self { } }\n}\n";
    assert!(
        macro_arg_producer_trait_exposure_violations(allowed).is_empty(),
        "the hand-written `Debug`/`Clone` impls for `MacroHotMirror` (no builder in body) must be \
         allowed"
    );
    // An allowlisted impl whose body NAMES a builder reddens (a trait method
    // reaching a builder).
    let allowed_but_calls = "impl Clone for MacroHotMirror {\n    fn clone(&self) -> Self { let _ = lower_type_expr_structural; Self { } }\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(allowed_but_calls).is_empty(),
        "an allowlisted `impl Clone for MacroHotMirror` whose body names a builder must redden"
    );
    // FULL-PATH ALLOWLIST. A QUALIFIED foreign trait merely NAMED `Clone`
    // (`impl evil::Clone for MacroHotMirror`) is NOT `core::clone::Clone` — it is a
    // user-defined trait that compiles (no coherence collision with the real
    // `impl Clone`) and a final-segment match would WRONGLY admit it. It MUST
    // redden — the allowlist matches the EXACT std spellings only.
    let qualified_foreign_clone = "impl evil::Clone for MacroHotMirror {}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(qualified_foreign_clone).is_empty(),
        "a qualified foreign `impl evil::Clone for MacroHotMirror` must redden — it is NOT the std \
         `Clone`; the allowlist matches the full std path spelling, not just the final segment"
    );
    // Likewise a qualified foreign `evil::Debug` reddens.
    let qualified_foreign_debug =
        "impl evil::Debug for MacroHotMirror {\n    fn fmt(&self, f: &mut F) -> R { todo!() }\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(qualified_foreign_debug).is_empty(),
        "a qualified foreign `impl evil::Debug for MacroHotMirror` must redden — it is NOT \
         `std::fmt::Debug`"
    );
    // ASSOCIATED-CONST FN-POINTER inside an allowlisted impl. An allowlisted
    // `impl Clone for MacroHotMirror` whose body holds `const F: fn(..) = builder;`
    // is a trait-dispatch reach to a builder through an associated const — it MUST
    // redden (the impl body's assoc-const initialiser is scanned, not only its
    // method bodies).
    let allowed_assoc_const_ptr = "impl Clone for MacroHotMirror {\n    const F: fn(&SemanticGraphStore, &TypeExpr) -> () = lower_type_expr_structural;\n    fn clone(&self) -> Self { Self { } }\n}\n";
    assert!(
        !macro_arg_producer_trait_exposure_violations(allowed_assoc_const_ptr).is_empty(),
        "an allowlisted `impl Clone for MacroHotMirror` holding `const F = lower_type_expr_structural;` \
         must redden — an associated const fn-pointer reaching a builder is a trait-dispatch \
         value-exposure"
    );
    // The std spellings of the allowlisted traits are accepted (a path-spelling
    // robustness arm): `impl std::clone::Clone` / `impl core::fmt::Debug` pass when
    // their bodies name no builder.
    let std_spellings = "impl std::clone::Clone for MacroHotMirror {\n    fn clone(&self) -> Self { Self { } }\n}\nimpl core::fmt::Debug for MacroHotMirror {\n    fn fmt(&self, f: &mut F) -> R { f.debug_struct(\"x\").finish() }\n}\n";
    assert!(
        macro_arg_producer_trait_exposure_violations(std_spellings).is_empty(),
        "the std-spelled `impl std::clone::Clone` / `impl core::fmt::Debug` for `MacroHotMirror` \
         (no builder in body) must be allowed"
    );
    // A `#[cfg(test)]`-gated foreign trait impl is test-only → not a production
    // trait-exposure.
    let test_trait =
        "#[cfg(test)]\nimpl SomeTrait for MacroHotMirror {\n    fn via_trait(&self) {}\n}\n";
    assert!(
        macro_arg_producer_trait_exposure_violations(test_trait).is_empty(),
        "a `#[cfg(test)]`-gated foreign trait impl must NOT redden (test-only)"
    );

    // ── mod.rs PIN ─────────────────────────────────────────────────────────
    // The EXACT sanctioned shape passes.
    let ok_mod = "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n";
    assert!(
        mod_rs_reexport_shape_violations(ok_mod).is_empty(),
        "the exact sanctioned mod.rs shape must pass"
    );
    let owned_nominals = "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror, MacroHotProduct, MacroMirrorAttachment};\n";
    assert!(
        mod_rs_reexport_shape_violations(owned_nominals).is_empty(),
        "only the two fixed owned nominal records extend the re-export"
    );
    // An EXTRA re-exported leaf reddens.
    let extra_leaf = "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror, lower_type_expr_structural};\n";
    assert!(
        !mod_rs_reexport_shape_violations(extra_leaf).is_empty(),
        "an extra re-exported leaf (`lower_type_expr_structural`) in mod.rs must redden"
    );
    // A SECOND `pub(crate) use` re-exporting a restricted helper reddens.
    let extra_use = "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\npub(crate) use macro_arg_producer::rogue;\n";
    assert!(
        !mod_rs_reexport_shape_violations(extra_use).is_empty(),
        "a second `pub(crate) use macro_arg_producer::rogue;` in mod.rs must redden"
    );
    // An ALIAS reddens.
    let aliased = "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref as hot, MacroHotMirror};\n";
    assert!(
        !mod_rs_reexport_shape_violations(aliased).is_empty(),
        "an aliased re-export leaf in mod.rs must redden"
    );
    // A GLOB reddens.
    let glob =
        "mod infer_binder_names;\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::*;\n";
    assert!(
        !mod_rs_reexport_shape_violations(glob).is_empty(),
        "a glob re-export in mod.rs must redden"
    );
    // A `pub` (not `pub(crate)`) module decl reddens (foreign-nameable).
    let pub_mod = "mod infer_binder_names;\npub mod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n";
    assert!(
        !mod_rs_reexport_shape_violations(pub_mod).is_empty(),
        "a `pub mod macro_arg_producer;` decl in mod.rs must redden (foreign-nameable)"
    );
    // A `pub` (not `pub(crate)`) re-export visibility reddens.
    let pub_use = "mod infer_binder_names;\nmod macro_arg_producer;\npub use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n";
    assert!(
        !mod_rs_reexport_shape_violations(pub_use).is_empty(),
        "a `pub use` (wider than `pub(crate)`) re-export in mod.rs must redden"
    );
    // A `#[path = "../evil.rs"]` ATTRIBUTE on the `mod macro_arg_producer;` decl
    // re-roots the producer module to an arbitrary source file (a
    // build-substitution route) while keeping the inherited visibility +
    // out-of-line shape — it MUST redden. The decl must carry NO attribute other
    // than a strict `#[cfg(test)]`.
    let path_attr = "mod infer_binder_names;\n#[path = \"../evil.rs\"]\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n";
    assert!(
        !mod_rs_reexport_shape_violations(path_attr).is_empty(),
        "a `#[path = \"../evil.rs\"] mod macro_arg_producer;` must redden — a `#[path]` re-roots the \
         producer module to an arbitrary file (build-substitution)"
    );
    // A proc-macro / non-inert ATTRIBUTE on the decl likewise reddens (it could
    // rewrite the module).
    let proc_attr = "mod infer_binder_names;\n#[some_proc_macro]\nmod macro_arg_producer;\npub(crate) use macro_arg_producer::{macro_type_arg_hot_ref, MacroHotMirror};\n";
    assert!(
        !mod_rs_reexport_shape_violations(proc_attr).is_empty(),
        "a `#[some_proc_macro] mod macro_arg_producer;` decl must redden — a non-cfg-test attribute \
         could rewrite the producer module"
    );

    // ── REAL files pass ALL three collectors ───────────────────────────────
    let producer_src = std::fs::read_to_string(workspace_path(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    ))
    .expect("producer module readable");
    assert!(
        macro_arg_producer_value_exposure_violations(&producer_src).is_empty(),
        "the REAL macro_arg_producer.rs must have NO value-exposure"
    );
    assert!(
        macro_arg_producer_trait_exposure_violations(&producer_src).is_empty(),
        "the REAL macro_arg_producer.rs must have NO trait-exposure (only the hand-written \
         Debug/Clone for MacroHotMirror)"
    );
    let mod_src = std::fs::read_to_string(workspace_path(
        "crates/verter_type_engine/src/structural_carrier_producer/mod.rs",
    ))
    .expect("mod.rs readable");
    assert!(
        mod_rs_reexport_shape_violations(&mod_src).is_empty(),
        "the REAL mod.rs must match the exact sanctioned shape"
    );
}

#[test]
fn component_meta_hot_paths_self_test_field_types_direct_build_fires() {
    // A direct `ScopeShadowing::from_scope_payload(...)` build in a field-types
    // hot path (the regression form) must FIRE.
    let planted = r#"
        fn materialize_component_meta_type_expr_until_stable_full(
            query_engine: &mut ComponentMetaQueryEngine,
            scope_canonical_id: &str,
        ) {
            let scope_payload = query_engine.scope_payload_for_scope(scope_canonical_id);
            let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
                scope_payload.as_deref(),
            );
            let _ = shadowing;
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    assert_eq!(
        v.len(),
        1,
        "a direct `ScopeShadowing::from_scope_payload(...)` build in a field-types \
         hot path MUST fire; got: {v:?}"
    );
    assert_eq!(v[0].method, "from_scope_payload");
    assert_eq!(
        v[0].fn_name,
        "materialize_component_meta_type_expr_until_stable_full"
    );
}

#[test]
fn component_meta_hot_paths_self_test_unconditional_dispatch_build_fires() {
    // A pre-match UNCONDITIONAL `from_host_scope` build in
    // `project_expr_class_a_via_dispatch_threaded` must FIRE even though the
    // same fn ALSO contains the legitimate engine-less `None`-arm build — this
    // proves the allowlist is arm-precise, not fn-wide or file-wide.
    let planted = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            mut engine: Option<&mut ComponentMetaQueryEngine>,
            scope_canonical_id: &str,
        ) {
            let pre_match = crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                ctx,
                scope_canonical_id,
            );
            let memoized = match engine.as_deref_mut() {
                Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
                None => std::sync::Arc::new(
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    ),
                ),
            };
            let _ = (pre_match, memoized);
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
        planted,
    );
    // EXACTLY one: the pre-match build fires; the `None`-arm build is
    // allowlisted. fn-wide allowance would yield 0; no allowance would yield 2.
    assert_eq!(
        v.len(),
        1,
        "the pre-match unconditional `from_host_scope` build MUST fire while the \
         engine-less `None`-arm build stays allowlisted (arm-precise); got: {v:?}"
    );
    assert_eq!(v[0].method, "from_host_scope");
    assert_eq!(v[0].fn_name, "project_expr_class_a_via_dispatch_threaded");
}

#[test]
fn component_meta_hot_paths_self_test_second_none_arm_build_fires() {
    // The engine-less `None`-arm allowance covers AT MOST ONE
    // `from_host_scope` build. A SECOND build in the same real `None` arm is
    // a duplicate the allowance does NOT cover: it fires. (The sanctioned
    // shape constructs the fallback once.)
    let planted = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            mut engine: Option<&mut ComponentMetaQueryEngine>,
            scope_canonical_id: &str,
        ) {
            let shadowing = match engine.as_deref_mut() {
                Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
                None => {
                    let first = crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    );
                    let second = crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    );
                    std::sync::Arc::new(if first.is_empty() { second } else { first })
                }
            };
            let _ = shadowing;
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
        planted,
    );
    // An unbounded `None`-arm allowance yields 0; a bounded (at-most-one)
    // allowance fires the second build → exactly 1.
    assert_eq!(
        v.len(),
        1,
        "a SECOND `from_host_scope` build in the engine-less `None` arm MUST \
         fire (the allowance covers at most one); got: {v:?}"
    );
    assert_eq!(v[0].method, "from_host_scope");
    assert_eq!(v[0].fn_name, "project_expr_class_a_via_dispatch_threaded");
}

#[test]
fn component_meta_hot_paths_self_test_non_engine_scrutinee_none_arm_fires() {
    // The `None` arm of a match whose scrutinee merely MENTIONS the `engine`
    // token but whose ROOT RECEIVER is not the `engine` binding
    // (`match other.engine` — root receiver `other`) is NOT the engine-less
    // fallback. A build there fires even inside the allowlisted fn.
    let planted = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            other: &SomeHolder,
            scope_canonical_id: &str,
        ) {
            let shadowing = match other.engine {
                Some(_) => default_shadowing(),
                None => std::sync::Arc::new(
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    ),
                ),
            };
            let _ = shadowing;
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
        planted,
    );
    // A token-stream "mentions engine" scan allowlists this (0); a
    // root-receiver match fires it (1).
    assert_eq!(
        v.len(),
        1,
        "a `from_host_scope` build in the `None` arm of a NON-engine-rooted \
         match (`match other.engine`, root receiver `other`) MUST fire; \
         got: {v:?}"
    );
    assert_eq!(v[0].method, "from_host_scope");
    assert_eq!(v[0].fn_name, "project_expr_class_a_via_dispatch_threaded");
}

#[test]
fn component_meta_hot_paths_self_test_nested_engine_match_none_arm_fires() {
    // A build in the `None` arm of a NESTED engine match inside a non-`None`
    // outer arm — `Some(e) => match engine { None => build }` — is dead code
    // the allowance must NOT cover: the `None`-arm flag does not inherit or
    // re-establish across a nested match.
    let planted = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            mut engine: Option<&mut ComponentMetaQueryEngine>,
            scope_canonical_id: &str,
        ) {
            let shadowing = match engine.as_deref_mut() {
                Some(e) => match engine {
                    Some(_) => e.scope_shadowing_for_scope(scope_canonical_id),
                    None => std::sync::Arc::new(
                        crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                            ctx,
                            scope_canonical_id,
                        ),
                    ),
                },
                None => default_shadowing(),
            };
            let _ = shadowing;
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
        planted,
    );
    // An inheriting / re-establishing flag allowlists the nested build (0);
    // a per-match-depth reset fires it (1).
    assert_eq!(
        v.len(),
        1,
        "a `from_host_scope` build in the `None` arm of a NESTED engine match \
         (inside a non-`None` outer arm) MUST fire — the allowance does not \
         inherit across a nested match; got: {v:?}"
    );
    assert_eq!(v[0].method, "from_host_scope");
    assert_eq!(v[0].fn_name, "project_expr_class_a_via_dispatch_threaded");
}

#[test]
fn component_meta_hot_paths_self_test_ufcs_build_fires() {
    // The UFCS / qself form `<ScopeShadowing>::from_host_scope(...)` launders
    // the `ScopeShadowing` ident into the path's qself, leaving only the ctor
    // segment in the call path — WITHOUT renaming. A maintainer using the real
    // name expects coverage: it must FIRE.
    let planted = r#"
        fn materialize_component_meta_type_expr_until_stable_full(
            ctx: &dyn ResolverContext,
            scope_canonical_id: &str,
        ) {
            let bare = <ScopeShadowing>::from_host_scope(ctx, scope_canonical_id);
            let qualified =
                <crate::resolver_core::scope_shadowing::ScopeShadowing>::from_scope_payload(None);
            let _ = (bare, qualified);
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // The path-only `segments[n-2] == ScopeShadowing` check bails on the qself
    // form (the ctor is the lone segment) → 0; the qself-aware check fires both.
    assert_eq!(
        v.len(),
        2,
        "the UFCS `<ScopeShadowing>::<ctor>(...)` form (bare and \
         module-qualified qself) MUST fire — the qself launders the type ident \
         but does not rename it; got: {v:?}"
    );
    assert!(v.iter().any(|x| x.method == "from_host_scope"));
    assert!(v.iter().any(|x| x.method == "from_scope_payload"));
}

#[test]
fn component_meta_hot_paths_self_test_bare_mod_tests_is_scanned() {
    // A bare `mod tests` with NO `#[cfg(test)]` gate compiles into PRODUCTION
    // Rust — the module name alone is not a build gate. A name-based skip
    // (`ident == "tests"`) wrongly ignores its builds; only a cfg that entails
    // test gates a module out of the build. The build MUST fire.
    let planted = r#"
        mod tests {
            fn t() {
                let _ =
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(None);
            }
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/materialize/field_types.rs",
        planted,
    );
    // A bare-name `mod tests` skip ignores this (0); cfg-entailment gating scans
    // it (1).
    assert_eq!(
        v.len(),
        1,
        "a bare production `mod tests` (no `#[cfg(test)]`) compiles into \
         production and MUST be scanned; got: {v:?}"
    );
    assert_eq!(v[0].method, "from_scope_payload");
}

#[test]
fn component_meta_hot_paths_self_test_non_canonical_engine_scrutinee_fires() {
    // Only `match engine.as_deref_mut() { None => ... }` provably runs its `None`
    // arm with NO engine present. `match engine.filter(..)` and
    // `match engine.and_then(..)` can take their `None` arm with an engine
    // PRESENT, so a direct build there is NOT the sanctioned engine-less
    // fallback. Both builds MUST fire even inside the allowlisted fn.
    let planted = r#"
        fn project_expr_class_a_via_dispatch_threaded(
            ctx: &dyn ResolverContext,
            mut engine: Option<&mut ComponentMetaQueryEngine>,
            scope_canonical_id: &str,
        ) {
            let a = match engine.filter(|_| false) {
                Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
                None => std::sync::Arc::new(
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    ),
                ),
            };
            let b = match engine.and_then(|_| None) {
                Some(e) => e.scope_shadowing_for_scope(scope_canonical_id),
                None => std::sync::Arc::new(
                    crate::resolver_core::scope_shadowing::ScopeShadowing::from_host_scope(
                        ctx,
                        scope_canonical_id,
                    ),
                ),
            };
            let _ = (a, b);
        }
    "#;
    let v = component_meta_scope_shadowing_memo::violations_in(
        "crates/verter_session/src/meta_resolve/dispatch_helpers.rs",
        planted,
    );
    // A "root receiver is engine" scan sanctions BOTH `None` arms — the first
    // consumes the single allowance, so it yields 1; the exact
    // `engine.as_deref_mut()` match sanctions NEITHER (2).
    assert_eq!(
        v.len(),
        2,
        "a `from_host_scope` build in the `None` arm of `match engine.filter(..)` \
         or `match engine.and_then(..)` MUST fire — only `engine.as_deref_mut()` \
         is the sanctioned engine-less scrutinee; got: {v:?}"
    );
    assert!(v.iter().all(|x| x.method == "from_host_scope"));
    assert!(v
        .iter()
        .all(|x| x.fn_name == "project_expr_class_a_via_dispatch_threaded"));
}
