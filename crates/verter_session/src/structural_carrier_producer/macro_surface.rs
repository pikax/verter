//! The macro hot mirror — the single-entry producer of a Vue SFC MACRO
//! type-argument's mode-NEUTRAL semantic-graph handle.
//!
//! ## What it is
//!
//! The hot mirror is the SINGLE-ENTRY producer of a macro's type argument
//! ([`AnalyzedMacro.parsed_type_argument`](verter_semantic::analysis::AnalyzedMacro))
//! graph node. The eager, CONTEXT-SHAPED (per the caller's
//! [`ProjectionMode`](crate::semantic_query::ProjectionMode)) per-site lowering
//! it replaced produced a one-demand-only reduction — not a storable, shared,
//! mode-neutral handle. The four production macro-arg sites
//! (`meta_resolve::slot_binding_graph`, `meta_resolve::projectors`,
//! `host_manage::eval_env`, and `typeinfo::framework_surface::vue_exec`) now
//! READ this mirror handle instead of lowering the macro arg themselves.
//!
//! The hot mirror produces exactly such a handle. On first demand per
//! `macro_index` it lowers the macro's `parsed_type_argument` ONCE through
//! the shared query-free structural lowerer (the witness-gated
//! [`super::lower::emit_macro_arg`]) into a [`HotTypeRef`] — an interned
//! [`SemanticNodeId`] carrying the unresolved `BareRef` / `ImportType` /
//! operator-shell carriers, NO resolution. Every production site that needs a
//! macro type-argument graph node reads THIS handle and re-enters the ONE
//! shared dispatch (`SemanticQueryKey` → `ProjectSemanticDispatch::execute`)
//! at its own demand / mode (Navigate, a `ProjectPath` for an indexed-access
//! or per-field path, a Shallow surface). Different TERMINAL demands are fine;
//! a second BASE producer of the macro arg's graph node is not — that is the
//! forbidden callsite-scattered structural-vs-eager dual path.
//!
//! ## Single-entry producer / witness-gated boundary
//!
//! The raw structural lowerer is PRIVATE to [`super::lower`]; the macro
//! surface reaches it ONLY through the witnessed [`super::lower::emit_macro_arg`]
//! wrapper, presenting the [`MacroProducerWitness`] capability proof. The sole
//! production macro-arg ENTRY is [`macro_type_arg_hot_ref`]. The binder-seed
//! constraint/default lowering inside
//! [`script_setup_binder::build_script_setup_seed_frames`] is INTERNAL to the
//! mirror builder — it lowers generic-binder constraint / default exprs while
//! building the same handle's scope, NOT a second macro-arg producer. No
//! production module OUTSIDE [`crate::structural_carrier_producer`] can forge a
//! witness, so a second production producer is UNREPRESENTABLE by construction
//! — the single-engine producer rule is compiler-enforced, not source-scanned.
//!
//! ## Laziness / content addressing / singleflight
//!
//! [`MacroHotMirror`] is a FILE-ARTIFACT child stored adjacent to the
//! macros + lazy [`DeclBodyMemo`](crate::decl_body_memo::DeclBodyMemo) on
//! [`IndexedReady`](crate::project_type_store::IndexedReady) — it mirrors the
//! memo shape. Its identity is the owning artifact's `(canonical,
//! whole_hash)` plus the `macro_index`; a content edit publishes a fresh
//! `IndexedReady` carrying a fresh empty mirror, so a superseded mirror can
//! never answer a new-content demand. Publishing an artifact lowers ZERO
//! macro mirrors (the cell table is unallocated until first demand). The
//! mirror is a lazy DENSE table: an outer [`OnceLock`] lazily allocates a
//! per-macro-count cell table ONCE on first demand (race-safe via
//! `get_or_init`); each per-slot [`OnceLock`] (indexed by `macro_index`) is
//! the singleflight unit — its `get_or_init` collapses concurrent first-touch
//! of one macro onto one lowering, and waiters block cooperatively on the
//! cell. Two threads racing the TABLE allocation also singleflight on the
//! outer cell.
//!
//! ## Script-setup generic seeding
//!
//! A `<script setup generic="T">` parameter must lower to its
//! [`SemanticNodeData::TypeParam`] binder, NOT a `BareRef(T)`. The
//! structural lowerer does not consult the host's script-setup bindings; it
//! only does a syntactic in-scope binder lookup. So the builder pre-builds a
//! SEED [`BinderScope`] frame by re-sourcing the `<script setup generic="…">`
//! clause from the owner's ROUTE-FREE local [`IndexedReady`] data
//! (`raw_source` + `framework_parse`, through `sfc_script_setup_type_params`)
//! — interning a `TypeParam` node matching the eager path's `<script-setup>`
//! decl sentinel + ordinal + lowered constraint / default shape — and passes
//! it to the lowerer, so `lookup_binder("T")` returns the binder node. This
//! is owner-local shallow scope data (NOT the prepared-decl bundle, whose
//! cold path can route-resolve imports), keeping the producer PURE. The seed
//! frame is built incrementally so an earlier binder is visible to a later
//! one's constraint / default (TS scoping).

