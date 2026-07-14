//! Output-materialization capability fence: the sealed reverse boundary
//! that turns a graph [`SemanticNodeId`] back into a [`TypeExpr`] for the
//! true OUTPUT/PUBLICATION sinks ONLY.
//!
//! Reverse materialization (`SemanticNodeId -> TypeExpr`) is a laundering
//! surface: hot/session code that raises a node to a [`TypeExpr`] and then
//! makes semantic decisions on it bypasses the single graph-native
//! resolver. This module makes the callable reverse boundary an
//! UNFORGEABLE CAPABILITY, and — for the carrier-UNWRAP half — puts the
//! raised [`TypeExpr`] in a structurally-unreachable PAYLOAD VAULT:
//!
//! - [`OutputProjector`] is a SEALED capability trait (it can only be
//!   implemented by the sanctioned output-sink capability types, through
//!   explicit `impl` pairs in the private `projector` module that names the
//!   private `projector::sealed::Sealed` marker). Its two boundary methods —
//!   [`OutputProjector::materialize_output_type_expr`] (plain shell raise)
//!   and [`OutputProjector::materialize_reduced_output_type_expr`]
//!   (reduce-then-raise) — hand back SEALED CARRIERS, never a bare
//!   [`TypeExpr`].
//! - [`OutputTypeExpr`] / [`MaterializedOutputTypeExpr`] are sealed
//!   carriers whose inner [`TypeExpr`] lives in a deeply-private nested
//!   `carrier::payload` vault. The `TypeExpr` is reachable by field access
//!   ONLY from inside that `payload` module — so in safe Rust OUTSIDE the
//!   vault there is NO readable `TypeExpr` field to return, and a
//!   capability-free unwrap is UNREPRESENTABLE by field access, auto-deref,
//!   an arbitrary trait impl (e.g. `Deref`/`Index` returning `&TypeExpr`),
//!   or an inherent method. The ONLY production APIs that yield the inner
//!   [`TypeExpr`] are the capability-gated accessors
//!   ([`OutputTypeExpr::into_type_expr`] /
//!   [`MaterializedOutputTypeExpr::into_type_expr`] /
//!   [`MaterializedOutputTypeExpr::type_expr`]), each of which takes a
//!   `&impl OutputProjector`.
//! - Each true output-SINK module (the exact module that projects) owns a tiny
//!   private-field capability type whose constructor is PRIVATE to that SINK
//!   (`mint: pub(in <sink-module>)`), NOT to the whole subtree. The sink
//!   modules are: `meta_resolve::projectors::output_sink` (a DEDICATED terminal
//!   submodule — extracted exactly so the parent `projectors`' NON-sink helpers
//!   (`macro_payload_substrate`, `published_reducer`, `define_shapes`, the
//!   per-kind projector children) cannot mint),
//!   `meta_resolve::materialize::field_types`,
//!   `host_manage::component_meta_methods`, `typeinfo::raise`,
//!   `typeinfo::framework_surface::svelte_exec`,
//!   `typeinfo::framework_surface::vue_exec` (whose whole reachable scope —
//!   `vue_exec` + its `normalize` normalizer child — IS output-only, so the
//!   single cap is correct), and
//!   `component_meta_query_engine::{registry_decl,surface}`. The capability
//!   type is `pub(crate)` so the owner `projector` module can name it for the
//!   sealed-trait impl (the projectors cap is re-exported at
//!   `meta_resolve::projectors::MetaResolveProjectorsOutputCap` for that
//!   naming), but it is UNFORGEABLE outside its sink (private constructor +
//!   private dispatch field). `pub(in P)` grants the mint to `P` AND every
//!   module at-or-under `P`, so the mint scope is scoped to a TERMINAL output
//!   sink whose entire reachable production module tree is itself output-only:
//!   a Kind-B bridge sibling (`dispatch_helpers`, `eval_env`) — or a non-sink
//!   helper that shares the subtree — is NOT reachable from any sink's mint
//!   scope, so it cannot name any cap's constructor (a planted mint is
//!   `E0624`). The `output_cap_mint_scope_is_per_leaf_not_subtree` guard models
//!   this with a Rust-visibility reachable-module-tree check (default-deny on
//!   any non-sink module reachable from a mint scope).
//!
//! Net effect: in safe production Rust OUTSIDE the audited payload vault,
//! [`OutputTypeExpr`] and [`MaterializedOutputTypeExpr`] do not expose a
//! readable [`TypeExpr`] field — capability-free unwrap is unrepresentable
//! by field access, auto-deref, arbitrary trait impls, or inherent methods,
//! and the only production APIs returning [`TypeExpr`] / `&TypeExpr` are the
//! capability-gated `into_type_expr` / `type_expr` accessors. The PRIMARY
//! barriers are COMPILER-ENFORCED: the payload vault makes a capability-free
//! unwrap unrepresentable in safe Rust, the private `projector::sealed` marker
//! makes a non-owner `OutputProjector` impl `E0603`, and the terminal-sink
//! `mint: pub(in <sink>)` scope makes a non-sink mint `E0624`. The residual
//! TRUSTED surface — the inline payload vault + the projector registration
//! source — is the part the COMPILER cannot itself pin (the identity of which
//! owner-named types are sinks, and edits inside the trusted file). Over that
//! BOUNDED surface, the `output_projector_residual_guards` `syn` checks are
//! DEFENSE-IN-DEPTH, shaped as a CLOSED structural allowlist (exact module
//! topology; the exact `impl OutputProjector` / `impl sealed::Sealed` multiset
//! by full self-type path; a cap-gated-or-test signature allowlist for every
//! `TypeExpr`-returning fn; bans on item/impl/trait-position macro invocations,
//! `include!`, unknown attributes, a `sealed::Sealed` alias, and any owner
//! `TypeExpr` alias), backed by the `output_materialization_guards`
//! accidental-regression canary. The defense-in-depth claim does NOT cover
//! guard deletion or unsafe code (unless the crate forbids unsafe globally).
//! Hot / session / Kind-B code can neither construct an [`OutputProjector`]
//! capability (no constructible capability type is reachable from a non-sink
//! module — a planted hot mint is `E0624`/`E0451`) nor unwrap a sealed
//! carrier (the accessor requires a capability instance, the trait is sealed
//! so a hot module cannot implement it for one of its own types, AND the
//! inner `TypeExpr` is not even a readable field outside the vault). The
//! carrier `_for_test` accessors are gated
//! `#[cfg(any(test, feature = "test-support"))]` (the production-unreachable
//! test-support feature), so they too are COMPILE-ABSENT from every
//! production build — a planted hot `.type_expr_for_test()` is `E0599`.
//!
//! The raw shell raise primitive `raise_node_to_type_expr` stays
//! MODULE-PRIVATE to [`super::raise`]. The capability's boundary methods reach
//! the raise side through the `pub(super)` seams that return SEALED carriers,
//! never a bare `TypeExpr`: the shell raise via
//! [`ProjectSemanticDispatch::output_shell_raise_sealed`] (returns
//! `Option<OutputTypeExpr>`), and the reduce path via
//! [`ProjectSemanticDispatch::raise_and_reduce_with_context`] (returns the
//! sealed [`MaterializedOutputTypeExpr`]) directly. A `project_semantic_dispatch`
//! sibling can REACH these `pub(super)` seams but cannot unwrap the returned
//! carrier without a capability.
//!
//! The former Kind-B raise-then-decide sites (which once raised a node to a
//! bare `TypeExpr` mid-flight to make a semantic decision) are RETIRED: every
//! Kind-B caller now decides on the node-domain `RaisedShapeFacts` / interned
//! `RaisedShapeKey` (no mid-flight raise), and the single publication `TypeExpr`
//! is materialised ONCE at a registered output sink through this capability —
//! the demand-bound surface adapters
//! ([`super::super::resolver_core::component_meta_query_engine::surface`]) and
//! the sink-owned macro-output expansion demand methods
//! (`host_manage::component_meta_methods::expand_define_model_output` /
//! `expand_generic_project_path_output` / `expand_slot_binding_output`), which
//! resolve a closed semantic demand internally and materialise the produced node
//! at the module-private sealed sink.

