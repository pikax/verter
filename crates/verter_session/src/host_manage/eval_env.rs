//! `host_manage::eval_env` — eval-env builders, file-analysis snapshot
//! constructors, and evaluated-type computation.
//!
//! Domain G. Owns the host's `base_eval_env_arc` artifact read, the
//! `FileAnalysisSnapshot` builders for parse / source flows, and the
//! per-owner evaluated-type compute path. Public surface
//! remains rooted at `crate::host_manage::*`; this file contributes a
//! continuation `impl VerterHost { … }` block.

use std::sync::Arc;

use crate::instant::Instant;
use crate::resolver_core::ValueDeclIdentity;
use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
    is_raw_import_specifier_id, log_snapshot_debug, resolve_eval_dependency_canonical_with,
    ComputedEvaluatedTypes,
};

impl VerterHost {
    /// The canonical per-file `EvalEnv` for a base (non-overlay) read.
    ///
    /// A DEMAND product for whole-file consumers (fallthrough, runtime
    /// values, value-alias peeling): the artifact's lazy declaration-body
    /// memo materialises the whole-file env once through the retained
    /// scheduler-side parse snapshot and memoizes it (script-setup type
    /// params applied). The per-symbol query path never touches it, and
    /// publishing the artifact never builds it. There is no separate env
    /// cache and no env-only build path.
    pub(crate) fn base_eval_env_arc(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_eval::EvalEnv>> {
        component_meta_trace_custom!(
            "base_eval_env",
            format!("owner={} store_view={}", canonical_id, false),
        );
        let resolved_canonical_id = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        // The base whole-file eval env is the memo-backed product of the
        // canonical's `IndexedReady`: `ensure_indexed_ready_serve` performs the
        // one cold materialise (or joins the published artifact), and the
        // memo's `whole_env()` lowers the file's declaration set once through
        // the retained eval program — applying SFC `<script setup generic>`
        // params and the Svelte rune ambient env per file. No eager rebuild,
        // no second parse.
        let indexed = self
            .ensure_indexed_ready_serve(resolved_canonical_id.as_str())?
            .indexed;
        component_meta_trace_custom!(
            "base_eval_env_built",
            format!("owner={} whole_hash={:?}", canonical_id, indexed.whole_hash),
        );
        Some(indexed.shallow_state.decl_bodies().whole_env())
    }

    pub(crate) fn local_type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        // Resolve the owner from the cached declaration-header inventory
        // before consulting the whole env. A Vue canonical can contain both
        // module and setup owners; the legacy ordinary-owner lookup lost a
        // setup-local declaration, while the former file-wide import guard
        // let an import in one owner hide a real declaration in another.
        // Exactly one declaration owner is required — multi-owner same-name
        // declarations are ambiguous at this owner-agnostic API and fail
        // closed. An import-only name has no declaration header and therefore
        // also returns `None` without a permissive name rematch.
        let state = self.routed_shallow_state(canonical_source)?;
        let owner = Self::unique_local_type_declaration_owner_in(&state, resolved_name)?;

        // Read the id through the memo-owned whole-env `Arc` at the proven
        // owner — a single map lookup; never a whole-env deep clone.
        let oracle = self
            .base_eval_env_arc(canonical_source)
            .and_then(|env| env.type_declaration_id_in(owner, resolved_name));
        // Non-breaking readiness cross-check (debug/test only): the
        // bounded graph-native reader must AGREE with the oracle on
        // PRESENCE (`Some`/`None`). The oracle stays authoritative for the
        // id VALUE (see the reader's doc on stable-unique vs
        // equal-to-oracle); release builds skip this entirely.
        debug_assert_eq!(
            oracle.is_some(),
            self.local_type_declaration_id_graph_native(canonical_source, resolved_name)
                .is_some(),
            "graph-native consumer-reader readiness: graph-native C1 reader diverged from the oracle on presence for \
             ({canonical_source}, {resolved_name})"
        );
        oracle
    }

    /// Return the sole authored owner declaring `resolved_name` in the
    /// canonical's cached header inventory. The owner-agnostic declaration-id
    /// API cannot choose between two lexical owners, so ambiguity is `None`.
    /// Import bindings never enter this inventory and cannot mask a local
    /// declaration owned by another SFC region.
    fn unique_local_type_declaration_owner_in(
        state: &crate::resolver_core::shallow_file_state::ShallowFileState,
        resolved_name: &str,
    ) -> Option<verter_type_expr::TopLevelOwnerId> {
        let mut owner = None;
        for key in state
            .decl_bodies()
            .header_index()
            .type_headers
            .keys()
            .filter(|key| key.name.as_ref() == resolved_name)
        {
            match owner {
                None => owner = Some(key.owner),
                Some(existing) if existing == key.owner => {}
                Some(_) => return None,
            }
        }
        owner
    }

    /// Bounded, graph-native presence reader for the C1 consumer
    /// (`local_type_declaration_id`). Routes the unique declaration-owner +
    /// local-type PRESENCE checks through `routed_shallow_state` and the
    /// per-symbol declaration-header index — it NEVER materialises
    /// `whole_env()` / `base_eval_env_arc`.
    ///
    /// The legacy oracle stays in production: the `DeclarationId` the
    /// oracle assigns is the 1-based ordinal in the INTERLEAVED
    /// type+value `add_type`/`add_value` registration order of
    /// `build_eval_env` (a single shared `next_declaration_id` counter,
    /// every `TypeDeclInfo.declaration_id == 0` at build time). That
    /// interleaving is NOT recoverable from the unordered, kind-split
    /// `DeclHeaderIndex` without replaying the registration walk, so a
    /// graph-native reader cannot reconstruct an EQUAL id without
    /// re-materialising the whole env.
    ///
    /// This is acceptable because the id is an OPAQUE in-process identity
    /// token: it never crosses the FFI/wire surface
    /// (`FfiResolvedTypeDeclaration` carries no `declaration_id`), is
    /// never compared cross-file, and no production reader branches on
    /// its value. The C1 contract is therefore STABLE-AND-UNIQUE per
    /// `(file, name)`, NOT EQUAL-TO-ORACLE. This reader returns a stable
    /// per-`(file, name)` id derived from the header index's
    /// deterministic name ordering, with the legacy oracle retained as
    /// the production path. The bound-test asserts it never materialises
    /// `whole_env()`; the equivalence-test pins its
    /// presence-equivalence (`Some`/`None`) to the oracle.
    pub(crate) fn local_type_declaration_id_graph_native(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        let state = self.routed_shallow_state(canonical_source)?;
        let owner = Self::unique_local_type_declaration_owner_in(&state, resolved_name)?;
        // Presence WITHOUT body lowering — a header miss is `None`,
        // mirroring the oracle's `type_declaration_id` miss.
        let header_index = state.decl_bodies().header_index();
        header_index.type_header_in(owner, resolved_name)?;
        // Stable-unique per `(file, name)`: a deterministic ordinal over
        // the header index's sorted type-symbol names. This is NOT the
        // oracle's interleaved id (see the doc comment) but is stable
        // across reads for an unchanged file and unique within the file —
        // the only property the consumer's opaque-token use requires.
        let mut type_names: Vec<&str> = header_index
            .type_headers
            .keys()
            .filter(|key| key.owner == owner)
            .map(|key| key.name.as_ref())
            .collect();
        type_names.sort_unstable();
        type_names
            .iter()
            .position(|name| *name == resolved_name)
            .map(|ordinal| (ordinal as u64) + 1)
    }