#[path = "script_setup_binder.rs"]
pub(in crate::structural_carrier_producer) mod script_setup_binder;

use std::sync::{Arc, OnceLock};

use crate::resolver_core::ResolverContext;
use crate::semantic_query::{HotTypeRef, NodeScopeId};

use super::lower::{self, StructuralLowerContext};

/// Compile-time capability proof that the macro hot mirror is invoking the
/// shared structural lowerer through its sanctioned macro-surface entry
/// ([`super::lower::emit_macro_arg`]).
///
/// The field is PRIVATE and the constructor is confined to this surface, so
/// no other module — not even a sibling under
/// [`crate::structural_carrier_producer`] — can forge one. The wrapper in
/// [`super::lower`] can NAME the type (it is module-visible) but cannot
/// construct it, and a foreign module can do neither. A would-be second
/// structural-carrier producer therefore cannot present this witness and
/// cannot reach the lowerer — the single-producer rule is enforced by the
/// type system, not a scanner.
pub(in crate::structural_carrier_producer) struct MacroProducerWitness {
    _private: (),
}

impl MacroProducerWitness {
    /// Mint the macro-surface capability proof. Private to this surface
    /// (`macro_surface`): the builder path constructs it, INCLUDING the
    /// binder-seed helper (`script_setup_binder`), which reaches the lowerer
    /// through the witnessed [`super::lower::emit_macro_arg`] entry as part of
    /// building a macro handle's scope.
    fn new() -> Self {
        Self { _private: () }
    }
}

/// Lazy, singleflight, content-addressed mirror of one file's Vue SFC MACRO
/// type-argument graph handles.
///
/// See the module documentation. Stored on
/// [`IndexedReady`](crate::project_type_store::IndexedReady); content-
/// addressed by construction (a fresh artifact carries a fresh empty
/// mirror).
#[derive(Default)]
pub struct MacroHotMirror {
    /// Lazily allocated once on first demand, sized to the owner's macro
    /// count. `cells[macro_index]` is a per-slot [`OnceLock`]: `Some(HotTypeRef)`
    /// = lowered, `None` = stable negative (no `parsed_type_argument` / not
    /// structurally lowerable). The outer [`OnceLock`] stays EMPTY until the
    /// first `macro_type_arg_hot_ref` demand, so publishing an artifact
    /// allocates ZERO.
    cells: OnceLock<Box<[OnceLock<Option<HotTypeRef>>]>>,
}

impl std::fmt::Debug for MacroHotMirror {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MacroHotMirror")
            .field(
                "demanded",
                &self
                    .cells
                    .get()
                    .map(|c| c.iter().filter(|x| x.get().is_some()).count())
                    .unwrap_or(0),
            )
            .finish()
    }
}

impl Clone for MacroHotMirror {
    /// A cloned artifact starts with an EMPTY mirror: the `HotTypeRef`
    /// handles are interned ids valid for the project graph, but the mirror
    /// is a per-artifact demand cache and a clone is a distinct artifact
    /// instance. Re-demand repopulates it (the underlying interned nodes are
    /// content-addressed, so a re-lower hits the same node ids).
    fn clone(&self) -> Self {
        Self {
            cells: OnceLock::new(),
        }
    }
}

impl MacroHotMirror {
    /// Number of demanded (filled) macro cells — test observability only,
    /// never a validity signal. A freshly published artifact reports `0`.
    #[cfg(test)]
    pub(crate) fn demanded_count(&self) -> usize {
        self.cells
            .get()
            .map(|c| c.iter().filter(|cell| cell.get().is_some()).count())
            .unwrap_or(0)
    }
}

