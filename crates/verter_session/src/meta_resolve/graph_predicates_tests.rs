//! `#[cfg(test)]` modules for `graph_predicates` — extracted to a sibling
//! `_tests.rs` (excluded from the oversize-files guard) so the production module
//! stays under the line cap. The nested test modules are descendants of
//! `graph_predicates`, so `super::super::` reaches its (private) items.

mod node_root_gate_differential_tests {
    //! DIFFERENTIAL EQUIVALENCE: the node-domain root gates equal the `TypeExpr`
    //! fronts, field-for-field (verdict AND fence), on inputs that genuinely reach
    //! each path — the `Pick`/`Omit` package SOURCE-root trap, an indexed-access
    //! root, a bare package ref, a workspace-local ref, a non-ref, and a transitive
    //! generic cycle.

    use std::sync::Arc;

    use verter_type_expr::{PrimitiveName, TypeExpr};

    use super::super::{
        node_package_backed_object_like_root_with_fence,
        node_root_reaches_transitive_cycle_with_fence,
    };
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::resolver_core::ComponentMetaQueryEngine;
    use crate::semantic_query::{DeclIdentity, ProjectionMode, SemanticNodeData, SemanticNodeId};
    use crate::types::{AnalysisLevel, HostConfig};
    use crate::{DependencyResolution, VerterHost};

    fn lower(host: &VerterHost, scope: &str, expr: &TypeExpr) -> SemanticNodeId {
        ProjectSemanticDispatch::new(host)
            .lower_type_expr_in_scope_with_mode(scope, expr, ProjectionMode::Navigate)
            .expect("expr must lower")
    }

