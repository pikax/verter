use super::*;

#[test]
fn slot_binding_graph_emits_synthesis_spans() {
    let src = read_workspace_file("crates/verter_session/src/meta_resolve/slot_binding_graph.rs");
    assert!(
        src.contains("synthesize_slot_bindings"),
        "slot_binding_graph.rs must emit a `synthesize_slot_bindings` \
         tracing span at the synthesis entry point so log captures can \
         attribute work to the synthesis pass.",
    );
    assert!(
        src.contains("synthesize_macro"),
        "slot_binding_graph.rs must emit a per-macro `synthesize_macro` \
         tracing span so log captures can attribute work to a specific \
         macro invocation within the synthesis pass.",
    );
}

// ───────────────────────────────────────────────────────────────────────────
/// Architecture guard (CRITICAL: typeinfo spans-not-strings) — the typeinfo
/// `TypeInfoSurface` family carries NAMES, node ids, spans, origins, flags, and
/// JSDoc SPANS as authority — never RENDERED type strings or JSDoc TEXT.
///
/// The surface is the cache-owned, generation-stable projection a consumer
/// slices source from on demand (like Verter's Vue compiler / `CodeTransform`):
/// every span is a `(file, byte-range)` the consumer resolves against the
/// cache-owned `IndexedReady` source at the FFI / consumer boundary. Storing a
/// rendered `String` (a pre-sliced type display, a JSDoc description text) on
/// the surface would (a) bloat the host-owned cache with owned text, (b) couple
/// the surface to a display format, and (c) re-open the banned
/// synthesise-then-reparse direction (a consumer parsing the stored string).
///
/// This guard parses the typeinfo surface files — the core
/// `typeinfo/surface.rs` AND the relocated Vue-adapter surface
/// `typeinfo/framework_surface/vue_exec/mod.rs` (which carries the `.vue`-macro
/// `VueMacroSurface`) — and asserts NO surface struct (a name containing
/// `Surface` or starting with `TypeInfo`: the surface + member + signature +
/// index-signature + adapter-macro-surface types) has a `String` /
/// `Option<String>` / `Vec<String>` / `Box<str>` field. Names are `Arc<str>`
/// (interned), positions are `CanonicalSpan` / `Span`, types are
/// `SemanticNodeId`. A future `type_string: String` / `jsdoc_text: String`
/// field on EITHER file fails this gate — fix the producer (carry a span)
/// instead.
#[test]
fn typeinfo_surface_carries_spans_not_rendered_strings() {
    use syn::visit::Visit;

    // The surface authority spans both the core surface module and the
    // per-adapter surface modules; the spans-not-strings invariant must hold on
    // every one. Each entry pairs the file with the precondition struct that
    // proves the guard actually parsed the real surface there.
    const SURFACE_FILES: &[(&str, &str)] = &[
        (
            "crates/verter_session/src/typeinfo/surface.rs",
            "TypeInfoSurface",
        ),
        (
            "crates/verter_session/src/typeinfo/framework_surface/vue_exec/mod.rs",
            "VueMacroSurface",
        ),
    ];

    /// Is `ty` a rendered-text field type (the banned authority shape)?
    fn is_rendered_text_type(ty: &syn::Type) -> bool {
        // Match the LAST path segment's identifier + (for Option/Vec/Box) its
        // single generic argument.
        fn last_ident(ty: &syn::Type) -> Option<String> {
            match ty {
                syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
                _ => None,
            }
        }
        fn first_generic(ty: &syn::Type) -> Option<&syn::Type> {
            let syn::Type::Path(p) = ty else { return None };
            let seg = p.path.segments.last()?;
            let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
                return None;
            };
            args.args.iter().find_map(|a| match a {
                syn::GenericArgument::Type(t) => Some(t),
                _ => None,
            })
        }
        match last_ident(ty).as_deref() {
            // `String` and `Box<str>` are owned rendered text.
            Some("String") => true,
            Some("Box") => matches!(
                first_generic(ty).and_then(last_ident).as_deref(),
                Some("str")
            ),
            // `Option<String>` / `Vec<String>` — recurse into the element type.
            Some("Option") | Some("Vec") => first_generic(ty).is_some_and(is_rendered_text_type),
            _ => false,
        }
    }

    /// Is `name` a surface-authority struct (the types this invariant
    /// governs)? Matches the `TypeInfo*` family AND any `*Surface*` struct
    /// (e.g. the adapter `VueMacroSurface`, `SurfaceMemberOrigin`,
    /// `TypeInfoSurfaceMember`), so an adapter surface that carries owned text
    /// is caught regardless of its module prefix.
    fn is_surface_struct(name: &str) -> bool {
        name.starts_with("TypeInfo") || name.contains("Surface")
    }

    struct SurfaceStructVisitor {
        rel: &'static str,
        violations: Vec<String>,
    }
    impl<'ast> Visit<'ast> for SurfaceStructVisitor {
        fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
            let struct_name = node.ident.to_string();
            if is_surface_struct(&struct_name) {
                for field in &node.fields {
                    if is_rendered_text_type(&field.ty) {
                        let field_name = field
                            .ident
                            .as_ref()
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "<tuple>".to_string());
                        let rel = self.rel;
                        self.violations.push(format!(
                            "{rel}: {struct_name}.{field_name} is a rendered-text \
                             field — the typeinfo surface must carry a SPAN \
                             (`CanonicalSpan`) or interned name (`Arc<str>`), NOT a \
                             rendered type string / JSDoc text. Fix the producer to \
                             carry a span the consumer slices on demand.",
                        ));
                    }
                }
            }
            syn::visit::visit_item_struct(self, node);
        }
    }

    let mut violations: Vec<String> = Vec::new();
    for (rel, precondition_struct) in SURFACE_FILES {
        let src = read_workspace_file(rel);
        let parsed = syn::parse_file(&src)
            .unwrap_or_else(|e| panic!("spans-not-strings guard: `{rel}` failed to parse: {e}"));

        let mut visitor = SurfaceStructVisitor {
            rel,
            violations: Vec::new(),
        };
        visitor.visit_file(&parsed);
        violations.extend(visitor.violations);

        // Positive precondition: the guard actually SAW the surface struct (a
        // rename/move would silently pass otherwise).
        let saw_surface = parsed
            .items
            .iter()
            .any(|it| matches!(it, syn::Item::Struct(s) if s.ident == precondition_struct));
        assert!(
            saw_surface,
            "spans-not-strings guard: `{precondition_struct}` struct not found in \
             `{rel}` — did it move or get renamed? The guard must scan the real \
             surface type."
        );
    }

    assert!(
        violations.is_empty(),
        "typeinfo surface must carry spans/ids/names, not rendered strings; \
         found {} violation(s):\n{}",
        violations.len(),
        violations.join("\n"),
    );
}