use verter_type_expr::TypeExpr;

use super::ProjectSemanticDispatch;
use crate::semantic_query::{DepSignature, ProjectionReductionContext, SemanticNodeId};

// =====================================================================
// The carrier names are RE-EXPORTED so every consumer (the Kind-A sinks,
// the FFI facade, the raise-side seams, the guard module) keeps naming
// them at the SAME paths — `output_materialization::{OutputProjector,
// OutputTypeExpr, MaterializedOutputTypeExpr}` — with ZERO churn at call
// sites. The projector seal lives in the private `projector` module; the
// carriers and their TypeExpr payload vault live in the sibling private
// `carrier` module. `carrier`/`carrier::payload` may NAME `OutputProjector`
// (for the accessor capability bound) but CANNOT name the PRIVATE
// `projector::sealed::Sealed` marker (private `mod sealed`, not `pub(super)`),
// so a carrier-side `impl projector::sealed::Sealed for HotCap` is `E0603`
// (module `sealed` is private) — the carrier modules can never become a
// replacement owner-descendant scope that launders a sealed impl. The
// `output_projector_owner_registration_inventory` topology guard is the
// defense-in-depth backstop.
// =====================================================================
pub(crate) use carrier::{MaterializedOutputTypeExpr, OutputTypeExpr};
pub(crate) use projector::OutputProjector;