    #[test]
    fn node_package_backed_root_matches_type_expr_front_field_for_field() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from("export interface VendorProps { a: string; b: number }\n"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script lang=\"ts\">\n\
                 import type { VendorProps } from 'pkg'\n\
                 export interface LocalProps { x: string }\n\
                 </script>\n<template><div /></template>",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        let scope = "/src/App.vue";

        let cases: Vec<TypeExpr> = vec![
            // Pick over a PACKAGE source — the source-root trap (must inspect the
            // VendorProps source, not the `__builtin__::Pick` wrapper).
            TypeExpr::named_with_args(
                "Pick",
                vec![
                    TypeExpr::named("VendorProps"),
                    TypeExpr::string_literal("a"),
                ],
            ),
            // indexed-access over the package source
            TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("VendorProps")),
                index: Arc::new(TypeExpr::string_literal("a")),
            },
            // bare package ref (interface ⇒ object-like)
            TypeExpr::named("VendorProps"),
            // workspace-local ref (NOT package-backed)
            TypeExpr::named("LocalProps"),
            // non-ref root (no extractable root name)
            TypeExpr::Primitive(PrimitiveName::String),
        ];

        let mut any_true = false;
        let mut any_false = false;
        for expr in &cases {
            let node = lower(&host, scope, expr);
            let mut qe_node = ComponentMetaQueryEngine::new(&host);
            let node_result =
                node_package_backed_object_like_root_with_fence(&mut qe_node, scope, node);
            let mut qe_expr = ComponentMetaQueryEngine::new(&host);
            let expr_result =
                crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence(
                    expr, scope, &mut qe_expr,
                );
            assert_eq!(
                node_result, expr_result,
                "node package-backed gate must equal the TypeExpr front (verdict + fence) for {expr:?}"
            );
            if node_result.0 {
                any_true = true;
            } else {
                any_false = true;
            }
        }
        // Genuine reach: the package source IS package-backed; the local ref is NOT
        // — the cases are not vacuously all-equal.
        assert!(
            any_true && any_false,
            "the differential must exercise BOTH a package-backed root and a non-package-backed \
             one (genuine reach), not a single verdict"
        );
    }

    #[test]
    fn node_transitive_cycle_matches_type_expr_front() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from(
                "export type A<T> = B<T>\n\
                 export type B<T> = A<T>\n\
                 export type C<T> = { v: T }\n",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        let scope = "/src/m.ts";

        // cyclic generic (A<string> → B<string> → A<string>) and a non-cyclic one.
        let cyclic =
            TypeExpr::named_with_args("A", vec![TypeExpr::Primitive(PrimitiveName::String)]);
        let acyclic =
            TypeExpr::named_with_args("C", vec![TypeExpr::Primitive(PrimitiveName::String)]);

        for (expr, expect_cycle) in [(&cyclic, true), (&acyclic, false)] {
            let node = lower(&host, scope, expr);
            let node_cycle = node_root_reaches_transitive_cycle_with_fence(&host, scope, node).0;
            let mut qe = ComponentMetaQueryEngine::new(&host);
            // The bool variant `lowered_root_reaches_transitive_cycle` is the
            // production interface (it forwards to the `_with_fence` body and
            // returns `.0`), so the differential pins the same verdict.
            let expr_cycle =
                crate::meta_resolve::lowered_root_reaches_transitive_cycle(&mut qe, scope, expr);
            assert_eq!(
                node_cycle, expr_cycle,
                "node cycle gate must equal the TypeExpr front for {expr:?}"
            );
            assert_eq!(
                node_cycle, expect_cycle,
                "case {expr:?} must GENUINELY reach the expected cycle verdict (not vacuous)"
            );
        }
    }

    /// §4 BARE-REF CYCLE-ROOT RESOLUTION: a `BareRef` carrier (an unresolved
    /// generic reference `A<string>`) must NOT bypass the cycle gate. The collector
    /// resolves the BareRef head through `resolve_carrier_subject_node` under
    /// `Published(Navigate)` and collects the resolved declaration identity — so a
    /// BareRef whose head resolves to a cyclic generic IS detected as a cycle. The
    /// former `_ => {}` arm dropped every BareRef, letting the cycle escape the
    /// guard.
    #[test]
    fn node_cycle_gate_resolves_bare_ref_head_to_cyclic_root() {
        use crate::semantic_query::{NodeScopeId, PrimitiveKind, SemanticNodeData};

        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/m.ts".to_string(),
            Arc::from(
                "export type A<T> = B<T>\n\
                 export type B<T> = A<T>\n\
                 export type C<T> = { v: T }\n",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/m.ts"));
        let scope = "/src/m.ts";

        // Lower `Name<string>` to harvest the InstantiationRef's resolved base
        // scope, then DIRECT-CONSTRUCT a `BareRef` carrier with that SAME file
        // scope so the carrier resolver can resolve the head.
        let make_bare = |name: &str| -> SemanticNodeId {
            let inst = lower(
                &host,
                scope,
                &TypeExpr::named_with_args(name, vec![TypeExpr::Primitive(PrimitiveName::String)]),
            );
            let graph = Arc::clone(host.project_type_store().semantic_graph());
            let (canonical_id, whole_hash) = match graph.node_data(inst).as_deref() {
                Some(SemanticNodeData::InstantiationRef { base, .. }) => {
                    (Arc::clone(&base.canonical_id), base.whole_hash)
                }
                other => {
                    panic!("expected `{name}<string>` to lower to InstantiationRef, got {other:?}")
                }
            };
            let string_node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from(name),
                NodeScopeId::File {
                    canonical_id,
                    whole_hash,
                    local_scope: None,
                },
                Arc::from(vec![string_node].into_boxed_slice()),
            ))
        };

        let bare_a = make_bare("A");
        let bare_c = make_bare("C");

        assert!(
            node_root_reaches_transitive_cycle_with_fence(&host, scope, bare_a).0,
            "a BareRef whose head resolves to a cyclic generic (A → B → A) MUST be detected as a \
             cycle — the head is resolved via resolve_carrier_subject_node, NOT dropped"
        );
        assert!(
            !node_root_reaches_transitive_cycle_with_fence(&host, scope, bare_c).0,
            "a BareRef whose head resolves to an acyclic generic (C) is NOT a cycle (genuine reach)"
        );
    }

    /// §3 PACKAGE-ROOT CANONICAL-ID CORRECTION: a userland `type Pick` is NOT the
    /// builtin utility. With a userland `Pick` shadow declared locally, BOTH fronts
    /// must treat `Pick<VendorProps, 'a'>` as the (workspace-local, NOT
    /// package-backed) userland root — NEVER a builtin source-descent to the
    /// package `VendorProps`. The unshadowed builtin `Omit<VendorProps, 'a'>` in
    /// the SAME scope still descends to the package source. The former string-only
    /// `matches!(name, "Pick" | "Omit")` check descended the userland `Pick` to
    /// `VendorProps` (TRUE), diverging from the node front (FALSE); the
    /// resolver-aware check makes both fronts agree.
    #[test]
    fn node_package_backed_root_distinguishes_builtin_omit_from_userland_pick_shadow() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from("export interface VendorProps { a: string; b: number }\n"),
        );
        // App.vue declares a USERLAND `Pick` (shadowing the builtin) but NOT a
        // userland `Omit`, so the two utilities resolve differently in one scope.
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script lang=\"ts\">\n\
                 import type { VendorProps } from 'pkg'\n\
                 export type Pick<T, K extends keyof T> = { [P in K]: T[P] }\n\
                 </script>\n<template><div /></template>",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![DependencyResolution {
                specifier: "pkg".to_string(),
                resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                possible_canonical_ids: Vec::new(),
            }],
        );
        let scope = "/src/App.vue";

        // userland `Pick` is the root (App.vue, NOT package-backed) ⇒ false.
        let userland_pick = TypeExpr::named_with_args(
            "Pick",
            vec![
                TypeExpr::named("VendorProps"),
                TypeExpr::string_literal("a"),
            ],
        );
        // builtin `Omit` descends to the package `VendorProps` source ⇒ true.
        let builtin_omit = TypeExpr::named_with_args(
            "Omit",
            vec![
                TypeExpr::named("VendorProps"),
                TypeExpr::string_literal("a"),
            ],
        );

        for (expr, expect) in [(&userland_pick, false), (&builtin_omit, true)] {
            let node = lower(&host, scope, expr);
            let mut qe_node = ComponentMetaQueryEngine::new(&host);
            let node_result =
                node_package_backed_object_like_root_with_fence(&mut qe_node, scope, node);
            let mut qe_expr = ComponentMetaQueryEngine::new(&host);
            let expr_result =
                crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence(
                    expr, scope, &mut qe_expr,
                );
            // BOTH fronts agree on the resolver-aware builtin/userland decision.
            assert_eq!(
                node_result, expr_result,
                "node front must equal TypeExpr front (resolver-aware Pick/Omit) for {expr:?}"
            );
            assert_eq!(
                node_result.0, expect,
                "{expr:?}: userland Pick is its own (non-package) root; builtin Omit descends to \
                 the package source"
            );
        }
    }

    /// §3 PACKAGE-ROOT BUILTIN/SHADOW SINGLE SOURCE OF TRUTH: an IMPORTED `Pick`
    /// whose module RESOLVES but does NOT export `Pick` (a module-resolved,
    /// symbol-absent import) shadows the ambient builtin — the resolved import
    /// binding is in the owner-scope shadow set (`prepared_decl` records the binding
    /// once the module resolves, independent of whether `Pick` is actually
    /// exported), so `ScopeShadowing::is_shadowing_lib("Pick")` is `true` and
    /// dispatch's `resolve_bare_ref_head` suppresses the `__builtin__` route. The
    /// node front therefore returns `false` (NO builtin source-descent to the
    /// package `VendorProps`). The `TypeExpr` front MUST agree via the SAME shadow
    /// predicate.
    ///
    /// A `resolve_type_declaration(scope, name).kind == Unknown` builtin heuristic
    /// MISCLASSIFIES this case: it cannot tell "imported, module resolves" (kind ==
    /// Unknown, yet shadowing) apart from "ambient builtin" (kind == Unknown, NOT
    /// shadowing), so it would treat the imported `Pick` as the builtin, descend
    /// into `VendorProps`, and report package-backed — disagreeing with the node
    /// front. This test pins both fronts to `ScopeShadowing::is_shadowing_lib`; it
    /// FAILS against a `kind == Unknown` helper and PASSES through the shadow
    /// predicate.
    #[test]
    fn unresolved_imported_pick_shadow_is_not_a_builtin_source_descent() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        ws.inject_file(
            "/src/node_modules/pkg/index.d.ts".to_string(),
            Arc::from("export interface VendorProps { a: string; b: number }\n"),
        );
        // A RESOLVED local module that does NOT export `Pick` — the import binding
        // shadows the ambient builtin, yet `Pick` resolves to kind == Unknown.
        ws.inject_file(
            "/src/other.ts".to_string(),
            Arc::from("export interface Other { z: string }\n"),
        );
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script lang=\"ts\">\n\
                 import type { VendorProps } from 'pkg'\n\
                 import type { Pick } from './other'\n\
                 </script>\n<template><div /></template>",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/App.vue"));
        host.set_import_dependencies(
            "/src/App.vue",
            vec![
                DependencyResolution {
                    specifier: "pkg".to_string(),
                    resolved_canonical_id: Some("/src/node_modules/pkg/index.d.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
                DependencyResolution {
                    specifier: "./other".to_string(),
                    resolved_canonical_id: Some("/src/other.ts".to_string()),
                    possible_canonical_ids: Vec::new(),
                },
            ],
        );
        let scope = "/src/App.vue";

        let imported_pick = TypeExpr::named_with_args(
            "Pick",
            vec![
                TypeExpr::named("VendorProps"),
                TypeExpr::string_literal("a"),
            ],
        );
        let node = lower(&host, scope, &imported_pick);
        let mut qe_node = ComponentMetaQueryEngine::new(&host);
        let node_result =
            node_package_backed_object_like_root_with_fence(&mut qe_node, scope, node);
        let mut qe_expr = ComponentMetaQueryEngine::new(&host);
        let expr_result =
            crate::meta_resolve::materialize::type_expr_has_package_backed_object_like_root_with_fence(
                &imported_pick, scope, &mut qe_expr,
            );

        // BOTH fronts agree on the resolver-aware shadow decision (the node front
        // already returns `false` via dispatch's `__builtin__` suppression).
        assert_eq!(
            node_result, expr_result,
            "node front must equal the TypeExpr front for a module-resolved, symbol-absent `Pick` shadow \
             (verdict + fence)"
        );
        // The imported `Pick` shadow is the root — NOT a builtin descent to the
        // package `VendorProps`.
        assert!(
            !node_result.0,
            "node front: a module-resolved, symbol-absent `Pick` shadow is not a package-backed root"
        );
        assert!(
            !expr_result.0,
            "TypeExpr front: a module-resolved, symbol-absent `Pick` shadow must NOT descend into the \
             package `VendorProps` — the `kind == Unknown` heuristic wrongly reported \
             package-backed here"
        );
    }

    /// IDENTITY-PRESERVING ROOT (belt-and-braces): the shared package-backed tail
    /// ([`crate::meta_resolve::materialize::package_backed_object_like_root_identity_with_fence`])
    /// resolves the declaration KIND at the CARRIER's own
    /// [`DeclIdentity::canonical_id`] (file X), NEVER by re-resolving the bare
    /// declaration NAME from `scope` (file Y). Here the carrier's identity points
    /// at a PACKAGE `Shared` (file X), while the SAME name `Shared` re-resolves in
    /// `scope` to a DISTINCT workspace-local `Shared` (file Y). The package-backed
    /// verdict MUST follow the carrier identity (file X ⇒ `true`); reverting the
    /// tail to the deleted synthetic `TypeExpr::named(name)` bridge would
    /// re-resolve `Shared` in `scope` and yield the workspace-local `false`,
    /// failing this test.
    #[test]
    fn package_backed_root_uses_carrier_identity_not_scope_reresolution() {
        let ws = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        // File X — a PACKAGE `Shared` (object-like, package-backed).
        ws.inject_file(
            "/src/node_modules/pkgx/index.d.ts".to_string(),
            Arc::from("export interface Shared { a: string }\n"),
        );
        // File Y (scope) — a DISTINCT workspace-local `Shared` (object-like, NOT
        // package-backed). Same name, different declaring file.
        ws.inject_file(
            "/src/App.vue".to_string(),
            Arc::from(
                "<script lang=\"ts\">\n\
                 export interface Shared { x: string }\n\
                 </script>\n<template><div /></template>",
            ),
        );
        let host = VerterHost::new(
            HostConfig {
                analysis_level: AnalysisLevel::Full,
                ..HostConfig::default()
            },
            ws,
        );
        assert!(host.ensure_loaded("/src/node_modules/pkgx/index.d.ts"));
        assert!(host.ensure_loaded("/src/App.vue"));
        let scope = "/src/App.vue";
        let file_x = "/src/node_modules/pkgx/index.d.ts";

        // Harvest the file-X `Shared` identity by lowering it in file X's OWN scope
        // (so the carrier's `canonical_id` is genuinely file X, with its real
        // whole-hash), then DIRECT-CONSTRUCT a clean `DeclRef` carrier from it.
        let file_x_shared: DeclIdentity = {
            let node = lower(&host, file_x, &TypeExpr::named("Shared"));
            let graph = host.project_type_store().semantic_graph();
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::DeclRef { identity }) => identity.clone(),
                Some(SemanticNodeData::InstantiationRef { base, .. }) => base.clone(),
                other => panic!("expected file-X `Shared` to lower to a DeclRef, got {other:?}"),
            }
        };
        assert_eq!(
            file_x_shared.canonical_id.as_ref(),
            file_x,
            "the carrier identity must point at file X (the package `Shared`)"
        );
        let carrier = {
            let graph = host.project_type_store().semantic_graph();
            graph.intern_node(SemanticNodeData::DeclRef {
                identity: file_x_shared,
            })
        };

        // The carrier (file-X identity) is package-backed — resolved at file X, NOT
        // re-resolved by name from `scope`.
        let mut qe = ComponentMetaQueryEngine::new(&host);
        let (carrier_verdict, _fence) =
            node_package_backed_object_like_root_with_fence(&mut qe, scope, carrier);
        assert!(
            carrier_verdict,
            "the carrier's preserved file-X identity (a PACKAGE `Shared`) is package-backed; the \
             gate must resolve at the carrier's own file, not re-resolve `Shared` from `scope`"
        );

        // Non-vacuity: the SAME name `Shared` resolved in `scope` is the
        // workspace-local file-Y `Shared` (NOT package-backed). A name-based bridge
        // would collapse the carrier verdict to THIS `false`, so the `true` above
        // can only come from identity preservation.
        let scope_local = lower(&host, scope, &TypeExpr::named("Shared"));
        let mut qe_scope = ComponentMetaQueryEngine::new(&host);
        let (scope_local_verdict, _) =
            node_package_backed_object_like_root_with_fence(&mut qe_scope, scope, scope_local);
        assert!(
            !scope_local_verdict,
            "the scope-local `Shared` (file Y) is workspace-local, not package-backed — proving \
             the carrier verdict is identity-driven, never name-driven"
        );
    }
}