/// Self-test for the structural-carrier-producer builder privacy classifiers:
/// the bare module-private `fn <builder>(` shape passes for EACH of the three
/// builders; ANY visibility modifier (`pub` / `pub(crate)` / `pub(in …)`)
/// reddens; a `pub use` re-export of any builder is reported.
#[test]
fn structural_carrier_producer_lowerer_privacy_classifier_discriminates() {
    // Exercise the classifier for EVERY pinned builder — same-module privacy
    // permits a second producer that names ANY of the three, so each must be
    // bare-private.
    for builder in STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS {
        // CORRECT — bare module-private fn (no visibility modifier).
        let ok = format!("fn {builder}(\n");
        assert!(
            structural_carrier_producer_builder_privacy_violation(&ok, builder).is_none(),
            "the bare module-private `fn {builder}(` shape must PASS"
        );
        // CORRECT — indented bare module-private fn (the real in-module shape).
        let ok_indented = format!("    fn {builder}(\n");
        assert!(
            structural_carrier_producer_builder_privacy_violation(&ok_indented, builder).is_none(),
            "an indented bare module-private `fn {builder}(` must PASS (the def line is trimmed \
             first)"
        );
        // WRONG — bare `pub` lets any module name the entry.
        let wrong_pub = format!("pub fn {builder}(\n");
        assert!(
            structural_carrier_producer_builder_privacy_violation(&wrong_pub, builder).is_some(),
            "a bare `pub fn {builder}(` shape must be reported as a single-engine violation"
        );
        // WRONG — `pub(crate)` is crate-wide visibility.
        let wrong_crate = format!("    pub(crate) fn {builder}(\n");
        assert!(
            structural_carrier_producer_builder_privacy_violation(&wrong_crate, builder).is_some(),
            "a `pub(crate) fn {builder}(` shape must be reported as a single-engine violation"
        );
        // WRONG — a `pub(in …)` owner-subtree path lets a sibling under the owner
        // module name the entry.
        let wrong_in = format!("    pub(in crate::structural_carrier_producer) fn {builder}(\n");
        assert!(
            structural_carrier_producer_builder_privacy_violation(&wrong_in, builder).is_some(),
            "even a `pub(in crate::structural_carrier_producer) fn {builder}(` shape must be \
             reported — a sibling surface could then name the builder"
        );
        // The re-export classifier: a `pub use … <builder>` is a violation; the
        // bare def line is not.
        let reexport =
            format!("pub use crate::structural_carrier_producer::macro_arg_producer::{builder};\n");
        assert!(
            structural_builder_reexport_violation(&reexport, builder),
            "a `pub use … {builder}` re-export must be reported"
        );
        assert!(
            !structural_builder_reexport_violation(&format!("fn {builder}() {{}}\n"), builder),
            "a bare definition (not a `pub use`) must NOT be flagged as a re-export"
        );
    }
    // The REAL owner source is the genuine module-private shape for all three
    // builders and re-exports none of them (revert proof: the production tree is
    // pristine).
    let real = std::fs::read_to_string(workspace_path(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    ))
    .expect("the producer's owner module must be readable");
    for builder in STRUCTURAL_CARRIER_PRODUCER_PRIVATE_BUILDERS {
        assert!(
            structural_carrier_producer_builder_privacy_violation(&real, builder).is_none(),
            "the REAL `{builder}` in `macro_arg_producer.rs` MUST be a bare module-private fn — if \
             this reddens, the builder's visibility was widened, re-opening the producer surface"
        );
        assert!(
            !structural_builder_reexport_violation(&real, builder),
            "the REAL `macro_arg_producer.rs` must NOT re-export `{builder}`"
        );
    }
}