/// The projector seal: the sealed [`OutputProjector`] capability trait, the
/// private `sealed::Sealed` marker it is sealed against, and the explicit
/// `impl` pairs that register the sanctioned output-sink capability types.
///
/// Kept in a private module SEPARATE from the carrier payload vault so that
/// the only scope able to name `sealed::Sealed` — and therefore the only
/// scope able to implement [`OutputProjector`] — is this `projector` module
/// itself. Because `mod sealed` is PRIVATE (not `pub(super)`), the sibling
/// `carrier` module (and its `payload` vault) cannot name `projector::sealed`
/// at all: a carrier-side `impl projector::sealed::Sealed for HotCap` is a
/// COMPILE error (`E0603`, module `sealed` is private), so the seal is
/// compiler-enforced — the topology guard is a defense-in-depth backstop.
mod projector {
    use super::{
        MaterializedOutputTypeExpr, OutputTypeExpr, ProjectSemanticDispatch,
        ProjectionReductionContext, SemanticNodeId,
    };

    /// Seals [`OutputProjector`] against external implementations: only the
    /// capability types this `projector` module names (and impls the marker
    /// for) can implement the capability trait. A hot module — or a sibling
    /// `carrier`/`payload` module — cannot add a new capability type and
    /// implement [`OutputProjector`] for it, because it cannot name (let
    /// alone implement) this private marker.
    ///
    /// The module is PRIVATE (no visibility modifier), so `sealed::Sealed` is
    /// nameable ONLY from within this `projector` module. A sibling
    /// `carrier`/`payload` module — or any other module in the crate — that
    /// writes `projector::sealed::Sealed` gets `E0603` (module `sealed` is
    /// private): the seal is COMPILER-enforced, not merely guard-backstopped.
    /// (A `pub(super)` here would have leaked the marker to the parent
    /// `output_materialization` module and ALL its descendants, including the
    /// sibling `carrier` module, letting a hand-written carrier-side
    /// `impl projector::sealed::Sealed for HotCap` launder a sealed impl.)
    mod sealed {
        /// Marker trait [`super::OutputProjector`] is sealed against.
        pub trait Sealed {}
    }

    /// The sealed reverse-materialization capability.
    ///
    /// Implemented ONLY for the true-output-sink capability types registered
    /// (via the explicit `impl` pairs below) in this module. The two boundary
    /// methods are the SOLE callable `SemanticNodeId -> TypeExpr` output seam;
    /// they return sealed carriers, never a bare [`TypeExpr`]. Hold a
    /// capability local to an output subtree, materialize the node into a
    /// sealed carrier, then unwrap the carrier through that same capability to
    /// obtain the [`TypeExpr`] the sink publishes.
    pub(crate) trait OutputProjector: sealed::Sealed {
        /// The dispatch this capability projects through.
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_>;

        /// Plain SHELL raise (no operator reduction): materialize `node` into
        /// a sealed [`OutputTypeExpr`] carrier. `None` is the miss signal (the
        /// node — or a node required while raising it — is unavailable from
        /// the live graph store).
        fn materialize_output_type_expr(&self, node: SemanticNodeId) -> Option<OutputTypeExpr> {
            self.dispatch().output_shell_raise_sealed(node)
        }

        /// REDUCE-then-raise: apply the supplied projection reduction context,
        /// then materialize the reduced node into a sealed
        /// [`MaterializedOutputTypeExpr`] (the producing reduced `node_id`, the
        /// sealed `type_expr` payload, the accumulated `dep_signature`, and the
        /// `result_is_partial` flag).
        fn materialize_reduced_output_type_expr(
            &self,
            node: SemanticNodeId,
            context: ProjectionReductionContext,
        ) -> MaterializedOutputTypeExpr {
            self.dispatch().raise_and_reduce_with_context(node, context)
        }
    }