    fn peel_value_decl_alias(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> ValueDeclIdentity {
        let mut current = ValueDeclIdentity {
            canonical_id: canonical_id.to_string(),
            owner,
            name: name.to_string(),
        };
        let mut visited = rustc_hash::FxHashSet::default();

        loop {
            if !visited.insert(current.clone()) {
                break;
            }

            let Some(env) = self.base_eval_env_arc(current.canonical_id.as_str()) else {
                break;
            };
            let Some(group) = env.value_group_in(current.owner, current.name.as_str()) else {
                break;
            };
            let decl = group.primary();
            // The `ValueTypeAnnotationFact` producer (verter_semantic
            // fact_projection.rs) already encodes the single-hop + self-ref-break
            // termination guard: `typeof_alias_target` is `Some` IFF the
            // annotation is a single-segment `typeof x` whose target is not this
            // declaration itself, so a multi-hop `typeof x.y` or a self-peel
            // yields `None` here and terminates the walk. The membership check
            // stays session-side.
            let Some(target) = decl.type_annotation.typeof_alias_target.as_ref() else {
                break;
            };
            let next = ValueDeclIdentity {
                canonical_id: target.canonical_id.to_string(),
                owner: target.owner,
                name: target.symbol.to_string(),
            };
            let Some(target_env) = self.base_eval_env_arc(next.canonical_id.as_str()) else {
                break;
            };
            if target_env
                .value_group_in(next.owner, next.name.as_str())
                .is_none()
            {
                break;
            }

            current = next;
        }

        // Non-breaking readiness cross-check (debug/test only): the
        // graph-native peeler must land on the SAME terminal identity. The
        // rune-module case is now covered too — both peelers resolve a
        // `$`-rune ambient hop through the SAME centralized effective lookup
        // (`ShallowFileState::effective_value_decl` / `effective_value_header_present`),
        // so there is no rune-module exception.
        #[cfg(debug_assertions)]
        {
            let graph_native = self.peel_value_decl_alias_graph_native(canonical_id, owner, name);
            debug_assert_eq!(
                current, graph_native,
                "graph-native consumer-reader readiness: graph-native C2 peeler diverged from the oracle for \
                 ({canonical_id}, {owner:?}, {name})"
            );
        }

        current
    }

    /// Bounded, graph-native sibling of [`Self::peel_value_decl_alias`].
    ///
    /// Walks the same single-segment `typeof` alias chain, but per hop
    /// reads exactly the ONE demanded value symbol's lowered body via
    /// `routed_shallow_state(cur).effective_value_decl(name)` (the lazy
    /// per-symbol memo, with the centralized rune-ambient overlay) and
    /// resolves the `env.value_symbols.contains_key(next)` membership
    /// through `effective_value_header_present(next)` (per-symbol
    /// declaration-header PRESENCE plus the rune ambient — no body
    /// lowering), NEVER `whole_env()` / `base_eval_env_arc`. The
    /// visited-set, same-name, and path-length guards are byte-identical
    /// to the oracle.
    ///
    /// Rune ambient parity: a Svelte rune MODULE (`.svelte.ts` /
    /// `.svelte.js`) exposes its ambient `$`-rune value symbols (`$state`,
    /// `$derived`, …) through the CENTRALIZED effective lookup
    /// ([`crate::resolver_core::ShallowFileState::effective_value_decl`] /
    /// `effective_value_header_present`) — the SAME single authority the
    /// oracle's `whole_env()` folds in. So a `typeof $rune` hop terminates
    /// identically here and in the oracle; there is no rune-module
    /// exception.
    fn peel_value_decl_alias_graph_native(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> ValueDeclIdentity {
        let mut current = ValueDeclIdentity {
            canonical_id: canonical_id.to_string(),
            owner,
            name: name.to_string(),
        };
        let mut visited = rustc_hash::FxHashSet::default();

        loop {
            if !visited.insert(current.clone()) {
                break;
            }

            let Some(state) = self.routed_shallow_state(current.canonical_id.as_str()) else {
                break;
            };
            // The CENTRALIZED effective lookup surfaces rune-ambient symbols in
            // a rune module too, so the peeler agrees with the oracle for rune
            // modules without this body knowing about the rune prelude.
            let Some(lowered) = state.effective_value_decl_in(current.owner, current.name.as_str())
            else {
                break;
            };
            // The `ValueTypeAnnotationFact` producer (verter_semantic
            // fact_projection.rs) already encodes the single-hop +
            // self-ref-break termination guard: `typeof_alias_target` is
            // `Some` IFF the annotation is a single-segment `typeof x` whose
            // target is not this declaration itself, so a multi-hop
            // `typeof x.y` or a self-peel yields `None` here and terminates
            // the walk — byte-identical termination to the retired
            // `TypeExpr::TypeOf` match. The membership check stays
            // session-side.
            let Some(target) = lowered.type_annotation.typeof_alias_target.as_ref() else {
                break;
            };
            let next = ValueDeclIdentity {
                canonical_id: target.canonical_id.to_string(),
                owner: target.owner,
                name: target.symbol.to_string(),
            };
            // Membership via header PRESENCE — no body lowering of the
            // next symbol just to learn it exists; effective presence so a
            // `typeof $rune` hop in a rune module is seen too.
            let Some(target_state) = self.routed_shallow_state(next.canonical_id.as_str()) else {
                break;
            };
            if !target_state.effective_value_header_present_in(next.owner, next.name.as_str()) {
                break;
            }

            current = next;
        }

        current
    }

    /// Bounded, graph-native per-name value-symbol reader for the C4
    /// consumer (`dependency_eval_env`'s sole use:
    /// `source_env.value_symbols.get(&source_name)` →
    /// `dep_group.primary().clone()` after a `prepared_value_decl` miss).
    ///
    /// Reads exactly the demanded value symbol's lowered body via
    /// `routed_shallow_state(src).value_decl(name)` (the lazy per-symbol
    /// memo — synthesised `.vue` default first, then the memo) and
    /// converts the `LoweredValueDecl` into the `ValueDeclInfo` the
    /// materializer's whole-env read produced. NEVER materialises
    /// `whole_env()` / `base_eval_env_arc`.
    ///
    /// `declaration_id = 0` matches the import-alias hydration path the
    /// materializer already uses for the prepared route
    /// (`prepared_value_decl_to_value_decl_info`): the id is opaque and
    /// is overwritten downstream when the alias takes the importing
    /// binding's name. The `name` is the demanded `source_name`,
    /// matching the oracle's `dep_group.primary().name`.
    pub(crate) fn dependency_value_symbol_graph_native(
        &self,
        source: &ValueDeclIdentity,
    ) -> Option<verter_semantic::analysis::type_eval::ValueDeclInfo> {
        let state = self.routed_shallow_state(&source.canonical_id)?;
        // The CENTRALIZED effective lookup applies user-wins → rune-ambient →
        // miss, so a Svelte rune module's ambient `$state`/`$derived`/`$effect`/
        // `$inspect` resolve here WITHOUT this reader knowing anything about the
        // rune prelude — the single authority lives on `ShallowFileState`.
        let lowered = state.effective_value_decl_in(source.owner, &source.name)?;
        Some(verter_semantic::analysis::type_eval::ValueDeclInfo {
            owner: source.owner,
            name: source.name.clone(),
            declaration_id: 0,
            kind: lowered.kind,
            type_annotation: lowered.type_annotation.clone(),
            signatures: lowered.signatures.clone(),
            object_shape: lowered.object_shape.clone(),
            enum_members: lowered.enum_members.clone(),
            enum_member_names: lowered.enum_member_names.clone(),
        })
    }

    /// Test-only `(canonical, name)` view of the C4 graph-native reader.
    #[cfg(test)]
    pub(crate) fn dependency_value_symbol_graph_native_for_test(
        &self,
        source_canonical_id: &str,
        source_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::ValueDeclInfo> {
        self.dependency_value_symbol_graph_native(&ValueDeclIdentity {
            canonical_id: source_canonical_id.to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            name: source_name.to_string(),
        })
    }

    /// Test-only exact-identity view of the legacy oracle peeler.
    #[cfg(test)]
    pub(crate) fn peel_value_decl_alias_for_test(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> ValueDeclIdentity {
        self.peel_value_decl_alias(canonical_id, owner, name)
    }

    /// Test-only exact-identity view of the graph-native peeler.
    #[cfg(test)]
    pub(crate) fn peel_value_decl_alias_graph_native_for_test(
        &self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
    ) -> ValueDeclIdentity {
        self.peel_value_decl_alias_graph_native(canonical_id, owner, name)
    }

    pub(crate) fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        let target = self.resolve_value_export_route_identity(dep_canonical_id, imported_name)?;
        Some(self.peel_value_decl_alias(&target.canonical_id, target.owner, &target.name))
    }

    /// Bounded, graph-native sibling of [`Self::resolve_value_export_target`].
    ///
    /// Resolves the same export target through the graph-native export
    /// walk (`resolve_named_export` already walks the export graph, never
    /// the whole env), then peels the value alias chain through
    /// [`Self::peel_value_decl_alias_graph_native`] (per-symbol value
    /// memo + header PRESENCE) instead of the legacy
    /// [`Self::peel_value_decl_alias`] (which materialises
    /// `base_eval_env_arc`/`whole_env()`). NEVER materialises a
    /// dependency's whole env.
    pub(crate) fn resolve_value_export_target_graph_native(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        let target = self.resolve_value_export_route_identity(dep_canonical_id, imported_name)?;
        Some(self.peel_value_decl_alias_graph_native(
            &target.canonical_id,
            target.owner,
            &target.name,
        ))
    }

    fn resolve_value_export_route_identity(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ValueDeclIdentity> {
        let (route_result, _) =
            self.build_named_type_export_route_entry(dep_canonical_id, imported_name)?;
        let (canonical_id, owner, name) = route_result.resolved()?;
        let canonical_id = self
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());
        Some(ValueDeclIdentity {
            canonical_id,
            owner,
            name: name.to_string(),
        })
    }

    /// View-aware, FULL-CHAIN-fact value-export root resolver — the VALUE
    /// counterpart of
    /// [`Self::resolve_imported_type_root_with_facts_with_store_view`].
    ///
    /// Resolves a value re-export to its FINAL defining value AND returns the
    /// version facts of EVERY file on the re-export walk (each participant's
    /// `FileWholeHash` + route surface), then peels the terminal SAME-FILE
    /// `typeof` value alias.
    ///
    /// Integration role at the sole production call site
    /// (`build_prepared_import_canonicalization`): this rail is reached ONLY
    /// when the symbol-space-NEUTRAL TYPE rail above it did NOT produce a
    /// DIFFERENT final canonical — i.e. it resolved the name to the BARREL
    /// itself, covering BOTH a same-file resolution AND a type-route
    /// miss/fallback (both return the barrel). The type rail's
    /// participant-accumulating walk follows EVERY cross-file re-export hop —
    /// value-only re-exports included, because module resolution is
    /// symbol-space-neutral — and short-circuits the moment it lands cross-file;
    /// so by the time this rail runs, the only remaining work is the SAME-FILE
    /// terminal value-alias peel (`export const V: typeof realImpl = realImpl`
    /// declared on the barrel itself → `realImpl`). That same-file `typeof` peel
    /// is this rail's distinct live contribution; the cross-file fact
    /// completeness is delivered by the type rail's full-chain walk, not here.
    /// The rail is whole-env-free and SYMMETRIC with the type rail, so it stays
    /// correct if the integration ordering ever changes.
    ///
    /// Two graph-native sub-walks, both whole-env-free; NEVER routes through
    /// `peel_value_decl_alias` / `base_eval_env_arc` / `whole_env()`:
    ///
    /// 1. The re-export CHAIN walk reuses
    ///    [`Self::build_named_type_export_route_entry`] — the shared
    ///    participant-accumulating walk over `ShallowFileState::export_target`
    ///    (`Local` / `Reexport`, module resolution is symbol-space-neutral, so
    ///    a `export { V } from './mid'` value re-export traverses identically to
    ///    a type re-export). It returns the terminal defining `(canonical,
    ///    name)` plus the participant `FileWholeHash` + `Route` facts; a
    ///    fenced-serve walk returns EMPTY facts (the strict-admission
    ///    negative-cache contract), so this resolver inherits it. The terminal
    ///    canonical is normalized through
    ///    [`Self::resolve_eval_dependency_canonical`] — the SAME normalization
    ///    the type rail applies to its final `defining_canonical` — so a final
    ///    that an eval-dependency alias collapses onto the barrel is reported
    ///    identically by both rails (parity; no spurious cross-file divergence).
    /// 2. The terminal value `typeof`-alias is peeled graph-native via
    ///    [`Self::peel_value_decl_alias_graph_native`] (per-symbol value memo +
    ///    header PRESENCE). The peeled identity is the final defining value.
    ///
    /// `view` carries the request boundary, symmetric with the type rail's
    /// `_with_store_view` entry: the participant CHAIN walk (shared
    /// `build_named_type_export_route_entry`) is the view-INDEPENDENT cold
    /// compute — exactly as the type rail's identically-named cold closure is —
    /// and the recorded chain facts are validated against `view` by the
    /// consuming bundle's `ReadSetSignature` fact rail at warm-read time. The
    /// parameter is the seam through which a future value-space route cache can
    /// validate a cached value-export entry against the supplied view.
    pub(crate) fn resolve_value_export_root_with_facts_with_store_view(
        &self,
        _view: &dyn crate::resolver_core::StoreView,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> (
        Option<ValueDeclIdentity>,
        Vec<crate::resolver_core::FactVersionRef>,
    ) {
        let Some((route_result, chain_facts)) =
            self.build_named_type_export_route_entry(dep_canonical_id, imported_name)
        else {
            return (None, Vec::new());
        };
        let Some((final_canonical, final_owner, final_name)) = route_result.resolved() else {
            // A stable Miss carries the participant facts so a later-appearing
            // export still invalidates a recorded miss; no value identity.
            return (None, chain_facts);
        };
        // Normalize the terminal canonical exactly as the type rail normalizes
        // its final `defining_canonical` (companion-declaration / bundle-entry
        // collapse) so the two rails agree on the final identity — parity, no
        // value-only divergence on an eval-dependency-aliased final.
        let final_canonical = self
            .resolve_eval_dependency_canonical(final_canonical)
            .unwrap_or_else(|| final_canonical.to_string());
        // Peel the terminal value alias graph-native (no whole-env). A pure
        // `export const V` terminal peels to itself.
        let identity = self.peel_value_decl_alias_graph_native(
            final_canonical.as_str(),
            final_owner,
            final_name,
        );
        (Some(identity), chain_facts)
    }

    pub(crate) fn build_snapshot_from_parse(parse: crate::ParseSnapshot) -> FileAnalysisSnapshot {
        // The snapshot is shared by `Arc`; this builder consumes a freshly
        // parsed `ParseSnapshot` whose snapshot is uniquely held, so
        // `unwrap_or_clone` moves the inner value out without copying (it only
        // deep-copies in the rare case the snapshot is still shared).
        let script_analysis = Arc::unwrap_or_clone(parse.script_analysis);
        FileAnalysisSnapshot {
            imports: script_analysis.imports,
            bindings: script_analysis.bindings,
            module_references: Arc::new(script_analysis.module_references),
            macros: Arc::new(script_analysis.macros),
            macro_type_deps: Arc::new(script_analysis.macro_type_deps),
            script_flags: script_analysis.flags.bits(),
            styles: Arc::new(parse.style_analyses),
            template: None,
            vue_api_calls: Arc::new(script_analysis.vue_api_calls),
            dom_query_calls: Arc::new(script_analysis.dom_query_calls),
            css_var_manipulations: Arc::new(script_analysis.css_var_manipulations),
            script_binding_occurrences: Arc::new(script_analysis.script_binding_occurrences),
            export_signatures: Arc::new(parse.export_signatures),
            options_api: script_analysis.options_api,
            store_usages: Arc::new(script_analysis.store_usages),
            store_definitions: Arc::new(script_analysis.store_definitions),
            is_typescript: script_analysis.is_typescript,
        }
    }

    pub(crate) fn build_snapshot_and_template_inputs_from_source(
        &self,
        canonical: &str,
        source: &Arc<str>,
        store_published: bool,
    ) -> (
        FileAnalysisSnapshot,
        Option<crate::types::VueTemplateInputs>,
    ) {
        component_meta_trace_custom!(
            "build_snapshot_from_source",
            format!("owner={} bytes={}", canonical, source.len()),
        );
        let file_language = self.language_classifier.classify(canonical);
        if file_language.is_vue() {
            component_meta_trace_custom!("parse_vue_snapshot", format!("owner={canonical}"));
            let (parse, parsed) = crate::parse::parse_vue_snapshot(
                canonical,
                source,
                self.config.effective_scope(),
                &self.provenance,
            );
            component_meta_trace_custom!(
                "parse_vue_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            let template_inputs = crate::types::VueTemplateInputs {
                source: Arc::clone(source),
                framework_parse: Some(parsed),
                store_published,
                // This builder reads no scheduler node, so it can
                // never attest a node generation; the computed
                // template serves the caller but never persists.
                source_generation: None,
            };
            (
                Self::build_snapshot_from_parse(parse),
                Some(template_inputs),
            )
        } else {
            component_meta_trace_custom!("parse_non_sfc_snapshot", format!("owner={canonical}"));
            let parse = crate::parse::parse_non_sfc_snapshot(
                canonical,
                source,
                &file_language,
                &self.provenance,
            );
            component_meta_trace_custom!(
                "parse_non_sfc_snapshot_result",
                format!(
                    "owner={} imports={} macros={} export_signatures={}",
                    canonical,
                    parse.script_analysis.imports.len(),
                    parse.script_analysis.macros.len(),
                    parse.export_signatures.len(),
                ),
            );
            (Self::build_snapshot_from_parse(parse), None)
        }
    }

    pub(in crate::host_manage) fn finalize_analysis_snapshot(
        &self,
        canonical: &str,
        mut snapshot: FileAnalysisSnapshot,
        needs_template_analysis: bool,
        template_inputs: Option<crate::types::VueTemplateInputs>,
        analysis_started: Option<Instant>,
    ) -> FileAnalysisSnapshot {
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if needs_template_analysis {
            // No coherent inputs (torn generation join, non-SFC) →
            // the template stays absent for this caller — fail closed.
            if let Some(inputs) = template_inputs {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot, inputs);
            }
        }
        if let Some(started) = analysis_started {
            log_snapshot_debug("get_analysis", canonical, started, &snapshot);
        }
        snapshot
    }

    fn is_expanded_types_empty(
        result: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    ) -> bool {
        result.is_empty()
    }

    pub(crate) fn current_eval_state(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_language::FrameworkParseArtifact>>,
        Hash16,
    )> {
        if canonical_id.is_empty() {
            return None;
        }

        let normalized_canonical_id = self.normalized_analysis_canonical(canonical_id);
        // The uncached path already hits the project-global `FileArtifactStore`
        // for repeated probes, so no per-request memo layer is needed.
        self.current_eval_state_uncached(normalized_canonical_id.as_ref())
    }

    fn current_eval_state_uncached(
        &self,
        canonical_id: &str,
    ) -> Option<(
        Arc<str>,
        Option<Arc<verter_language::FrameworkParseArtifact>>,
        Hash16,
    )> {
        component_meta_trace_custom!("current_eval_state", format!("owner={}", canonical_id),);

        // FileArtifactStore fast path — **current-content-pinned** (no
        // `get_any`). `current_eval_state` returns the canonical's source
        // for the cold type-evaluation recompute; a stale artifact would
        // feed pre-edit source into the evaluation. With the own-canonical
        // drain retired a stale pre-edit `IndexedReady` can linger past a
        // same-canonical edit, so the artifact read is pinned to the
        // canonical's authoritative current content hash:
        // `current_content_pinned_indexed` serves only a content-current
        // artifact for a scheduler-tracked canonical, and
        // `artifact_current_indexed` answers for a genuinely artifact-only
        // canonical. A stale candidate for a live scope misses both — the
        // scheduler source path below is the authoritative current content.
        let cached_facts = self
            .current_content_pinned_indexed(canonical_id)
            .or_else(|| self.artifact_current_indexed(canonical_id));
        if let Some(facts) = cached_facts {
            return Some((
                Arc::clone(&facts.raw_source),
                facts.framework_parse.clone(),
                facts.whole_hash,
            ));
        }

        // Scheduler source path for files loaded via `ensure_loaded` but not
        // yet materialized into `FileArtifactStore`. The scheduler is the sole
        // source authority; on miss, call `ensure_loaded` once.
        if let Some(state) = self.effective_file_state(canonical_id, None) {
            return Some((state.source, state.framework_parse, state.whole_hash));
        }
        if !canonical_id.is_empty()
            && !is_raw_import_specifier_id(canonical_id)
            && self.ensure_loaded(canonical_id)
        {
            if let Some(state) = self.effective_file_state(canonical_id, None) {
                return Some((state.source, state.framework_parse, state.whole_hash));
            }
        }
        None
    }

    pub(crate) fn resolve_eval_dependency_canonical(&self, dep_canonical: &str) -> Option<String> {
        // Request-scoped POSITIVE-ONLY memo
        // (`RequestContext::dep_canonical_memo`). One request
        // re-normalizes the same dependency canonicals tens of thousands
        // of times, and every cold walk probes up to 14 candidate
        // canonicals through `analysis_source_exists` (artifact store +
        // scheduler + workspace VFS); a memo hit skips the whole walk.
        // `None` results are deliberately NOT memoized — a mid-request
        // artifact publication / load can turn a `None` into a hit, and
        // a stale pinned `None` would misroute the rest of the request.
        // Empty ids and raw import specifiers bypass the memo (mirroring
        // the `normalized_analysis_canonical` guard); with no request
        // context installed the behavior is unchanged.
        let memo_ctx = if dep_canonical.is_empty() || is_raw_import_specifier_id(dep_canonical) {
            None
        } else {
            crate::request_context::current_request_context()
        };
        if let Some(ctx) = memo_ctx.as_ref() {
            if let Some(hit) = ctx.dep_canonical_memo.lock().get(dep_canonical).cloned() {
                return Some(hit);
            }
        }
        let resolved = resolve_eval_dependency_canonical_with(dep_canonical, |candidate| {
            self.analysis_source_exists(candidate)
        });
        if let (Some(ctx), Some(resolved)) = (memo_ctx.as_ref(), resolved.as_ref()) {
            ctx.dep_canonical_memo
                .lock()
                .insert(dep_canonical.to_string(), resolved.clone());
        }
        resolved
    }

    pub(crate) fn normalized_analysis_canonical<'a>(
        &self,
        canonical_id: &'a str,
    ) -> std::borrow::Cow<'a, str> {
        if canonical_id.is_empty() || is_raw_import_specifier_id(canonical_id) {
            return std::borrow::Cow::Borrowed(canonical_id);
        }

        self.resolve_eval_dependency_canonical(canonical_id)
            .map(std::borrow::Cow::Owned)
            .unwrap_or_else(|| std::borrow::Cow::Borrowed(canonical_id))
    }

    pub(crate) fn cache_dependency_candidates_from_snapshot(
        &self,
        owner_canonical_id: &str,
        snapshot: &FileAnalysisSnapshot,
    ) -> std::collections::BTreeSet<String> {
        let mut candidates = std::collections::BTreeSet::new();

        if let Some(serve) = self.ensure_indexed_ready_serve(owner_canonical_id) {
            let facts = &serve.indexed;
            // Baked-edge currency gate. The artifact's cross-file edge
            // `canonical_id`s were resolved at materialise time; a
            // FENCED (ReturnOnly) serve carries edges baked against the
            // pre-mutation file set, so trusting them would track the
            // superseded dependency targets. Consume baked edges only
            // from a store-published serve (published artifacts are
            // store-current at publish; a stale one re-materialises
            // through the serve's own currency gates); a fenced serve
            // re-resolves every raw source specifier through the live
            // resolver instead — the same discipline as the
            // augmentation probe's re-export walk.
            let baked_edges_current = serve.store_published;
            for target in facts.shallow_state.import_targets.values() {
                if baked_edges_current && !target.canonical_id.is_empty() {
                    candidates.insert(target.canonical_id.clone());
                    continue;
                }
                if let Some(resolved) =
                    self.resolve_route_type_edge(owner_canonical_id, &target.source_specifier)
                {
                    candidates.insert(resolved);
                }
            }

            for export in facts.shallow_state.exports.values() {
                if let crate::resolver_core::ExportTarget::Reexport {
                    canonical_id,
                    source_specifier,
                    ..
                } = export
                {
                    if baked_edges_current && !canonical_id.is_empty() {
                        candidates.insert(canonical_id.clone());
                    } else if let Some(resolved) =
                        self.resolve_route_type_edge(owner_canonical_id, source_specifier)
                    {
                        candidates.insert(resolved);
                    }
                }
            }

            for wildcard in &facts.shallow_state.wildcard_reexports {
                if baked_edges_current && !wildcard.canonical_id.is_empty() {
                    candidates.insert(wildcard.canonical_id.clone());
                } else if let Some(resolved) =
                    self.resolve_route_type_edge(owner_canonical_id, &wildcard.source_specifier)
                {
                    candidates.insert(resolved);
                }
            }
        }

        for import in &snapshot.imports {
            if let Some(resolved) = import.resolved_canonical_id.as_deref() {
                candidates.insert(resolved.to_string());
                continue;
            }

            if let Some(target) = self.resolve_route_type_edge(owner_canonical_id, &import.source) {
                candidates.insert(target);
                continue;
            }

            if import.source.starts_with('.') {
                candidates
                    .extend(self.expand_relative_candidates(owner_canonical_id, &import.source));
            }
        }

        candidates
    }

    /// View-aware macro-argument-type expansion entry point.
    /// Routes the resolver-tier reads (query engine, dispatch
    /// lowering, prepared-decl bundle) through the supplied
    /// `ResolverContext` so overlay-bearing sessions observe overlay
    /// candidates for cross-file macro-argument-type expansion.
    pub(crate) fn compute_evaluated_types_with_tracking_from_owner_context_with_ctx(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        owner_eval_source: Option<&str>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> Option<ComputedEvaluatedTypes> {
        let eval_source = owner_eval_source.map(str::to_string).or_else(|| {
            self.current_eval_state(canonical)
                .map(|(source, framework_parse, _)| {
                    Self::build_eval_script_source(canonical, &source, framework_parse.as_deref())
                })
        })?;
        self.compute_evaluated_types_from_owner_context_with_ctx(
            ctx,
            canonical,
            snapshot,
            &eval_source,
            purpose,
        )
    }

    /// The `defineExpose` binding-entry NAMES the macro expander emits
    /// `FieldKind::Binding` closure invocations for: the requested binding
    /// names that resolve to a prepared VALUE declaration carrying an
    /// annotation fact (the same has-annotation gate the retired typed-entry
    /// list applied — the closure resolves the binding's TYPE on demand
    /// through the prepared surface, by name).
    fn component_meta_binding_type_entries(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical: &str,
        requested_bindings: &std::collections::BTreeSet<verter_type_expr::DeclKey>,
    ) -> Vec<verter_semantic::analysis::type_eval_build::BindingExpansionEntry> {
        if requested_bindings.is_empty() {
            return Vec::new();
        }

        let Some(indexed) = ctx
            .ensure_indexed_ready_serve(canonical)
            .map(|serve| serve.indexed)
        else {
            return Vec::new();
        };
        let mut admitted = std::collections::BTreeSet::new();
        for demand in requested_bindings {
            let Some(crate::resolver_core::shallow_file_state::LexicalValueBinding::Local(owner)) =
                indexed
                    .shallow_state
                    .visible_value_binding(demand.owner, demand.name.as_ref())
            else {
                continue;
            };
            admitted.insert(verter_type_expr::DeclKey::new(
                owner,
                Arc::clone(&demand.name),
            ));
        }

        admitted
            .iter()
            .filter(|binding| {
                ctx.prepared_value_decl(canonical, binding.owner, binding.name.as_ref())
                    .is_some_and(|decl| {
                        !matches!(
                            decl.type_annotation.classification,
                            verter_type_expr::facts::ValueAnnotationClass::Absent
                        )
                    })
            })
            .map(
                |binding| verter_semantic::analysis::type_eval_build::BindingExpansionEntry {
                    name: binding.name.to_string(),
                    owner: binding.owner,
                },
            )
            .collect()
    }

    /// Macro-argument-type expander entry point. The expander uses
    /// `ctx` for the query-engine and dispatch construction so the
    /// cross-file type lookups observe overlay candidates when the
    /// session view carries them.
    pub(crate) fn compute_evaluated_types_from_owner_context_with_ctx(
        &self,
        ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
        canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        eval_source: &str,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> Option<ComputedEvaluatedTypes> {
        {
            component_meta_trace_custom!(
                "compute_evaluated_types_seed_owner_cache",
                format!("owner={} store_view={}", canonical, false),
            );
            let _ = ctx.ensure_indexed_ready_serve(canonical);
        }
        let requested_bindings =
            if purpose == crate::resolver_core::ComponentMetaResolutionPurpose::Full {
                crate::resolver_core::collect_requested_binding_demands(snapshot.macros.as_ref())
            } else {
                std::collections::BTreeSet::new()
            };
        let binding_entries = {
            component_meta_trace_custom!(
                "compute_evaluated_types_binding_entries",
                format!(
                    "owner={} requested_bindings={} store_view={}",
                    canonical,
                    requested_bindings.len(),
                    false,
                ),
            );
            self.component_meta_binding_type_entries(ctx, canonical, &requested_bindings)
        };
        // the retired `external_engine` branch is
        // gone; there is only one `expand_macro_types` entry point left.
        // Step 9.1 / D32: surface-id sidecar capture buffers. Populated
        // when audit is on; the dispatch round-trip inside the closure
        // gives a SemanticNodeId for the produced expanded type, which
        // is stored in the buffer keyed by FieldKind. After the closure
        // returns, the buffers feed `SurfaceNodeIdentities` so Step
        // 9.2's scoped origin export reverse-walks only the reachable
        // subgraph rooted at these ids.
        let audit_enabled = self.config.audit_enabled;
        let prop_node_ids: std::cell::RefCell<Vec<Option<crate::semantic_query::SemanticNodeId>>> =
            std::cell::RefCell::new(Vec::new());
        let emit_node_ids: std::cell::RefCell<Vec<Option<crate::semantic_query::SemanticNodeId>>> =
            std::cell::RefCell::new(Vec::new());
        let slot_binding_node_ids: std::cell::RefCell<
            Vec<Option<crate::semantic_query::SemanticNodeId>>,
        > = std::cell::RefCell::new(Vec::new());
        let binding_node_ids: std::cell::RefCell<
            Vec<Option<crate::semantic_query::SemanticNodeId>>,
        > = std::cell::RefCell::new(Vec::new());
        let result = {
            component_meta_trace_custom!(
                "compute_evaluated_types_expand_macros",
                format!(
                    "owner={} macros={} bindings={} store_view={}",
                    canonical,
                    snapshot.macros.len(),
                    binding_entries.len(),
                    false,
                ),
            );
            let mut engine = crate::resolver_core::ComponentMetaQueryEngine::new(ctx);
            verter_semantic::analysis::type_eval_build::expand_macro_types_impl_with_expander(
                snapshot.macros.as_ref(),
                Some(eval_source),
                binding_entries.as_slice(),
                None,
                match purpose {
                    crate::resolver_core::ComponentMetaResolutionPurpose::Full => {
                        verter_semantic::analysis::type_eval_build::MacroExpansionScope::Full
                    }
                    crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough => {
                        verter_semantic::analysis::type_eval_build::MacroExpansionScope::Fallthrough
                    }
                },
                |ctx, payload| {
                    use crate::resolver_core::component_meta_query_engine::{
                        FastShallowFieldExpr, FastShallowFieldExprExactness,
                    };
                    use verter_semantic::analysis::type_expand::{
                        ExpandedNormalizedExpr, ExpansionResult,
                    };
                    use verter_type_expr::facts::SemanticTypeSource;
                    use verter_type_expr::locators::AuthoredBodyLocator;

                    // The publication boundary writes the fast carrier's
                    // content-free SOURCE; the session-side node decisions
                    // already ran on its `hot` handle inside the producer.
                    fn fast_to_expansion(
                        fast: FastShallowFieldExpr,
                    ) -> ExpansionResult<ExpandedNormalizedExpr> {
                        match fast.exactness {
                            FastShallowFieldExprExactness::Symbolic => {
                                ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                                    expr: fast.semantic_source,
                                })
                            }
                            FastShallowFieldExprExactness::Concrete => {
                                ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                    expr: fast.semantic_source,
                                })
                            }
                        }
                    }

                    // Capture
                    // the production-path SemanticNodeId for this
                    // field. Each branch that lowers via dispatch
                    // sets this variable to the produced terminal
                    // node id. Branches that do not dispatch (fast
                    // path, shallow-preserve, defineModel-without-
                    // type-arg, etc.) leave it as `None`. The
                    // captured id replaces the retired audit-only
                    // re-lowering at the closure's tail (no
                    // duplicate dispatch round-trip — audit is now a
                    // pure reader of production work).
                    let mut produced_node_id: Option<crate::semantic_query::SemanticNodeId> = None;

                    // The field's content-free AUTHORED source: the macro
                    // payload position when the analyzer stamped one, or —
                    // for a top-level value binding (`FieldKind::Binding`,
                    // no macro payload) — the value declaration's own
                    // authored position. `None` only when neither exists;
                    // the degraded publication is the honest Unknown leaf.
                    let authored_field_source = || -> Option<SemanticTypeSource> {
                        if let Some(payload) = payload {
                            return Some(SemanticTypeSource::Authored(
                                AuthoredBodyLocator::MacroPayload(payload.clone()),
                            ));
                        }
                        if matches!(
                            ctx.kind,
                            verter_semantic::analysis::type_eval_build::FieldKind::Binding
                        ) {
                            use verter_semantic::analysis::type_eval_build::PathSegment as MacroPathSegment;
                            if let [MacroPathSegment::Member(name)] = ctx.output_path.as_ref() {
                                return Some(SemanticTypeSource::Authored(
                                    AuthoredBodyLocator::DeclBody(
                                        verter_type_expr::locators::TypeBodySlot {
                                            anchor: verter_type_expr::locators::AuthoredAnchor {
                                                canonical_id: std::sync::Arc::from(canonical),
                                                owner: ctx.scope_owner,
                                                symbol: std::sync::Arc::clone(name),
                                                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                                            },
                                            path: std::sync::Arc::from(
                                                Vec::new().into_boxed_slice(),
                                            ),
                                        },
                                    ),
                                ));
                            }
                        }
                        None
                    };
                    let unknown_source = || {
                        SemanticTypeSource::Closed(verter_type_expr::facts::ClosedTypeFact::Leaf(
                            verter_type_expr::facts::LeafTypeFact::Primitive(
                                verter_type_expr::PrimitiveName::Unknown,
                            ),
                        ))
                    };

                    // Node-domain fast paths: resolve the field's value node
                    // once (structural mirror member / authored member
                    // position through the one dispatch) and classify it.
                    let fast = match payload {
                        Some(payload) => engine
                            .macro_field_value_node(
                                canonical,
                                ctx.macro_index,
                                ctx.output_path.as_ref(),
                            )
                            .map(|field_value| (payload, field_value)),
                        None => None,
                    }
                    .and_then(|(payload, field_value)| {
                        engine.try_fast_shallow_field_expr(canonical, payload, field_value)
                    });

                    let expansion = if let Some(fast) = fast {
                        fast_to_expansion(fast)
                    } else {
                        // Dispatch-projection branch. Lower the macro's
                        // parent shell once via dispatch (using the
                        // cache-owned parsed_type_argument), then
                        // project the closure's output_path off the
                        // lowered base. On any failure (no
                        // parsed_type_argument, empty output_path,
                        // lowering miss, projection unknown, raise
                        // failed) emit a structured trace event and
                        // fall back to symbolic preservation.
                        use crate::semantic_query::{
                            PathSegment as SemanticPathSegment, ProjectionMode,
                        };
                        use verter_semantic::analysis::type_eval_build::PathSegment as MacroPathSegment;

                        let preserve_authored_symbolically = || {
                            ExpansionResult::exact_symbolic(ExpandedNormalizedExpr {
                                expr: authored_field_source().unwrap_or_else(unknown_source),
                            })
                        };

                        let macro_type_arg = snapshot
                            .macros
                            .get(ctx.macro_index)
                            .and_then(|m| m.parsed_type_argument.clone());
                        let macro_kind = snapshot.macros.get(ctx.macro_index).map(|m| m.kind);

                        // Field-level fast path. When the macro's parent
                        // shell is a named generic / non-generic carrier and
                        // the field's authored value does NOT reference any
                        // of the parent's type parameters (decided in NODE
                        // DOMAIN off the shared graph carriers), the closure
                        // short-circuits to publishing the field's authored
                        // SOURCE as exact-concrete. No parent projection
                        // runs. Skipping the parent lower means we do NOT
                        // dispatch `Instantiate { base = <heritage>, .. }`
                        // for any of the shell's `extends`-chain types,
                        // which is the source of the cold-time blow-up when
                        // the heritage points into a third-party package
                        // (the `defineProps<ChatMessageProps>() extends
                        // UIMessage from 'ai'` regression).
                        //
                        // The `defineModel<T>()` arm BELOW retains its
                        // existing direct-lower path; the fast path applies
                        // only when the slow `output_path` projection branch
                        // would otherwise run.
                        //
                        // The early-exit assigns `expansion` rather than
                        // returning from the closure: the audit-gated
                        // push at the bottom of the closure must still
                        // run to keep per-FieldKind cardinality in
                        // sync with the macro emitter's field count.
                        let fast_path_applied = !ctx.output_path.is_empty()
                            && !matches!(
                                macro_kind,
                                Some(verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
                            )
                            && macro_type_arg.is_some()
                            && !engine.field_needs_parent_projection(
                                canonical,
                                ctx.macro_index,
                                ctx.output_path.as_ref(),
                            );

                        if fast_path_applied {
                            ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                expr: authored_field_source().unwrap_or_else(unknown_source),
                            })
                        } else {
                            // `defineModel<T>()` prop /
                            // model lowering. The `expand_macro_types_impl_with_expander`
                            // emits the model's prop field with
                            // `output_path = [Member(<model_name>)]`, but the
                            // macro's `parsed_type_argument` is `T` itself —
                            // not a parent shell whose member is the type.
                            // Dispatching `ProjectPath { base, [Member(model)],
                            // Expanded }` always misses because `T` is
                            // typically a `Primitive` / `Ref` / `Union` (no
                            // member to navigate).
                            //
                            // routes `DefineModel` prop / model
                            // fields through a direct lower+raise of
                            // `macro_type_arg` (the type IS the field's
                            // type), bypassing the path projection. Mirrors
                            // the empty-output_path arm semantically. Closes
                            // `fixture_models` deferred fixture (re-homed
                            // from 5k per §5.13 r15 table).
                            if matches!(
                                macro_kind,
                                Some(verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
                            ) {
                                if macro_type_arg.is_some() {
                                    // Sink-owned demand: the model value type IS
                                    // the field's type, so the sink resolves the
                                    // macro-argument carrier head at `Expanded`
                                    // INTERNALLY and materialises at the sealed
                                    // sink. The eval_env branch passes ONLY the
                                    // closed demand (resolver ctx + owner
                                    // canonical + macro index) — never a raw node.
                                    use crate::host_manage::component_meta_methods::DefineModelOutputExpansion;
                                    // The model's fallback SOURCE is its own T —
                                    // the macro type-argument payload position.
                                    let model_fallback = macro_type_arg
                                        .as_ref()
                                        .map(|locator| {
                                            SemanticTypeSource::Authored(
                                                AuthoredBodyLocator::MacroPayload(locator.clone()),
                                            )
                                        })
                                        .or_else(authored_field_source)
                                        .unwrap_or_else(unknown_source);
                                    match crate::host_manage::component_meta_methods::expand_define_model_output(
                                        engine.ctx(),
                                        canonical,
                                        ctx.macro_index,
                                        &model_fallback,
                                    ) {
                                        DefineModelOutputExpansion::Materialized {
                                            produced_node_id: id,
                                            normalized,
                                        } => {
                                            // Capture production node id for the
                                            // audit record (the carrier head).
                                            produced_node_id = Some(id);
                                            ExpansionResult::exact_concrete(normalized)
                                        }
                                        DefineModelOutputExpansion::RaiseMiss {
                                            produced_node_id: id,
                                        } => {
                                            // Lowering succeeded but the resolved
                                            // root is unmaterialisable — fall back
                                            // to the model's authored source (its
                                            // own T). `produced_node_id` is still
                                            // captured for audit parity.
                                            produced_node_id = Some(id);
                                            ExpansionResult::exact_concrete(
                                                ExpandedNormalizedExpr {
                                                    expr: model_fallback.clone(),
                                                },
                                            )
                                        }
                                        DefineModelOutputExpansion::CarrierMiss => {
                                            // Lowering miss — fall back to the
                                            // model's authored source.
                                            ExpansionResult::exact_concrete(
                                                ExpandedNormalizedExpr {
                                                    expr: model_fallback.clone(),
                                                },
                                            )
                                        }
                                    }
                                } else {
                                    // No `parsed_type_argument` — publish the
                                    // field's own authored source (the macro's
                                    // `prop_fields[0]` payload per
                                    // `extract_define_model_type` IS the
                                    // macro's first type argument).
                                    ExpansionResult::exact_concrete(ExpandedNormalizedExpr {
                                        expr: authored_field_source()
                                            .unwrap_or_else(unknown_source),
                                    })
                                }
                            } else {
                                match (ctx.output_path.is_empty(), macro_type_arg.as_ref()) {
                                    (true, _) | (_, None) => {
                                        component_meta_trace_custom!(
                                    "macro_projection_failover",
                                    format!(
                                        "macro_index={} field_kind={:?} reason=no_parsed_type_argument",
                                        ctx.macro_index, ctx.kind,
                                    ),
                                );
                                        preserve_authored_symbolically()
                                    }
                                    (false, Some(_macro_type_arg)) => {
                                        // Issue #3 — selective carrier-mode demotion.
                                        // Path-precise contract (`/type-resolution`):
                                        // when the carrier is a named `Ref` (e.g.
                                        // `defineProps<UIMessage>()` or
                                        // `defineProps<ChatMessageProps>()`), the
                                        // shell is an intermediate hop on the way
                                        // to the field — the field itself is the
                                        // terminal hop. Lower the carrier in
                                        // `Navigate` mode so the shell expands
                                        // only as much as navigation needs; the
                                        // terminal `ProjectPath` query inside the
                                        // sink runs in `Expanded` and owns the full
                                        // expansion of the requested member.
                                        //
                                        // For compound carriers (anonymous object
                                        // literals, conditionals, mapped types,
                                        // intersections, etc.) the field's parsed
                                        // body may reference parent-generic params
                                        // and depend on slow-path `Expanded`
                                        // resolution to instantiate the body
                                        // correctly — keep `Expanded` for those.
                                        // This carrier-mode decision is a pure
                                        // node-domain predicate over the macro
                                        // hot mirror's root carrier (parens are
                                        // structurally transparent there),
                                        // computed here and passed to the sink
                                        // as a closed scalar.
                                        let carrier_lower_mode = {
                                            let root_is_reference_carrier =
                                                crate::structural_carrier_producer::macro_type_arg_hot_ref(
                                                    engine.ctx(),
                                                    canonical,
                                                    ctx.macro_index,
                                                )
                                                .and_then(|handle| {
                                                    crate::project_semantic_dispatch::node_data_for(
                                                        engine.ctx(),
                                                        handle.node(),
                                                    )
                                                })
                                                .is_some_and(|data| {
                                                    data.bare_ref_head().is_some()
                                                        || matches!(
                                                            data.as_ref(),
                                                            crate::semantic_query::SemanticNodeData::DeclRef { .. }
                                                                | crate::semantic_query::SemanticNodeData::InstantiationRef { .. }
                                                        )
                                                });
                                            if root_is_reference_carrier {
                                                ProjectionMode::Navigate
                                            } else {
                                                ProjectionMode::Expanded
                                            }
                                        };
                                        use crate::host_manage::component_meta_methods::MacroPathOutputExpansion;
                                        // slot-binding-parameter type lowering goes
                                        // through dispatch via the
                                        // `ResolveMacroPayload` variant +
                                        // `MaterializeSurface { Slots }` codepath:
                                        // the sink composes existing variants to
                                        // descend `Function` -> `params[0].ty` ->
                                        // `Member(binding)` (the slot value's
                                        // bindings live inside the function's
                                        // first-parameter Object, not as a direct
                                        // member of the Function).
                                        if matches!(
                                            ctx.kind,
                                            verter_semantic::analysis::type_eval_build::FieldKind::SlotBinding
                                        ) {
                                            // SlotBinding output_path always has
                                            // exactly two segments per
                                            // `type_eval_build.rs` SlotBinding
                                            // emission: [Member(slot),
                                            // Member(binding)]. Anything else is
                                            // a closure-emission contract
                                            // violation; fall back to symbolic.
                                            // The path-shape destructure is the
                                            // closure-emission contract check; it
                                            // stays here and only the destructured
                                            // names cross into the sink.
                                            let mut iter = ctx.output_path.iter();
                                            match (iter.next(), iter.next(), iter.next()) {
                                                (
                                                    Some(MacroPathSegment::Member(slot)),
                                                    Some(MacroPathSegment::Member(binding)),
                                                    None,
                                                ) => {
                                                    // Sink-owned demand: the sink
                                                    // lowers the carrier head at
                                                    // `carrier_lower_mode`, descends
                                                    // the slot-binding terminal at
                                                    // `Expanded`, and materialises at
                                                    // the sealed sink. Only closed
                                                    // inputs cross (resolver ctx +
                                                    // owner canonical + macro index +
                                                    // carrier mode + slot/binding
                                                    // names) — never a raw node.
                                                    let field_fallback =
                                                        authored_field_source()
                                                            .unwrap_or_else(unknown_source);
                                                    match crate::host_manage::component_meta_methods::expand_slot_binding_output(
                                                        engine.ctx(),
                                                        canonical,
                                                        ctx.macro_index,
                                                        carrier_lower_mode,
                                                        slot.as_ref(),
                                                        binding.as_ref(),
                                                        &field_fallback,
                                                    ) {
                                                        MacroPathOutputExpansion::Materialized {
                                                            produced_node_id: id,
                                                            normalized,
                                                        } => {
                                                            produced_node_id = Some(id);
                                                            ExpansionResult::exact_concrete(
                                                                normalized,
                                                            )
                                                        }
                                                        MacroPathOutputExpansion::RaiseMiss {
                                                            produced_node_id: id,
                                                        } => {
                                                            produced_node_id = Some(id);
                                                            component_meta_trace_custom!(
                                                                "macro_projection_failover",
                                                                format!(
                                                                    "macro_index={} field_kind={:?} reason=slot_binding_raise_miss",
                                                                    ctx.macro_index, ctx.kind,
                                                                ),
                                                            );
                                                            preserve_authored_symbolically()
                                                        }
                                                        MacroPathOutputExpansion::ProjectionMiss => {
                                                            component_meta_trace_custom!(
                                                                "macro_projection_failover",
                                                                format!(
                                                                    "macro_index={} field_kind={:?} reason=slot_binding_projection_miss",
                                                                    ctx.macro_index, ctx.kind,
                                                                ),
                                                            );
                                                            preserve_authored_symbolically()
                                                        }
                                                        MacroPathOutputExpansion::CarrierMiss => {
                                                            component_meta_trace_custom!(
                                                                "macro_projection_failover",
                                                                format!(
                                                                    "macro_index={} field_kind={:?} reason=opaque_scope_or_uninterpretable",
                                                                    ctx.macro_index, ctx.kind,
                                                                ),
                                                            );
                                                            preserve_authored_symbolically()
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    // Dead emitter-contract safety
                                                    // net: the SlotBinding emitter
                                                    // (`type_eval_build.rs`) ALWAYS
                                                    // emits exactly
                                                    // `[Member(slot), Member(binding)]`,
                                                    // so this arm is unreachable on
                                                    // real emitter output — the
                                                    // path-shape check runs before
                                                    // the sink's carrier-lower (vs
                                                    // after, in the former inline
                                                    // path) only on input the emitter
                                                    // cannot produce, so the trace
                                                    // ordering is unobservable.
                                                    component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=slot_binding_unexpected_path",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                    preserve_authored_symbolically()
                                                }
                                            }
                                        } else {
                                            // Convert the macro output_path to the
                                            // semantic path segments the terminal
                                            // `ProjectPath` demand needs (member
                                            // names only — a closed demand input,
                                            // not a node).
                                            let dispatch_path: std::sync::Arc<[SemanticPathSegment]> =
                                                std::sync::Arc::from(
                                                    ctx.output_path
                                                        .iter()
                                                        .map(|seg| match seg {
                                                            MacroPathSegment::Member(name) => {
                                                                SemanticPathSegment::Member(
                                                                    std::sync::Arc::clone(name),
                                                                )
                                                            }
                                                        })
                                                        .collect::<Vec<_>>(),
                                                );
                                            // Sink-owned demand: the sink lowers the
                                            // carrier head at `carrier_lower_mode`,
                                            // projects the terminal path at
                                            // `Expanded`, and materialises at the
                                            // sealed sink. Only closed inputs cross
                                            // (resolver ctx + owner canonical + macro
                                            // index + carrier mode + the member path)
                                            // — never a raw node.
                                            let field_fallback = authored_field_source()
                                                .unwrap_or_else(unknown_source);
                                            match crate::host_manage::component_meta_methods::expand_generic_project_path_output(
                                                engine.ctx(),
                                                canonical,
                                                ctx.macro_index,
                                                carrier_lower_mode,
                                                dispatch_path,
                                                &field_fallback,
                                            ) {
                                                MacroPathOutputExpansion::Materialized {
                                                    produced_node_id: id,
                                                    normalized,
                                                } => {
                                                    produced_node_id = Some(id);
                                                    ExpansionResult::exact_concrete(normalized)
                                                }
                                                MacroPathOutputExpansion::RaiseMiss {
                                                    produced_node_id: id,
                                                } => {
                                                    produced_node_id = Some(id);
                                                    component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=raise_failed",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                    preserve_authored_symbolically()
                                                }
                                                MacroPathOutputExpansion::ProjectionMiss => {
                                                    component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=projection_unknown",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                    preserve_authored_symbolically()
                                                }
                                                MacroPathOutputExpansion::CarrierMiss => {
                                                    component_meta_trace_custom!(
                                                        "macro_projection_failover",
                                                        format!(
                                                            "macro_index={} field_kind={:?} reason=opaque_scope_or_uninterpretable",
                                                            ctx.macro_index, ctx.kind,
                                                        ),
                                                    );
                                                    preserve_authored_symbolically()
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    };

                    // The
                    // audit-gated re-lowering sidecar is RETIRED.
                    // `produced_node_id` was captured directly off
                    // each production dispatch branch above (or left
                    // as `None` for fast-path / symbolic / failed-
                    // raise branches that legitimately have no
                    // semantic node to publish). The buffer push is
                    // unconditional so audit-on/off perform IDENTICAL
                    // semantic work — the only audit-side cost is a
                    // `Vec::push(Option<SemanticNodeId>)` per field,
                    // which is microseconds. The
                    // `SurfaceNodeIdentities` assembly below remains
                    // audit-gated (it materialises the per-FieldKind
                    // vectors into the audit record only when audit
                    // is on).
                    let target = match ctx.kind {
                        verter_semantic::analysis::type_eval_build::FieldKind::Prop => {
                            &prop_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::Emit => {
                            &emit_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::SlotBinding => {
                            &slot_binding_node_ids
                        }
                        verter_semantic::analysis::type_eval_build::FieldKind::Binding => {
                            &binding_node_ids
                        }
                    };
                    target.borrow_mut().push(produced_node_id);

                    expansion
                },
            )
        };
        // Dependency tracking comes from the frontier/shallow-file-state path.
        let discovered_dependencies = std::collections::BTreeSet::<String>::new();
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "compute_evaluated_types owner={} props={} define_props={} emits={} slot_bindings={} bindings={}",
                canonical,
                result.props.len(),
                result.define_props.len(),
                result.emits.len(),
                result.slot_bindings.len(),
                result.bindings.len(),
            ));
        }
        // Step 9.1: assemble SurfaceNodeIdentities from the audit-gated
        // capture buffers. Length-equality with the corresponding output
        // vectors is guaranteed by the closure being called once per
        // FieldKind-tagged field in the same order
        // expand_macro_types_impl_with_expander pushes into props/emits/
        // slot_bindings/bindings.
        let surface_identities =
            if audit_enabled {
                let prop_ids = prop_node_ids.into_inner();
                let emit_ids = emit_node_ids.into_inner();
                let slot_binding_ids = slot_binding_node_ids.into_inner();
                let binding_ids = binding_node_ids.into_inner();
                // Sanity invariant — debug panic in tests, fall back to None
                // in release if the closure-call cardinality somehow differs.
                let lengths_match = prop_ids.len() == result.props.len()
                    && emit_ids.len() == result.emits.len()
                    && slot_binding_ids.len() == result.slot_bindings.len()
                    && binding_ids.len() == result.bindings.len();
                if lengths_match {
                    Some(crate::meta_resolve::SurfaceNodeIdentities {
                        prop_node_ids: prop_ids,
                        emit_node_ids: emit_ids,
                        slot_binding_node_ids: slot_binding_ids,
                        binding_node_ids: binding_ids,
                        registry_node_ids: Vec::new(),
                    })
                } else {
                    debug_assert!(
                    lengths_match,
                    "surface_identities length mismatch — closure-call cardinality drifted from \
                     ExpandedComponentTypes vector lengths. props {}/{}, emits {}/{}, \
                     slot_bindings {}/{}, bindings {}/{}.",
                    prop_ids.len(), result.props.len(),
                    emit_ids.len(), result.emits.len(),
                    slot_binding_ids.len(), result.slot_bindings.len(),
                    binding_ids.len(), result.bindings.len(),
                );
                    None
                }
            } else {
                None
            };
        Some(ComputedEvaluatedTypes {
            evaluated_types: (!Self::is_expanded_types_empty(&result)).then_some(result),
            discovered_dependencies,
            surface_identities,
        })
    }
}

#[cfg(test)]
#[path = "eval_env_tests.rs"]
mod eval_env_tests;