/// Self-test for [`macro_arg_producer_expansion_surface_violations`] +
/// [`macro_arg_producer_derive_shadow_import_violations`]: the genuine producer
/// source passes; a planted production bang-macro (ANY, incl. `matches!` /
/// `format!`) / `macro_rules!` / `include!` / qualified-or-custom `#[derive]` /
/// `#[macro_use]` / out-of-line `#[path]` mod reddens; a built-in single-segment
/// `#[derive]` is allowed; a derive-shadow import / glob reddens; the sanctioned
/// `#[cfg(test)] #[path] mod *_tests;` wiring and a `#[cfg(test)]`-gated macro do
/// NOT.
#[test]
fn macro_arg_producer_expansion_surface_classifier_discriminates() {
    // ACCEPT — the genuine producer source: explicit `match` (no `matches!`), the
    // three sanctioned `#[cfg(test)] #[path] mod *_tests;` test children, no
    // derive / macro_use / out-of-line production mod / bang macro.
    let real = std::fs::read_to_string(workspace_path(
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
    ))
    .expect("the producer module must be readable");
    assert!(
        macro_arg_producer_expansion_surface_violations(&real).is_empty(),
        "ACCEPT: the REAL `macro_arg_producer.rs` must have NO production expansion surface — if \
         this reddens, a production bang macro / derive / macro_use / out-of-line mod was added"
    );
    assert!(
        macro_arg_producer_derive_shadow_import_violations(&real).is_empty(),
        "ACCEPT: the REAL `macro_arg_producer.rs` must have NO derive-shadow import"
    );
    // The sanctioned `#[cfg(test)] #[path] mod *_tests;` wiring alone is allowed.
    let sanctioned =
        "#[cfg(test)]\n#[path = \"structural_lower_tests.rs\"]\nmod structural_lower_tests;\n";
    assert!(
        macro_arg_producer_expansion_surface_violations(sanctioned).is_empty(),
        "the sanctioned `#[cfg(test)] #[path] mod structural_lower_tests;` wiring must be allowed"
    );
    // A `#[cfg(test)]`-gated code-gen macro is NOT a production surface (test item
    // is skipped, not descended).
    let cfg_test_macro = "#[cfg(test)]\nmod t {\n    fn f() { include!(\"x.rs\"); }\n}\n";
    assert!(
        macro_arg_producer_expansion_surface_violations(cfg_test_macro).is_empty(),
        "a `#[cfg(test)]`-gated code-gen macro must NOT be a production violation (the cfg-test \
         item is skipped, not descended)"
    );
    // REJECT (FIX C ban-all) — ANY production bang macro now reddens, including an
    // ordinary std declarative macro (`matches!` / `format!`). A function-like
    // macro defined elsewhere and invoked here is invisible to `syn`, so a
    // denylist is incomplete; the `matches!`→`match` de-sugar keeps the real file
    // bang-macro-free.
    let std_macro = "fn build() { let _ = matches!(x, A | B); }\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(std_macro).is_empty(),
        "a production `matches!` bang macro must NOW redden (FIX C bans ALL bang macros; the real \
         file's `matches!` is de-sugared)"
    );
    let format_macro = "fn build() { let _ = format!(\"{}\", y); }\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(format_macro).is_empty(),
        "a production `format!` bang macro must redden (ban-all)"
    );
    // ACCEPT — a built-in SINGLE-SEGMENT `#[derive(…)]` on the producer's data
    // types (the genuine file carries `#[derive(Debug, Default, Clone, Copy, …)]`).
    let builtin_derive =
        "#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]\nstruct Ctx { _x: () }\n";
    assert!(
        macro_arg_producer_expansion_surface_violations(builtin_derive).is_empty(),
        "a built-in single-segment `#[derive(…)]` on a producer data type must NOT redden (only a \
         custom / qualified derive is a code-gen surface)"
    );
    // REJECT (FIX E) — a QUALIFIED `#[derive(evil::Debug)]` whose final segment is
    // a built-in NAME but resolves to a FOREIGN macro.
    let qualified_derive = "#[derive(evil::Debug)]\nstruct Producer { _x: () }\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(qualified_derive).is_empty(),
        "a QUALIFIED `#[derive(evil::Debug)]` must redden — a derive path must be a single-segment \
         compiler built-in (FIX E)"
    );
    // REJECT — a production `macro_rules!` item (defines a new macro here).
    let macro_rules = "macro_rules! synth { () => {}; }\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(macro_rules).is_empty(),
        "a production `macro_rules!` item must redden"
    );
    // REJECT — a production `include!` file splice (now caught by the ban-all bang
    // rule).
    let include = "include!(\"generated.rs\");\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(include).is_empty(),
        "a production `include!` file splice must redden"
    );
    // REJECT — a production CUSTOM (non-builtin) `#[derive(…)]` (a derive
    // proc-macro) on a producer-capable type.
    let derive = "#[derive(serde::Serialize)]\nstruct Producer { _x: () }\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(derive).is_empty(),
        "a production CUSTOM `#[derive(serde::Serialize)]` (a derive proc-macro) must redden"
    );
    // REJECT — a production `#[macro_use]` attribute.
    let macro_use = "#[macro_use]\nextern crate serde;\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(macro_use).is_empty(),
        "a production `#[macro_use]` attribute must redden"
    );
    // REJECT — a NON-sanctioned out-of-line `#[path]` child mod (not test wiring).
    let path_mod = "#[path = \"second_producer.rs\"]\nmod second_producer;\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(path_mod).is_empty(),
        "a non-sanctioned production out-of-line `#[path]` child mod must redden"
    );
    // REJECT — an out-of-line `mod *_tests;` WITHOUT the `#[cfg(test)]` gate is a
    // production splice (the gate is mandatory for the sanctioned wiring).
    let ungated_tests = "#[path = \"structural_lower_tests.rs\"]\nmod structural_lower_tests;\n";
    assert!(
        !macro_arg_producer_expansion_surface_violations(ungated_tests).is_empty(),
        "an out-of-line `mod *_tests;` WITHOUT `#[cfg(test)]` is a production splice and must redden"
    );

    // ── FIX E — derive-shadow import rejection ─────────────────────────────
    // REJECT — `use evil::Clone;` shadows the built-in derive `Clone`.
    let shadow_plain = "use evil::Clone;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(shadow_plain).is_empty(),
        "a `use evil::Clone;` import shadowing the built-in `Clone` derive must redden"
    );
    // REJECT — a raw-ident `use evil::r#Clone;` binds the SAME name `Clone`.
    let shadow_raw = "use evil::r#Clone;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(shadow_raw).is_empty(),
        "a raw-ident `use evil::r#Clone;` import must redden (it binds `Clone`, unrawed)"
    );
    // REJECT — an alias `use evil::Serialize as Clone;` binds `Clone`.
    let shadow_alias = "use evil::Serialize as Clone;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(shadow_alias).is_empty(),
        "a `use evil::Serialize as Clone;` alias binding `Clone` must redden"
    );
    // REJECT — a glob `use evil::*;` could bring any built-in-derive name in.
    let shadow_glob = "use evil::*;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(shadow_glob).is_empty(),
        "a glob `use evil::*;` import must redden (it could shadow a built-in derive)"
    );
    // REJECT — a `#[macro_use]` attribute (foreign derive-macro prelude injection).
    let shadow_macro_use = "#[macro_use]\nextern crate evil;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(shadow_macro_use).is_empty(),
        "a `#[macro_use] extern crate evil;` must redden (derive-shadow via prelude injection)"
    );
    // ACCEPT — the genuine producer imports bind NO built-in-derive name and use no
    // glob.
    let real_imports = "use std::cell::Cell;\nuse std::sync::{Arc, OnceLock};\nuse rustc_hash::FxHashMap;\nuse crate::resolver_core::ResolverContext;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(real_imports).is_empty(),
        "the genuine producer imports (no built-in-derive name, no glob) must NOT redden"
    );
    // A `#[cfg(test)]`-gated shadow import is test-only → not a production
    // derive-shadow.
    let test_shadow = "#[cfg(test)]\nuse evil::Clone;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(test_shadow).is_empty(),
        "a `#[cfg(test)]`-gated shadow import must NOT redden (test-only)"
    );

    // ── crate-root-rebind rejection (companion to the full-path trait allowlist)
    // A `use … as std;` rebinds the crate-root name `std` the qualified
    // `std::clone::Clone` allowlist spelling depends on → a foreign trait could be
    // spelled as an allowlisted path. REJECT.
    let rebind_std_use = "use evil as std;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(rebind_std_use).is_empty(),
        "a `use evil as std;` rebinding the crate-root name `std` must redden (crate-root-rebind — \
         it would let a foreign trait be spelled as an allowlisted `std::clone::Clone`)"
    );
    // A `use … as core;` rebinds `core`. REJECT.
    let rebind_core_use = "use evil as core;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(rebind_core_use).is_empty(),
        "a `use evil as core;` rebinding the crate-root name `core` must redden (crate-root-rebind)"
    );
    // A `use crate::semantic_query as std;` (a real-shaped in-crate module rebound to
    // `std`) likewise binds the LOCAL name `std`. REJECT — this is the exact live
    // plant shape.
    let rebind_intra_crate = "use crate::semantic_query as std;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(rebind_intra_crate).is_empty(),
        "a `use crate::semantic_query as std;` rebinding the local name `std` to an in-crate \
         module must redden (crate-root-rebind)"
    );
    // An `extern crate evil as std;` renames the crate root to `std`. REJECT.
    let rebind_extern_std = "extern crate evil as std;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(rebind_extern_std).is_empty(),
        "an `extern crate evil as std;` rename to `std` must redden (crate-root-rebind)"
    );
    // An `extern crate evil as core;` renames the crate root to `core`. REJECT.
    let rebind_extern_core = "extern crate evil as core;\n";
    assert!(
        !macro_arg_producer_derive_shadow_import_violations(rebind_extern_core).is_empty(),
        "an `extern crate evil as core;` rename to `core` must redden (crate-root-rebind)"
    );
    // CONTROL (GREEN) — a normal `use std::sync::Arc;` binds the LEAF `Arc`, NOT
    // `std`; the real file uses exactly this `std::…` shape. Must NOT redden.
    let control_std_leaf = "use std::sync::Arc;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(control_std_leaf).is_empty(),
        "a normal `use std::sync::Arc;` binds the leaf `Arc`, not `std`, and must NOT redden (the \
         real producer imports use this shape)"
    );
    // CONTROL (GREEN) — `use core::cell::Cell;` binds `Cell`, not `core`.
    let control_core_leaf = "use core::cell::Cell;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(control_core_leaf).is_empty(),
        "a normal `use core::cell::Cell;` binds the leaf `Cell`, not `core`, and must NOT redden"
    );
    // CONTROL (GREEN) — a grouped `use std::sync::{Arc, OnceLock};` (the real file's
    // exact import) binds `Arc` and `OnceLock`, NOT `std`. Must NOT redden.
    let control_std_group = "use std::sync::{Arc, OnceLock};\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(control_std_group).is_empty(),
        "a grouped `use std::sync::{{Arc, OnceLock}};` (the real producer import) binds the leaves, \
         not `std`, and must NOT redden"
    );
    // CONTROL (GREEN) — a bare `extern crate proc_macro;` with NO rename to std/core
    // binds `proc_macro`, not `std`/`core`. Must NOT redden.
    let control_extern_bare = "extern crate proc_macro;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(control_extern_bare).is_empty(),
        "a bare `extern crate proc_macro;` (no rename to std/core) must NOT redden"
    );
    // A `#[cfg(test)]`-gated crate-root rebind is test-only → not a production
    // rebind. Must NOT redden.
    let test_rebind = "#[cfg(test)]\nuse evil as std;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(test_rebind).is_empty(),
        "a `#[cfg(test)]`-gated `use evil as std;` must NOT redden (test-only)"
    );
    let test_rebind_extern = "#[cfg(test)]\nextern crate evil as std;\n";
    assert!(
        macro_arg_producer_derive_shadow_import_violations(test_rebind_extern).is_empty(),
        "a `#[cfg(test)]`-gated `extern crate evil as std;` must NOT redden (test-only)"
    );
}