#[cfg(test)]
mod carrier_descent_tests {
    //! Carrier-arg descent for the cycle-BFS ref/recursive-ref walkers.
    //!
    //! `collect_ref_identities_node` and `body_contains_recursive_ref_to_name`
    //! walk a lowered body's structural children to discover declaration
    //! references and recursive-ref back-edges. A `BareRef` / `TypeOf` /
    //! `ImportType` carrier applies its `type_args` at the reference site; those
    //! args can themselves carry a `DeclRef` / `InstantiationRef` (a real
    //! cross-decl edge) or an `Opaque(RecursiveRef)` (a cycle back-edge). The
    //! walkers MUST descend `SemanticNodeData::carrier_type_args` so those
    //! identities / back-edges are not silently dropped — a missed edge would
    //! under-collect the cycle graph and let a genuine cycle escape the guard.
    //!
    //! Each test DIRECT-CONSTRUCTS a carrier (no head resolution — that is the
    //! producer's job) and asserts only the DESCENT into its args. Discrimination
    //! is the negative assertion: against the pre-descent `_ => {}` arm the
    //! identity / back-edge is missed.

    use std::sync::Arc;

    use crate::semantic_query::{
        DeclIdentity, NodeScopeId, QueryError, ScopeId, SemanticNodeData, SemanticNodeId,
        ValueRootKey,
    };
    use crate::semantic_query_memo::SemanticGraphStore;