    // =====================================================================
    // PER-SINK output-sink capability registration — EXPLICIT impl pairs.
    //
    // Each capability TYPE is defined (via the sink-side
    // `define_output_capability!` macro) in the EXACT output-SINK module that
    // legitimately projects, with a `mint` visibility scoped to that sink —
    // NOT the whole subtree. (Where a subtree's parent owns non-sink helper
    // children, the cap lives in a DEDICATED terminal sink submodule — e.g.
    // `meta_resolve::projectors::output_sink` — so those helpers cannot mint.)
    // Here, in the owner `projector` module, each cap is sealed
    // (`impl sealed::Sealed`) and implements [`OutputProjector`] EXPLICITLY (NOT
    // through a macro): explicit source items are scannable, so the
    // `output_projector_owner_registration_inventory` module-topology guard can
    // pin the sanctioned sink set by reading the actual `impl OutputProjector
    // for <Cap>` items (by FULL self-type path, as a multiset) rather than
    // trusting an opaque macro body.
    //
    // A hot/session/Kind-B module can NAME a capability type (it is
    // `pub(crate)`) but can neither call its private `new()` (E0624) nor
    // struct-literal-construct it (private field, E0451), so it cannot obtain an
    // [`OutputProjector`]. Because the constructor is scoped to a TERMINAL output
    // SINK whose entire reachable production module tree is itself output-only,
    // a Kind-B bridge sibling that shares the SUBTREE
    // (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink
    // helper sibling — is NOT reachable from any sink's `pub(in P)` mint scope
    // and ALSO cannot mint, closing the in-subtree convention hole. Combined
    // with the sealed trait (a hot module cannot implement [`OutputProjector`]
    // for one of its own types), the COMPILER enforces both mint-locality and
    // carrier-unwrap-locality in EVERY build profile.
    //
    // The cap TYPES are named through their re-export paths where the owning
    // sink module is private (`field_types`, `registry_decl`, `surface`, and
    // the projectors `output_sink` submodule): the parent module re-exports
    // ONLY the `pub(crate)` cap type, NOT the private sink module or its `new()`
    // constructor. So the owner names the type while the constructor stays
    // sink-private. The `output_cap_mint_scope_is_per_leaf_not_subtree` guard
    // pins each cap's `mint:` scope to a TERMINAL sink whose entire reachable
    // production module tree is output-only (a Rust-visibility reachable-tree
    // model, default-deny on any reachable non-sink module).
    // =====================================================================

    impl sealed::Sealed for crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap<'_, '_> {}
    impl OutputProjector for crate::meta_resolve::projectors::MetaResolveProjectorsOutputCap<'_, '_> {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed for crate::meta_resolve::materialize::MetaResolveFieldTypesOutputCap<'_, '_> {}
    impl OutputProjector for crate::meta_resolve::materialize::MetaResolveFieldTypesOutputCap<'_, '_> {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed for crate::typeinfo::raise::TypeinfoRaiseOutputCap<'_, '_> {}
    impl OutputProjector for crate::typeinfo::raise::TypeinfoRaiseOutputCap<'_, '_> {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed
        for crate::typeinfo::framework_surface::svelte_exec::TypeinfoSvelteSurfaceOutputCap<'_, '_>
    {
    }
    impl OutputProjector
        for crate::typeinfo::framework_surface::svelte_exec::TypeinfoSvelteSurfaceOutputCap<'_, '_>
    {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed
        for crate::typeinfo::framework_surface::vue_exec::TypeinfoVueSurfaceOutputCap<'_, '_>
    {
    }
    impl OutputProjector
        for crate::typeinfo::framework_surface::vue_exec::TypeinfoVueSurfaceOutputCap<'_, '_>
    {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed
        for crate::resolver_core::component_meta_query_engine::MetaQueryRegistryOutputCap<'_, '_>
    {
    }
    impl OutputProjector
        for crate::resolver_core::component_meta_query_engine::MetaQueryRegistryOutputCap<'_, '_>
    {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    impl sealed::Sealed
        for crate::resolver_core::component_meta_query_engine::MetaQuerySurfaceOutputCap<'_, '_>
    {
    }
    impl OutputProjector
        for crate::resolver_core::component_meta_query_engine::MetaQuerySurfaceOutputCap<'_, '_>
    {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch_for_projector()
        }
    }

    // =====================================================================
    // Test-only output capability.
    //
    // The carrier round-trip / reduce / projector-peek test suites drive the
    // boundary methods directly and assert on the raised `TypeExpr`. They are
    // not Kind-A subtrees, so they cannot mint a production capability. This
    // `#[cfg(test)]`-gated capability lets them obtain an `OutputProjector`
    // without holding a real sink's capability. It exists ONLY in test builds
    // — the shipped release binary contains no `TestOutputCap`, so it is NOT a
    // production reverse-materialization path and the structural fence-shape
    // inventory excludes it (it scans non-`#[cfg(test)]` production source).
    // =====================================================================

    /// Test-only `OutputProjector` capability. `#[cfg(test)]`-gated. See the
    /// note above for why it is not a fence hole.
    #[cfg(test)]
    pub(crate) struct TestOutputCap<'disp, 'ctx> {
        dispatch: &'disp ProjectSemanticDispatch<'ctx>,
    }

    #[cfg(test)]
    impl<'disp, 'ctx> TestOutputCap<'disp, 'ctx> {
        /// Mint the test capability over `dispatch`.
        pub(crate) fn new(dispatch: &'disp ProjectSemanticDispatch<'ctx>) -> Self {
            Self { dispatch }
        }
    }

    #[cfg(test)]
    impl sealed::Sealed for TestOutputCap<'_, '_> {}
    #[cfg(test)]
    impl OutputProjector for TestOutputCap<'_, '_> {
        fn dispatch(&self) -> &ProjectSemanticDispatch<'_> {
            self.dispatch
        }
    }
}

#[cfg(test)]
pub(crate) use projector::TestOutputCap;

/// The carriers and their structurally-unreachable [`TypeExpr`] payload
/// vault.
///
/// The inner [`TypeExpr`] is stored in the deeply-private nested `payload`
/// module ([`payload::OutputPayload`]); it is reachable by field access ONLY
/// from inside `payload`. The carrier types here ([`OutputTypeExpr`],
/// [`MaterializedOutputTypeExpr`]) hold the vault and forward the
/// capability-gated reads to the vault's `pub(super)` accessors. This module
/// may NAME [`OutputProjector`] (for the accessor capability bound) but
/// CANNOT name `projector::sealed::Sealed` (it is private to `projector`), so
/// a carrier-side `impl projector::sealed::Sealed for HotCap` is `E0603` and
/// this module can never become a scope that launders a sealed
/// [`OutputProjector`] impl.
mod carrier {
    use super::{DepSignature, OutputProjector, SemanticNodeId, TypeExpr};