/// Self-test for the owner-module narrowness classifier: the sanctioned member
/// set passes; a planted extra non-test module name is reported; a `*_tests.rs`
/// module is allowed.
#[test]
fn structural_carrier_producer_narrowness_classifier_discriminates() {
    // Reuse the SAME sanctioned-set membership predicate the live guard uses by
    // re-stating it here over an in-memory file list, so a divergence in the
    // allowlist reddens.
    const SANCTIONED: &[&str] = &["mod.rs", "macro_arg_producer.rs", "infer_binder_names.rs"];
    let classify = |names: &[&str]| -> Vec<String> {
        names
            .iter()
            .filter(|n| !n.ends_with("_tests.rs"))
            .filter(|n| !SANCTIONED.contains(*n))
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
    };
    // The genuine member set (production + tests) has NO extras.
    let genuine = [
        "mod.rs",
        "macro_arg_producer.rs",
        "infer_binder_names.rs",
        "structural_lower_tests.rs",
        "macro_hot_mirror_tests.rs",
        "script_setup_binder_tests.rs",
    ];
    assert!(
        classify(&genuine).is_empty(),
        "the genuine owner member set (sanctioned production + `*_tests.rs`) must have NO extras"
    );
    // A planted extra non-test module IS reported.
    let with_extra = [
        "mod.rs",
        "macro_arg_producer.rs",
        "infer_binder_names.rs",
        "second_producer.rs",
    ];
    assert_eq!(
        classify(&with_extra),
        vec!["second_producer.rs".to_string()],
        "a planted non-test, non-sanctioned module in the owner directory must be reported"
    );
    // A `*_tests.rs` module is NOT reported (test modules are always allowed).
    let only_test = [
        "mod.rs",
        "macro_arg_producer.rs",
        "infer_binder_names.rs",
        "extra_invariant_tests.rs",
    ];
    assert!(
        classify(&only_test).is_empty(),
        "a `*_tests.rs` module must be allowed and not reported as an extra"
    );
}