/// Resolve (lowering once on first demand) the mode-NEUTRAL
/// [`HotTypeRef`] for the macro at `macro_index` in `owner_canonical`.
///
/// This is the SOLE production entry that lowers a macro
/// `parsed_type_argument` into a semantic-graph handle. Returns `None` when
/// the owner file is not loaded, the macro index is out of range, the macro
/// carries no `parsed_type_argument`, or the type argument has no faithful
/// unresolved structural representation (a stable negative cell).
pub(crate) fn macro_type_arg_hot_ref(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    macro_index: usize,
) -> Option<HotTypeRef> {
    let serve = ctx.ensure_indexed_ready_serve(owner_canonical)?;
    let indexed = serve.indexed;

    // Lazily allocate the dense cell table once, sized to the owner's macro
    // count (race-safe via the outer `OnceLock::get_or_init`). An
    // out-of-range `macro_index` returns `None` (same negative as a missing
    // macro), never grows the table.
    let table = indexed.macro_hot_mirror.cells.get_or_init(|| {
        let n = indexed
            .script_analysis
            .as_ref()
            .map(|s| s.macros.len())
            .unwrap_or(0);
        (0..n)
            .map(|_| OnceLock::new())
            .collect::<Vec<_>>()
            .into_boxed_slice()
    });
    let cell = table.get(macro_index)?;

    // The mirror is a PURE producer of the UNRESOLVED structural carrier graph
    // (inert carrier nodes, resolved on demand at the consuming dispatch):
    // no host route lookup, no dependency emission. Dependency recording
    // belongs at the RESOLVING demand — the consumer re-enters the ONE
    // dispatch over this handle and the subquery read signatures (`TypeOf`
    // import-route facts, `ResolveDecl`/`Instantiate` file whole-hashes)
    // bubble into the consuming result's `ReadSetSignature`.
    *cell.get_or_init(|| build_macro_hot_ref(ctx, owner_canonical, &indexed, macro_index))
}

/// Build the structural [`HotTypeRef`] for one macro index — the
/// `get_or_init` body. Lowers the macro's `parsed_type_argument` once
/// through the shared query-free structural lowerer under a script-setup
/// seed binder frame.
fn build_macro_hot_ref(
    ctx: &dyn ResolverContext,
    owner_canonical: &str,
    indexed: &crate::project_type_store::IndexedReady,
    macro_index: usize,
) -> Option<HotTypeRef> {
    let snapshot = indexed.script_analysis.as_ref()?;
    let mac = snapshot.macros.get(macro_index)?;
    let parsed_arg = mac.parsed_type_argument.as_ref()?;

    let graph = ctx.project_type_store().semantic_graph();
    let scope = NodeScopeId::File {
        canonical_id: Arc::from(owner_canonical),
        whole_hash: indexed.whole_hash,
        local_scope: None,
    };

    // Macro-T own-body provenance: `defineProps` / `withDefaults` own-body
    // direct members carry `declared_in_macro_type_arg = true` (a props-axis
    // concern consumed by the published surface policy). Every other macro is
    // structural. This mirrors `macro_payload_surface_provenance` —
    // PROVENANCE is a structural-lowering property of the macro's own body,
    // not a demand/mode property, so it belongs on the mode-neutral mirror.
    use verter_semantic::analysis::AnalyzedMacroKind;
    let macro_own_body = matches!(
        mac.kind,
        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
    );

    // Seed the script-setup generic binders so `defineProps<T>()`'s `T` in a
    // `<script setup generic="T">` SFC lowers to its `TypeParam` binder, not
    // a `BareRef(T)`. Built from the owner's ROUTE-FREE local `IndexedReady`
    // data (`raw_source` + `framework_parse`) — NO host route lookup, so the
    // mirror stays a pure producer.
    let seed_frames = script_setup_binder::build_script_setup_seed_frames(indexed, graph, &scope);
    let lower_ctx = StructuralLowerContext::new(&seed_frames).with_macro_own_body(macro_own_body);

    lower::emit_macro_arg(
        graph,
        parsed_arg,
        scope,
        &lower_ctx,
        &MacroProducerWitness::new(),
    )
    .ok()
}

#[cfg(test)]
#[path = "macro_hot_mirror_tests.rs"]
mod macro_hot_mirror_tests;