    /// The PAYLOAD VAULT: the inner [`TypeExpr`] lives here and is reachable
    /// by field access ONLY from within this module.
    ///
    /// Every read of the inner [`TypeExpr`] outside `payload` must go through
    /// one of the `pub(super)` capability-gated accessors below — so in safe
    /// Rust there is NO readable [`TypeExpr`] field reachable from the parent
    /// `carrier` module, the grandparent `output_materialization` module, or
    /// anywhere else in the crate. Auto-deref, an arbitrary trait impl
    /// (`Deref` / `Index` / `AsRef` returning `&TypeExpr`), or an inherent
    /// method returning the inner `TypeExpr` is therefore UNREPRESENTABLE
    /// outside this vault: there is no field to borrow or move out. The
    /// capability bound (`P: OutputProjector`) on the read accessors is the
    /// unwrap-locality proof; the capability instance is not otherwise
    /// consulted.
    mod payload {
        use super::{OutputProjector, TypeExpr};

        /// The sealed inner-`TypeExpr` payload. The single field is private
        /// to this `payload` module — NOT `pub`, NOT `pub(super)`, NOT
        /// `pub(crate)`.
        pub(super) struct OutputPayload(TypeExpr);

        impl OutputPayload {
            /// Seal a raw [`TypeExpr`] into the vault. `pub(super)` — only the
            /// parent `carrier` module's carrier constructors reach it.
            pub(super) fn new(type_expr: TypeExpr) -> Self {
                Self(type_expr)
            }

            /// Structurally clone the payload (clones the inner [`TypeExpr`]).
            /// NOT an unwrap — no [`TypeExpr`] escapes the vault, no capability
            /// required; used by the carrier's [`Clone`] impl.
            pub(super) fn clone_payload(&self) -> Self {
                Self(self.0.clone())
            }

            /// Read the inner [`TypeExpr`] out, consuming the payload. Requires
            /// an [`OutputProjector`] capability — the unwrap-locality gate.
            pub(super) fn into_type_expr<P: OutputProjector + ?Sized>(self, _cap: &P) -> TypeExpr {
                self.0
            }

            /// Test-only borrow of the inner [`TypeExpr`] (no capability). The
            /// `pub(super)` visibility keeps it inside `carrier`; the carrier
            /// re-exposes it ONLY through the `#[cfg(any(test, feature =
            /// "test-support"))]`-gated carrier accessor, so this is
            /// COMPILE-ABSENT from production builds.
            #[cfg(any(test, feature = "test-support"))]
            pub(super) fn type_expr_for_test(&self) -> &TypeExpr {
                &self.0
            }
        }
    }

    /// Sealed carrier for a plain-raise output [`TypeExpr`].
    ///
    /// The inner [`TypeExpr`] is locked in the [`payload::OutputPayload`]
    /// vault: there is NO readable `TypeExpr` field, NO `pub` `Deref` /
    /// `AsRef<TypeExpr>` / `into_inner` / pub field. The only way to read it
    /// out is [`Self::into_type_expr`], which requires an [`OutputProjector`]
    /// capability — so a hot module cannot unwrap it (it cannot construct any
    /// capability, the capability trait is sealed against its own types, and
    /// the inner `TypeExpr` is not even a reachable field outside the vault).
    pub(crate) struct OutputTypeExpr(payload::OutputPayload);