#[test]
fn mirror_entry_surface_classifier_discriminates() {
    // Baseline: one sanctioned free entry + a restricted-subtree helper + an
    // impl-block method/associated-fn (BOTH module-private — `new` has inherited
    // visibility, `demanded_count` is `#[cfg(test)]`) → only the sanctioned entry
    // is reported. The restricted-subtree helper (`pub(in …)`) is not
    // crate-visible. (A synthetic `pub(in …)` example name is used here, NOT the
    // structural lowerer — the lowerer's real shape is bare module-private, not
    // `pub(in …)`.)
    let ok = "pub(crate) fn macro_type_arg_hot_ref(ctx: &C) -> Option<H> { None }\n\
              pub(in crate::structural_carrier_producer) fn restricted_subtree_helper(g: &G) -> R { todo!() }\n\
              impl Ctx {\n    fn new(b: &[B]) -> Self { Self }\n    #[cfg(test)]\n    pub(crate) fn demanded_count(&self) -> usize { 0 }\n}\n";
    assert_eq!(
        crate_visible_producer_fn_names(ok),
        vec!["macro_type_arg_hot_ref".to_string()],
        "only the `pub(crate)` free fn is a crate-visible entry; the restricted-subtree helper, the \
         module-private `impl`-block `new`, and the `#[cfg(test)]` `demanded_count` are not"
    );
    let widened_attachment = "impl MacroHotMirror { pub(crate) fn attach(&self) -> MacroMirrorAttachment { build_macro_hot_ref(); MacroMirrorAttachment { cells: Arc::clone(&self.cells) } } }";
    assert_eq!(
        crate_visible_producer_fn_names(widened_attachment),
        ["attach"],
        "a crate-visible `attach` is always an entry; whether it carries producer work is owned by \
         the capability guards over `macro_arg_producer.rs`, not by a body-shape classifier"
    );
    let wrong_field = "impl MacroHotMirror { pub(crate) fn attach(&self) -> MacroMirrorAttachment { MacroMirrorAttachment { cells: Arc::clone(&self.other_cells) } } }";
    assert_eq!(
        crate_visible_producer_fn_names(wrong_field),
        ["attach"],
        "a differing attachment body is still the same single crate-visible entry"
    );
    let production_cfg = "#[cfg(any(test, debug_assertions))] impl MacroHotMirror { pub(crate) fn attach(&self) -> MacroMirrorAttachment { build_macro_hot_ref(); MacroMirrorAttachment { cells: Arc::clone(&self.cells) } } }";
    assert_eq!(
        crate_visible_producer_fn_names(production_cfg),
        ["attach"],
        "production-satisfiable cfg cannot hide producer work"
    );
    let foreign_attachment = "impl Other { pub(crate) fn attach(&self) -> MacroMirrorAttachment { MacroMirrorAttachment { cells: Arc::clone(&self.cells) } } }";
    assert_eq!(
        crate_visible_producer_fn_names(foreign_attachment),
        ["attach"],
        "same method name on another owner is not exempt"
    );
    // INJECT a second module-level `pub(crate)` FREE producer → now TWO entries.
    let two = "pub(crate) fn macro_type_arg_hot_ref(ctx: &C) -> Option<H> { None }\n\
               pub(crate) fn macro_type_arg_hot_ref_v2(ctx: &C) -> Option<H> { None }\n";
    let names = crate_visible_producer_fn_names(two);
    assert!(
        names.contains(&"macro_type_arg_hot_ref".to_string())
            && names.contains(&"macro_type_arg_hot_ref_v2".to_string()),
        "an injected second module-level `pub(crate) fn` producer must be reported as an extra \
         crate-visible entry (reddening the guard)"
    );
    // ROUND-4 GOV P0 — the heart of this fix. INJECT a second crate-visible
    // ASSOCIATED fn inside an `impl` block (the exact gap the old line-scanner
    // missed: it only saw module-level free fns at brace-depth 0). It MUST be
    // reported as an extra crate-visible producer entry, reddening the guard.
    let assoc_second = "pub(crate) fn macro_type_arg_hot_ref(c: &C) -> Option<H> { None }\n\
                        impl MacroHotMirror {\n    pub(crate) fn second_entry(c: &C) -> Option<H> { None }\n}\n";
    let assoc_names = crate_visible_producer_fn_names(assoc_second);
    assert!(
        assoc_names.contains(&"macro_type_arg_hot_ref".to_string())
            && assoc_names.contains(&"second_entry".to_string()),
        "an injected `impl Foo {{ pub(crate) fn second_entry … }}` associated producer MUST be \
         reported as a crate-visible entry — the guard now sees `impl`-block associated fns, not \
         only module-level free fns (got {assoc_names:?})"
    );
    // ROUND-5 GOV — `pub(in crate)` is EXACTLY equivalent crate-wide visibility,
    // so an injected `pub(in crate)` producer MUST redden, FREE or ASSOCIATED.
    // FREE: a second `pub(in crate) fn` is a second outward entry.
    let pub_in_crate_free = "pub(crate) fn macro_type_arg_hot_ref(c: &C) -> Option<H> { None }\n\
         pub(in crate) fn second_entry(c: &C) -> Option<H> { None }\n";
    let pic_free = crate_visible_producer_fn_names(pub_in_crate_free);
    assert!(
        pic_free.contains(&"macro_type_arg_hot_ref".to_string())
            && pic_free.contains(&"second_entry".to_string()),
        "an injected `pub(in crate) fn` FREE producer must be reported as a crate-visible entry — \
         `pub(in crate)` is identical crate-wide visibility to `pub(crate)` (got {pic_free:?})"
    );
    // ASSOCIATED: a `pub(in crate) fn` inside an INHERENT `impl` is equally a
    // second outward entry.
    let pub_in_crate_assoc =
        "pub(crate) fn macro_type_arg_hot_ref(c: &C) -> Option<H> { None }\n\
         impl MacroHotMirror {\n    pub(in crate) fn second_entry(c: &C) -> Option<H> { None }\n}\n";
    let pic_assoc = crate_visible_producer_fn_names(pub_in_crate_assoc);
    assert!(
        pic_assoc.contains(&"macro_type_arg_hot_ref".to_string())
            && pic_assoc.contains(&"second_entry".to_string()),
        "an injected `impl MacroHotMirror {{ pub(in crate) fn second_entry … }}` associated \
         producer must be reported as a crate-visible entry (got {pic_assoc:?})"
    );
    // A TRAIT-impl method is NOT a producer-entry vector and is SKIPPED: a
    // trait-impl method inherits the trait's visibility and cannot carry its own
    // `pub` / `pub(crate)` (the spelling below is rejected by rustc; `syn` still
    // parses it). The collector ignores trait-impl methods entirely, so it
    // reports NOTHING here and never errors.
    let trait_assoc =
        "impl SomeTrait for MacroHotMirror {\n    pub(crate) fn via_trait(&self) {}\n}\n";
    assert!(
        crate_visible_producer_fn_names(trait_assoc).is_empty(),
        "a trait-impl method is not an independently crate-visible producer entry — the collector \
         must skip trait impls (and not panic on the syn-parsed body)"
    );
    // A bare-trait-impl method WITHOUT a `pub` (the only valid Rust form) is also
    // skipped — confirming the trait-impl branch is taken on the realistic shape.
    let trait_assoc_valid = "impl SomeTrait for MacroHotMirror {\n    fn via_trait(&self) {}\n}\n";
    assert!(
        crate_visible_producer_fn_names(trait_assoc_valid).is_empty(),
        "a (valid) trait-impl method is not a crate-visible producer entry — the collector skips \
         trait impls"
    );
    // A `pub(in crate::structural_carrier_producer)` ASSOCIATED fn is module-private
    // (restricted BELOW crate scope) → NOT crate-visible → guard stays GREEN.
    let restricted_assoc =
        "impl MacroHotMirror {\n    pub(in crate::structural_carrier_producer) fn inner(&self) {}\n}\n";
    assert!(
        crate_visible_producer_fn_names(restricted_assoc).is_empty(),
        "a `pub(in crate::structural_carrier_producer)` associated fn is restricted (not crate-visible) and \
         must stay green"
    );
    // A `pub(in crate::structural_carrier_producer)` FREE fn is likewise module-private →
    // NOT crate-visible → stays GREEN (distinct from `pub(in crate)`).
    assert!(
        crate_visible_producer_fn_names(
            "pub(in crate::structural_carrier_producer) fn inner(g: &G) -> R { todo!() }\n"
        )
        .is_empty(),
        "a `pub(in crate::structural_carrier_producer)` FREE fn is restricted below crate scope and must stay \
         green — only an EXACT `crate` restricted path is crate-visible"
    );
    // A bare `pub fn` module-level entry is also crate-visible (wider).
    assert_eq!(
        crate_visible_producer_fn_names("pub fn another_producer(x: u8) -> u8 { x }\n"),
        vec!["another_producer".to_string()],
        "a bare `pub fn` module-level entry is crate-visible (in fact wider)"
    );
    // GOV P0 #1 (carried forward): the classifier MUST see through fn modifiers.
    // A second crate-visible producer hiding behind `unsafe` / `async` / `const`
    // / `extern "C"` is STILL a crate-visible producer entry. `syn` carries the
    // modifiers structurally on `sig`, so the fn name is recognised regardless.
    let unsafe_second = "pub(crate) fn macro_type_arg_hot_ref(c: &C) -> Option<H> { None }\n\
                         pub(crate) unsafe fn second_producer(c: &C) -> Option<H> { None }\n";
    let unsafe_names = crate_visible_producer_fn_names(unsafe_second);
    assert!(
        unsafe_names.contains(&"macro_type_arg_hot_ref".to_string())
            && unsafe_names.contains(&"second_producer".to_string()),
        "an injected `pub(crate) unsafe fn` second producer must be reported as a crate-visible \
         entry — the classifier sees through the `unsafe` modifier (got {unsafe_names:?})"
    );
    assert_eq!(
        crate_visible_producer_fn_names("pub(crate) async fn async_producer() {}\n"),
        vec!["async_producer".to_string()],
        "a `pub(crate) async fn` module-level entry is crate-visible (modifier-aware)"
    );
    assert_eq!(
        crate_visible_producer_fn_names("pub(crate) const fn const_producer() -> u8 { 0 }\n"),
        vec!["const_producer".to_string()],
        "a `pub(crate) const fn` module-level entry is crate-visible (modifier-aware)"
    );
    assert_eq!(
        crate_visible_producer_fn_names("pub(crate) unsafe extern \"C\" fn extern_producer() {}\n"),
        vec!["extern_producer".to_string()],
        "a `pub(crate) unsafe extern \"C\" fn` module-level entry is crate-visible"
    );
    // A `pub(in …)` restricted FREE entry is NOT crate-visible even WITH a
    // modifier.
    assert!(
        crate_visible_producer_fn_names(
            "pub(in crate::structural_carrier_producer) unsafe fn restricted() {}\n"
        )
        .is_empty(),
        "a `pub(in …) unsafe fn` restricted-visibility entry is NOT crate-visible"
    );
    // A `#[cfg(test)]`-gated module's `pub fn` is test-only and excluded — it
    // does NOT widen the production producer surface.
    let facade = "#[cfg(test)]\nmod tests {\n    pub fn structural_lower_probe() {}\n}\n";
    assert!(
        crate_visible_producer_fn_names(facade).is_empty(),
        "a `#[cfg(test)]`-gated module's `pub fn` must be excluded (it does not widen the \
         production producer surface)"
    );
    // STRENGTHENING (F1): a bare `#[cfg(debug_assertions)] pub(crate) fn` is
    // COMPILED in debug production builds, so it MUST be counted as a production
    // producer entry — `debug_assertions` is NOT a test gate. Single-entry must
    // hold in debug builds too, so a debug-only crate-visible producer reddens.
    assert_eq!(
        crate_visible_producer_fn_names("#[cfg(debug_assertions)]\npub(crate) fn debug_only() {}\n"),
        vec!["debug_only".to_string()],
        "a `#[cfg(debug_assertions)] pub(crate) fn` is COMPILED in debug production builds and MUST \
         be counted — `debug_assertions` is NOT a test gate (single-entry must hold in debug too)"
    );
    // [P0] SATISFIABILITY CLOSURE — the heart of FIX B. A
    // `#[cfg(any(test, debug_assertions))] pub(crate) fn` is PRODUCTION-
    // SATISFIABLE (a debug non-test build satisfies it via `debug_assertions`),
    // so it COMPILES into the always-on debug production lib and MUST be COUNTED.
    // The entailment classifier: `any(...)` entails test ONLY IF ALL operands
    // entail test — here `debug_assertions` does not, so the predicate does NOT
    // entail test and the item is COUNTED (the rogue-producer evasion the prior
    // over-exclusion permitted is now closed).
    assert_eq!(
        crate_visible_producer_fn_names(
            "#[cfg(any(test, debug_assertions))]\npub(crate) fn t() {}\n"
        ),
        vec!["t".to_string()],
        "a `#[cfg(any(test, debug_assertions))] pub(crate) fn` is PRODUCTION-satisfiable (via \
         `debug_assertions` in a debug non-test build) and MUST be COUNTED — `any(...)` entails \
         test ONLY when ALL operands do (the [P0] satisfiability closure)"
    );
    // ROUND-5 GOV — `#[cfg(not(test))]` is a PRODUCTION-only item (compiled in
    // non-test builds), NOT a test gate. It MUST be COUNTED: a `pub(crate)` fn
    // gated `#[cfg(not(test))]` is a genuine crate-visible producer entry. The
    // previous `rendered.contains("test")` gate FALSELY excluded it.
    assert_eq!(
        crate_visible_producer_fn_names("#[cfg(not(test))]\npub(crate) fn prod_only() {}\n"),
        vec!["prod_only".to_string()],
        "a `#[cfg(not(test))] pub(crate) fn` is a PRODUCTION item and MUST be counted — \
         `not(test)` is not a test gate"
    );
    // Conversely a bare `#[cfg(test)]` `pub(crate) fn` is excluded (test-only).
    assert!(
        crate_visible_producer_fn_names("#[cfg(test)]\npub(crate) fn test_only() {}\n").is_empty(),
        "a `#[cfg(test)] pub(crate) fn` is test-only and must be excluded"
    );
    // `#[cfg(any(feature = "x", test))]` is PRODUCTION-SATISFIABLE (feature `x`
    // ON without test), so it MUST be COUNTED — `any(...)` entails test ONLY when
    // ALL operands do, and `feature = "x"` does not. (Under the prior `test`-in-
    // any-arm classifier this was wrongly excluded.)
    assert_eq!(
        crate_visible_producer_fn_names(
            "#[cfg(any(feature = \"x\", test))]\npub(crate) fn maybe_test() {}\n"
        ),
        vec!["maybe_test".to_string()],
        "a `#[cfg(any(feature = \"x\", test))] pub(crate) fn` is PRODUCTION-satisfiable (feature \
         `x` on without test) and MUST be COUNTED — `any(...)` entails test only when ALL operands \
         do"
    );
    // `#[cfg(all(test, debug_assertions))]` is TEST-ONLY (the conjunction REQUIRES
    // test in every satisfying config), so it stays EXCLUDED — `all(...)` entails
    // test iff ANY operand does, and `test` does.
    assert!(
        crate_visible_producer_fn_names(
            "#[cfg(all(test, debug_assertions))]\npub(crate) fn t2() {}\n"
        )
        .is_empty(),
        "a `#[cfg(all(test, debug_assertions))] pub(crate) fn` is test-only (the conjunction \
         requires test) and must be EXCLUDED"
    );
    // `#[cfg(all(unix, not(test)))]` — production (the only test mention is under
    // `not`), so it MUST be counted.
    assert_eq!(
        crate_visible_producer_fn_names(
            "#[cfg(all(unix, not(test)))]\npub(crate) fn unix_prod() {}\n"
        ),
        vec!["unix_prod".to_string()],
        "a `#[cfg(all(unix, not(test)))] pub(crate) fn` is production (test only under `not`) and \
         MUST be counted"
    );
    // `cfg_attr` IS NOT AN ITEM GATE. A `#[cfg_attr(test, allow(dead_code))]
    // pub(crate) fn` is compiled into EVERY build (the `cfg_attr` only
    // CONDITIONALLY APPLIES `allow(dead_code)` under `test`; it never removes the
    // item), so it is a real PRODUCTION producer entry and MUST be COUNTED. The
    // prior classifier (treating `cfg_attr` like `cfg` and entailing test on its
    // condition) wrongly EXCLUDED it — a single-entry evasion across all three
    // collectors. This is the heart of the cfg_attr fix.
    assert_eq!(
        crate_visible_producer_fn_names(
            "#[cfg_attr(test, allow(dead_code))]\npub(crate) fn rogue() {}\n"
        ),
        vec!["rogue".to_string()],
        "a `#[cfg_attr(test, allow(dead_code))] pub(crate) fn` is compiled in EVERY build (cfg_attr \
         only conditionally APPLIES an attribute, it never gates the ITEM out) and MUST be COUNTED \
         — cfg_attr is not an item gate"
    );
    // A `cfg_attr` whose condition is UNRELATED to test is likewise not an item
    // gate — the item is counted.
    assert_eq!(
        crate_visible_producer_fn_names(
            "#[cfg_attr(feature = \"x\", inline)]\npub(crate) fn cfgattr_feature() {}\n"
        ),
        vec!["cfgattr_feature".to_string()],
        "a `#[cfg_attr(feature = \"x\", inline)] pub(crate) fn` is compiled in every build and MUST \
         be counted — cfg_attr never gates the item out"
    );
    // The strict `#[cfg(test)]` item gate excludes a genuine test-only entry.
    assert!(
        crate_visible_producer_fn_names("#[cfg(test)]\npub(crate) fn test_wired() {}\n").is_empty(),
        "a genuine `#[cfg(test)] pub(crate) fn` stays EXCLUDED after the cfg_attr split — the \
         `cfg(test)` item gate is unaffected"
    );
    // The real owner source exposes exactly the sanctioned crate-visible
    // entries, with free + associated coverage across every production file.
    let mut real: Vec<String> = Vec::new();
    for rel in [
        "crates/verter_type_engine/src/structural_carrier_producer/mod.rs",
        "crates/verter_type_engine/src/structural_carrier_producer/macro_arg_producer.rs",
        "crates/verter_type_engine/src/structural_carrier_producer/infer_binder_names.rs",
    ] {
        let src = std::fs::read_to_string(workspace_path(rel))
            .unwrap_or_else(|e| panic!("could not read {rel}: {e}"));
        real.extend(crate_visible_producer_fn_names(&src));
    }
    real.sort();
    let mut expected: Vec<String> = MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES
        .iter()
        .map(|s| s.to_string())
        .collect();
    expected.sort();
    assert_eq!(
        real, expected,
        "the REAL owner module must expose EXACTLY the sanctioned crate-visible entries \
         {MIRROR_SANCTIONED_CRATE_VISIBLE_ENTRIES:?} as its crate-visible fns (free OR \
         associated); found {real:?}"
    );
}