    use super::super::{body_contains_recursive_ref_to_name, collect_ref_identities_node};

    fn decl_identity(canonical: &str, name: &str) -> DeclIdentity {
        DeclIdentity::from_scope(
            &NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                whole_hash: [7u8; 16],
                local_scope: None,
            },
            Arc::from(name),
        )
    }

    /// Build the three carriers, each wrapping `arg` as its single `type_args`
    /// entry, so a single descent assertion covers all three carrier kinds.
    fn carriers_wrapping(graph: &SemanticGraphStore, arg: SemanticNodeId) -> Vec<SemanticNodeId> {
        let args: Arc<[SemanticNodeId]> = Arc::from(vec![arg].into_boxed_slice());
        vec![
            graph.intern_node(SemanticNodeData::new_bare_ref(
                Arc::from("Foo"),
                NodeScopeId::Global,
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_typeof(
                ValueRootKey {
                    scope: ScopeId {
                        canonical_id: Arc::from("/v.ts"),
                        local_scope: None,
                    },
                    name: Arc::from("factory"),
                },
                Arc::from(Vec::new().into_boxed_slice()),
                Arc::clone(&args),
            )),
            graph.intern_node(SemanticNodeData::new_import_type(
                Arc::from("./m"),
                Arc::from(vec![Arc::<str>::from("G")].into_boxed_slice()),
                Arc::clone(&args),
                false,
            )),
        ]
    }

    // ── D1 — collect_ref_identities_node descends carrier args ──────────────
    //
    // A `DeclRef` (and an `InstantiationRef`) inside a carrier's `type_args` IS
    // a declaration edge. `collect_ref_identities_node` must collect it.
    // NEGATIVE: with the unchanged `_ => {}` arm the carrier is a leaf and the
    // identity is missed (the collected set would be empty).
    #[test]
    fn collect_ref_identities_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let inner_id = decl_identity("/dep.ts", "Inner");
        let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: inner_id.clone(),
        });

        for carrier in carriers_wrapping(&graph, decl_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            collect_ref_identities_node(&graph, carrier, &mut out, 0);
            assert!(
                out.iter().any(|(id, _)| *id == inner_id),
                "a DeclRef inside a carrier's type_args must be collected; got {out:?} for \
                 carrier {:?}",
                graph.node_data(carrier).as_deref()
            );
        }

        // InstantiationRef arg variant — the base identity is collected with
        // `has_type_args = true`.
        let inst_base = decl_identity("/dep.ts", "Box");
        let inst_ref = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: inst_base.clone(),
            args: Arc::from(Vec::new().into_boxed_slice()),
        });
        for carrier in carriers_wrapping(&graph, inst_ref) {
            let mut out: Vec<(DeclIdentity, bool)> = Vec::new();
            collect_ref_identities_node(&graph, carrier, &mut out, 0);
            assert!(
                out.iter().any(|(id, _)| *id == inst_base),
                "an InstantiationRef inside a carrier's type_args must be collected; got {out:?}"
            );
        }
    }

    // ── D2 — body_contains_recursive_ref_to_name descends carrier args ──────
    //
    // An `Opaque(RecursiveRef { name })` inside a carrier's `type_args` is a
    // cycle back-edge to `name`. NEGATIVE: with the unchanged `_ => {}` arm the
    // carrier is a leaf and the predicate returns `false`.
    #[test]
    fn body_contains_recursive_ref_descends_carrier_args() {
        let graph = SemanticGraphStore::new();
        let target: Arc<str> = Arc::from("SelfRef");
        let rec = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::clone(&target),
        }));

        for carrier in carriers_wrapping(&graph, rec) {
            assert!(
                body_contains_recursive_ref_to_name(&graph, carrier, &target, 0),
                "a RecursiveRef back-edge inside a carrier's type_args must be found for `{}`; \
                 carrier {:?}",
                target,
                graph.node_data(carrier).as_deref()
            );
        }

        // NEGATIVE control: a carrier whose args contain a RecursiveRef to a
        // DIFFERENT name does NOT match the target (proving the descent reads
        // the actual name, not a blanket true).
        let other = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
            name: Arc::from("OtherName"),
        }));
        for carrier in carriers_wrapping(&graph, other) {
            assert!(
                !body_contains_recursive_ref_to_name(&graph, carrier, &target, 0),
                "a carrier whose args reference a DIFFERENT name must NOT match the target"
            );
        }
    }
}