    impl OutputTypeExpr {
        /// Seal a raw [`TypeExpr`] into the carrier from the raise side
        /// (`crate::project_semantic_dispatch::raise`).
        /// `pub(in crate::project_semantic_dispatch)` — visible to the raise
        /// module (a sibling of `output_materialization` within
        /// `project_semantic_dispatch`) and to the capability-gated
        /// [`super::wrap_output_type_expr`] minting helper, so the
        /// reduce-then-raise orchestrator and the shell-raise delegator
        /// construct the carrier here; out-of-subsystem code (hot / session /
        /// Kind-B, outside `project_semantic_dispatch`) cannot reach this
        /// constructor and must go through a capability or a boundary method.
        /// This mirrors the pre-vault visibility (the carrier formerly lived
        /// directly in `output_materialization`, where `pub(super)` resolved to
        /// `project_semantic_dispatch`); the vault moved the carrier one level
        /// deeper, so the same reach is now spelled explicitly.
        pub(in crate::project_semantic_dispatch) fn from_raise(type_expr: TypeExpr) -> Self {
            Self(payload::OutputPayload::new(type_expr))
        }

        /// Read the inner [`TypeExpr`] out, consuming the carrier. Requires an
        /// [`OutputProjector`] capability — the compiler-enforced
        /// unwrap-locality gate. Delegates to the vault's capability-gated
        /// accessor; the capability argument is the proof the caller is a true
        /// output sink; it is not otherwise consulted.
        pub(crate) fn into_type_expr<P: OutputProjector + ?Sized>(self, cap: &P) -> TypeExpr {
            self.0.into_type_expr(cap)
        }
    }

    /// Sealed carrier for a reduce-then-raise output result — the
    /// publication-surface output contract the per-member projectors consume.
    ///
    /// The `type_expr` payload is a PRIVATE inner sealed [`OutputTypeExpr`]
    /// (whose own payload lives in the [`payload`] vault; capability-gated
    /// unwrap). The METADATA fields (`node_id`, `dep_signature`,
    /// `result_is_partial`) are facts-rail signatures, NOT the laundering
    /// surface, so they stay readable by the Kind-A sinks through public
    /// accessors.
    pub(crate) struct MaterializedOutputTypeExpr {
        /// The producing reduced [`SemanticNodeId`] (facts-rail metadata).
        node_id: Option<SemanticNodeId>,
        /// The sealed raised payload (capability-gated unwrap).
        type_expr: OutputTypeExpr,
        /// Accumulated dependency signature (facts-rail metadata).
        dep_signature: DepSignature,
        /// `true` when any contributing dispatch read returned a PARTIAL value
        /// (projection-budget exhaustion, cancellation, same-path recursion,
        /// or a walker fatal/pathological diagnostic). Consumers that publish
        /// the materialized result into a downstream shared cache must
        /// propagate this bit so the admission gate refuses to warm a partial.
        result_is_partial: bool,
    }

    impl MaterializedOutputTypeExpr {
        /// Assemble a [`MaterializedOutputTypeExpr`] from its parts. The
        /// `type_expr` payload is an already-sealed [`OutputTypeExpr`]; this is
        /// the constructor the raise-side reducer and the Kind-A re-assembly
        /// sites (which already hold a sealed payload) use.
        pub(crate) fn from_parts(
            node_id: Option<SemanticNodeId>,
            type_expr: OutputTypeExpr,
            dep_signature: DepSignature,
            result_is_partial: bool,
        ) -> Self {
            Self {
                node_id,
                type_expr,
                dep_signature,
                result_is_partial,
            }
        }

        /// The producing reduced [`SemanticNodeId`] (facts-rail metadata —
        /// always readable, NOT the laundering surface). Part of the carrier's
        /// documented readable-metadata contract, read in production by the
        /// publication pipeline off the reduced-output carrier: the no-poison
        /// root-sentinel gate in `reduce_field_type_expr_with_mode` and the
        /// node-domain shape comparison in `reduce_published_field_types` read
        /// node facts off this id instead of re-materialising a `TypeExpr`.
        pub(crate) fn node_id(&self) -> Option<SemanticNodeId> {
            self.node_id
        }

        /// The accumulated dependency signature (facts-rail metadata — always
        /// readable, NOT the laundering surface).
        pub(crate) fn dep_signature(&self) -> &DepSignature {
            &self.dep_signature
        }

        /// Replace the dependency signature (facts-rail metadata). Used by the
        /// cold-compute admit path to fold the gate fence into the entry's
        /// signature.
        pub(crate) fn set_dep_signature(&mut self, dep_signature: DepSignature) {
            self.dep_signature = dep_signature;
        }

        /// Whether any contributing read returned a PARTIAL value (facts-rail
        /// metadata — always readable, NOT the laundering surface).
        pub(crate) fn result_is_partial(&self) -> bool {
            self.result_is_partial
        }