/// K1: the host's live carrier-grammar authority is composed from the
/// capability catalog the compiler's immutable frontend registrations
/// produce — never from a per-framework row list spelled in the host.
///
/// The `[(FileLanguage::vue(), 1, 1), (FileLanguage::svelte(), 1, 1)]`
/// matrix the host used to carry enumerated frameworks twice: once here,
/// once in the adapter registry. A framework added to the compiler
/// catalog registered itself from one matrix but was silently absent
/// from the other, so the host's grammar authority and its dispatch
/// authority could disagree. Composition makes the catalog the only
/// enumeration; `HostServices` fails closed when the two disagree.
#[test]
fn host_carrier_grammar_registration_is_catalog_composed() {
    let src = read_workspace_file("crates/verter_session/src/host_construction.rs");
    let start = src
        .find("let carrier_grammar_authority")
        .expect("host construction still builds a carrier-grammar authority");
    let end = src
        .find("let carrier_publication_store")
        .expect("the carrier-grammar authority is still published into a publication store");
    let region = &src[start..end];
    for framework in ["FileLanguage::vue()", "FileLanguage::svelte()"] {
        assert!(
            !region.contains(framework),
            "host construction registers carrier grammars from the composed capability \
             catalog; a `{framework}` row literal in the registration region is the \
             per-framework branch matrix:\n{region}"
        );
    }
    assert!(
        region.contains("HostServices::built_in()")
            || region.contains("HostServices::composed(")
            || region.contains("FrameworkCapabilityCatalog::built_in()"),
        "the host must seed its grammar authority from the composed framework services:\n{region}"
    );
    assert!(
        region.contains("register_all("),
        "the host registers carrier grammars through the catalog, never row by row:\n{region}"
    );
}

/// K1: the LSP capability advertisement is built from the host's own
/// composed classification authority.
///
/// `LanguageRegistry::global()` is a process-wide, install-once catalog
/// authority: any consumer can read it whether or not the host it serves
/// composed that generation, so a watcher's surface and the host's own
/// surface can disagree. Reading the host's classifier keeps the
/// advertised framework surface inside the host's construction.
#[test]
fn lsp_capabilities_read_the_host_composed_classifier() {
    let src = read_workspace_file("crates/verter_lsp/src/capabilities.rs");
    for needle in ["LanguageRegistry::global()", "LanguageRegistry::built_in()"] {
        assert!(
            !src.contains(needle),
            "the LSP capability surface must read classification from the host it \
             serves, never from a process-global registry; `{needle}` reintroduces a \
             second, install-once catalog authority"
        );
    }
    assert!(
        src.contains("HostLanguageClassifier"),
        "the LSP capability surface must take the host's composed classifier:\n{src}"
    );
}