        /// Borrow the inner [`TypeExpr`] payload. Requires an
        /// [`OutputProjector`] capability — the compiler-enforced
        /// unwrap-locality gate for the borrowing read sites. Delegates to the
        /// vault's capability-gated accessor. The carrier's documented borrow
        /// read-surface, paired with the live by-value [`Self::into_type_expr`]:
        /// the per-field sentinel gate now reads the node-domain root-sentinel
        /// fact off the carrier `node_id` instead of borrowing the `TypeExpr`, so
        /// the borrow accessor (like the sibling `node_id` accessor) has no
        /// current caller but stays as the carrier's read contract.
        /// Test-only carrier assembly from a raw [`TypeExpr`] (no capability
        /// required). Gated `#[cfg(any(test, feature = "test-support"))]` — NOT
        /// `#[cfg(any(test, debug_assertions))]` — so it is reachable ONLY from
        /// genuine test code (the in-crate `#[cfg(test)]` suites AND, via the
        /// production-unreachable `test-support` feature, the separate
        /// integration-test binary's `ShapeCacheDb` synthetic-carrier proof
        /// helpers), and is COMPILE-ABSENT from a plain debug `cargo build` /
        /// `pnpm run build:lsp` / `pnpm dev-extension` / any release build.
        /// `debug_assertions` is ON in the dev cargo profile, so a
        /// `debug_assertions`-OR gate would expose a capability-free carrier
        /// constructor in ordinary debug builds — the exact reverse-materialization
        /// laundering the fence forbids. `test-support` is absent from `default`
        /// and is activated ONLY by `verter_session`'s `[dev-dependencies]`
        /// self-edge (test / example / bench targets), so this gate makes a planted
        /// hot `MaterializedOutputTypeExpr::from_type_expr_for_test(..)` in a
        /// non-test module a COMPILE error in EVERY build profile (debug AND
        /// release). The fence-shape inventory allows EXACTLY `#[cfg(test)]` /
        /// `#[cfg(any(test, feature = "test-support"))]` as the sanctioned test-only
        /// carrier-accessor gate and BANS `debug_assertions`, so a future
        /// re-widening is caught. Used by the cache + dispatch test harnesses that
        /// drive a `MaterializedOutputTypeExpr` from a synthetic `TypeExpr`.
        #[cfg(any(test, feature = "test-support"))]
        pub(crate) fn from_type_expr_for_test(
            node_id: Option<SemanticNodeId>,
            type_expr: TypeExpr,
            dep_signature: DepSignature,
            result_is_partial: bool,
        ) -> Self {
            Self {
                node_id,
                type_expr: OutputTypeExpr(payload::OutputPayload::new(type_expr)),
                dep_signature,
                result_is_partial,
            }
        }

        /// Test-only borrow of the inner [`TypeExpr`] payload (no capability
        /// required). Gated `#[cfg(any(test, feature = "test-support"))]` — see
        /// [`Self::from_type_expr_for_test`] for why `debug_assertions` is excluded
        /// (it would re-open the carrier-unwrap hole in ordinary debug builds) and
        /// why the production-unreachable `test-support` feature is the sanctioned
        /// way to reach this accessor from the separate integration-test binary.
        /// Delegates to the vault's test-only accessor.
        #[cfg(any(test, feature = "test-support"))]
        pub(crate) fn type_expr_for_test(&self) -> &TypeExpr {
            self.type_expr.0.type_expr_for_test()
        }
    }

    impl Clone for MaterializedOutputTypeExpr {
        fn clone(&self) -> Self {
            Self {
                node_id: self.node_id,
                // The sealed payload clones its private inner `TypeExpr` inside
                // the vault; this is structural cloning of the carrier, NOT an
                // unwrap (no capability required, no `TypeExpr` escapes the
                // vault).
                type_expr: OutputTypeExpr(self.type_expr.0.clone_payload()),
                dep_signature: self.dep_signature.clone(),
                result_is_partial: self.result_is_partial,
            }
        }
    }
}

// =====================================================================
// Capability minting — the sole owner-side entry that a Kind-A subtree's
// capability constructor delegates to.
//
// `wrap_output_type_expr` lets a Kind-A subtree that holds a RAW
// `TypeExpr` (e.g. the `admit_type_expr_shape_if_possible` /
// projectors literal-construct sites, which build a `MaterializedOutputTypeExpr`
// from a freshly-computed `TypeExpr`) seal it into an `OutputTypeExpr`.
// It requires an `OutputProjector` capability, so only a true output
// sink can mint a sealed payload from a bare `TypeExpr`. It seals through
// the carrier's `pub(super)` `from_raise` constructor (the same constructor
// the raise side uses) — the capability check happens here, at the mint
// boundary, before sealing.
// =====================================================================

/// Seal a raw [`TypeExpr`] into an [`OutputTypeExpr`] carrier. Requires an
/// [`OutputProjector`] capability — a hot module cannot reach this (it
/// cannot construct a capability). Used by the Kind-A sites that assemble
/// a [`MaterializedOutputTypeExpr`] from a freshly-computed [`TypeExpr`]
/// rather than from a boundary-method carrier.
pub(crate) fn wrap_output_type_expr<P: OutputProjector + ?Sized>(
    _cap: &P,
    type_expr: TypeExpr,
) -> OutputTypeExpr {
    OutputTypeExpr::from_raise(type_expr)
}

// =====================================================================
// The output-sink capability TYPES are defined in their output-SINK modules.
//
// Each capability is a private-field marker (it holds a PRIVATE
// `&ProjectSemanticDispatch`) whose constructor is PRIVATE to its owning
// output-SINK module — the exact module that projects, NOT the whole
// subtree (and, where a parent owns non-sink helper children, a DEDICATED
// terminal sink submodule so those helpers cannot mint). A sink DEFINES its
// capability with the [`define_output_capability!`] macro, which generates (in
// the sink): the `pub(crate)` capability struct holding a PRIVATE
// `&ProjectSemanticDispatch` field, a `new()` constructor PRIVATE to the sink
// (`mint: pub(in <sink-module>)`), and a `pub(crate)`
// `dispatch_for_projector()` accessor the owner `projector` module reads
// through. The owner `projector` module then seals + implements
// `OutputProjector` for the named capability type via the explicit `impl`
// pairs above.
//
// A hot/session/Kind-B module can NAME a capability type (it is
// `pub(crate)`) but can neither call its private `new()` (E0624) nor
// struct-literal-construct it (private field, E0451), so it cannot obtain
// an `OutputProjector`. Because the constructor is scoped to a TERMINAL output
// SINK whose entire reachable production module tree is itself output-only, a
// Kind-B bridge sibling that shares the SUBTREE
// (`meta_resolve::dispatch_helpers`, `host_manage::eval_env`) — or a non-sink
// helper sibling — is NOT reachable from any sink's mint scope and ALSO cannot
// mint, closing the in-subtree convention hole. Combined with the sealed trait
// (a hot module cannot implement `OutputProjector` for one of its own types),
// the COMPILER enforces both mint-locality and carrier-unwrap-locality in EVERY
// build profile.
// =====================================================================

/// Define an output-sink capability type in an output-SINK module.
///
/// Generates a `pub(crate)` private-field capability struct that borrows a
/// [`ProjectSemanticDispatch`], with:
/// - a `new()` constructor visible ONLY within the SINK module
///   (`$mint_vis`, e.g. `pub(in crate::meta_resolve::projectors::output_sink)`)
///   — so only that sink (and its output-only reachable scope) can obtain the
///   capability, and NO other module (hot / session / Kind-B, INCLUDING a
///   Kind-B bridge sibling that shares the subtree, or a non-sink helper
///   sibling) can mint it;
/// - a `pub(crate) dispatch_for_projector()` accessor the owner `projector`
///   module's `OutputProjector` impl reads through;
/// - a private field — so no module can struct-literal-construct it.
///
/// `$mint_vis` is the sink's own visibility path; expanding the macro with a
/// wider visibility, or in an unrelated module, would still not grant a
/// cross-sink capability because the [`OutputProjector`] + sealed-marker impls
/// (in the owner `projector` module) are keyed by the EXACT capability type
/// registered through the explicit `impl` pairs. The
/// `output_projector_owner_registration_inventory` module-topology guard pins
/// the registered capability set + the carrier/vault shape, and the
/// `output_cap_mint_scope_is_per_leaf_not_subtree` guard pins each cap's mint
/// scope to a terminal-sink reachable-module tree.
macro_rules! define_output_capability {
    ($(#[$meta:meta])* $vis:vis struct $name:ident; mint: $mint_vis:vis) => {
        $(#[$meta])*
        $vis struct $name<'disp, 'ctx> {
            dispatch: &'disp $crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx>,
        }

        impl<'disp, 'ctx> $name<'disp, 'ctx> {
            /// Mint the capability. Visible ONLY within this output-SINK module
            /// (`$mint_vis`) — a hot / session / non-sink module (INCLUDING a
            /// Kind-B bridge sibling that shares the subtree, or a non-sink
            /// helper sibling not reachable from the sink's mint scope) cannot
            /// call it, so it cannot obtain this output capability.
            $mint_vis fn new(
                dispatch: &'disp $crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx>,
            ) -> Self {
                Self { dispatch }
            }

            /// The dispatch this capability projects through. Read by the
            /// owner `projector` module's `OutputProjector` impl. Named
            /// distinctly from the trait's `dispatch` method so the impl can
            /// delegate to it without ambiguity (a same-named inherent + trait
            /// method would recurse).
            pub(crate) fn dispatch_for_projector(
                &self,
            ) -> &$crate::project_semantic_dispatch::ProjectSemanticDispatch<'ctx> {
                self.dispatch
            }
        }
    };
}
pub(crate) use define_output_capability;
