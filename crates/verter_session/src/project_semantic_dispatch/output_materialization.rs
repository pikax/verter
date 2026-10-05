//! Output-materialization fence: the sealed reverse boundary that turns a
//! graph [`SemanticNodeId`] back into a [`TypeExpr`] for the true
//! OUTPUT/PUBLICATION sinks ONLY.
//!
//! Reverse materialization (`SemanticNodeId -> TypeExpr`) is a laundering
//! surface: code that raises a node to a [`TypeExpr`] and then makes semantic
//! decisions on it bypasses the single graph-native resolver. Two compiler-held
//! barriers close it:
//!
//! - **The authority.** Materializing a node and unwrapping a sealed carrier
//!   both require a borrowed
//!   [`OutputAuthority`](super::engine_resources::OutputAuthority). The
//!   authority is minted ONCE per engine, by
//!   [`EngineStores::create`](super::engine_resources::EngineStores::create),
//!   alongside — and separately from — the stores a request binding clones.
//!   It has a private field, no public constructor, and is neither `Clone`,
//!   `Copy` nor `Default`; nothing reachable from query access (the engine
//!   binding, the dispatch, the request ports) produces or recovers it, and
//!   the constructor accepts no store handle, so recovering a live engine's
//!   graph cannot remint authority for it. Materializing through a dispatch
//!   over a different engine panics.
//! - **The payload vault.** [`OutputTypeExpr`] / [`MaterializedOutputTypeExpr`]
//!   keep the raised [`TypeExpr`] in a deeply-private nested
//!   `carrier::payload` module, reachable by field access ONLY from inside
//!   it. Outside the vault there is no readable `TypeExpr` field, so a
//!   capability-free unwrap is unrepresentable by field access, auto-deref, an
//!   arbitrary trait impl or an inherent method. The only production APIs that
//!   yield the inner `TypeExpr` take `&OutputAuthority`.
//!
//! The trust boundary is explicit: the host's composition code (the code that
//! constructs the engine stores and stores the authority) is trusted, together
//! with the terminal output sinks it lends the authority to. Which modules are
//! sinks is the host's decision, held there by locally sealed capability
//! wrappers; the engine names none of them. The claim
//! does NOT cover unsafe code (unless the crate forbids unsafe globally).
//!
//! The raw shell raise primitive `raise_node_to_type_expr` stays
//! MODULE-PRIVATE to [`super::raise`]. The authority reaches the raise side
//! through the `pub(super)` seams that return SEALED carriers, never a bare
//! `TypeExpr`: the shell raise via
//! [`ProjectSemanticDispatch::output_shell_raise_sealed`] (returns
//! `Option<OutputTypeExpr>`), and the reduce path via
//! [`ProjectSemanticDispatch::raise_and_reduce_with_context`] (returns the
//! sealed [`MaterializedOutputTypeExpr`]). A `project_semantic_dispatch`
//! sibling can REACH these seams but cannot unwrap the returned carrier
//! without the authority.
//!
//! The carrier `_for_test` accessors are gated
//! `#[cfg(any(test, feature = "test-support"))]`. `test-support` is not a
//! default feature: per-package artifact builds (the LSP, napi and wasm
//! packages) omit it. A whole-workspace build does compile it in — the
//! compile-contract variant crate depends on it as a normal dependency — so
//! the gate keeps these accessors out of shipped artifacts, not out of every
//! workspace build. A carrier assembled through them is stamped with no
//! engine and unwraps under no authority.
//!
//! [`ProjectSemanticDispatch::output_shell_raise_sealed`]: super::ProjectSemanticDispatch
//! [`ProjectSemanticDispatch::raise_and_reduce_with_context`]: super::ProjectSemanticDispatch

use verter_type_expr::TypeExpr;

use super::engine_resources::{EngineIdentity, OutputAuthority};
use crate::semantic_query::{DepSignature, SemanticNodeId};

pub(crate) use carrier::{MaterializedOutputTypeExpr, OutputTypeExpr};

/// The carriers and their structurally-unreachable [`TypeExpr`] payload
/// vault.
///
/// The inner [`TypeExpr`] is stored in the deeply-private nested `payload`
/// module ([`payload::OutputPayload`]); it is reachable by field access ONLY
/// from inside `payload`. The carrier types here ([`OutputTypeExpr`],
/// [`MaterializedOutputTypeExpr`]) hold the vault and forward the
/// authority-gated reads to the vault's `pub(super)` accessors.
mod carrier {
    use super::{DepSignature, EngineIdentity, OutputAuthority, SemanticNodeId, TypeExpr};
    use crate::project_semantic_dispatch::raise::{DegradedLeaf, MaterializedTypeExpr};

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
    /// borrowed `&OutputAuthority` on the read accessors is the
    /// unwrap-locality proof; the authority is not otherwise consulted.
    mod payload {
        use super::{
            DegradedLeaf, EngineIdentity, MaterializedTypeExpr, OutputAuthority, TypeExpr,
        };

        /// The sealed inner-`TypeExpr` payload. BOTH fields are private
        /// to this `payload` module — NOT `pub`, NOT `pub(super)`, NOT
        /// `pub(crate)`. The `degraded_leaves` sidecar rides beside the
        /// compat tree: it is folded into `result_is_partial` at the
        /// `from_parts` choke point and is discarded ONLY at the
        /// capability-gated terminal unwrap.
        pub(super) struct OutputPayload {
            /// The engine that produced this payload; every unwrap checks it
            /// against the authority.
            engine: EngineIdentity,
            type_expr: TypeExpr,
            degraded_leaves: Vec<DegradedLeaf>,
        }

        impl OutputPayload {
            /// Seal a [`MaterializedTypeExpr`] (tree + sidecar) into the
            /// vault. `pub(super)` — only the parent `carrier` module's
            /// carrier constructors reach it.
            pub(super) fn new(engine: EngineIdentity, materialized: MaterializedTypeExpr) -> Self {
                let (type_expr, degraded_leaves) = materialized.into_parts();
                Self {
                    engine,
                    type_expr,
                    degraded_leaves,
                }
            }

            /// `true` when the payload carries any degradation leaf.
            pub(super) fn has_degradation(&self) -> bool {
                !self.degraded_leaves.is_empty()
            }

            /// Structurally clone the payload (clones BOTH the inner
            /// [`TypeExpr`] and the degradation sidecar). NOT an unwrap — no
            /// [`TypeExpr`] escapes the vault, no capability required; used
            /// by the carrier's [`Clone`] impl.
            pub(super) fn clone_payload(&self) -> Self {
                Self {
                    engine: self.engine.clone(),
                    type_expr: self.type_expr.clone(),
                    degraded_leaves: self.degraded_leaves.clone(),
                }
            }

            /// Read the inner [`TypeExpr`] out, consuming the payload. Requires
            /// an [`OutputAuthority`] — the unwrap-locality gate.
            /// This is the ONLY place the degradation sidecar may be
            /// discarded (the capability-gated TERMINAL unwrap) — and it is
            /// discarded ONLY AFTER being observed into the NON-CACHEABLE
            /// read channel (`OutputMaterializationLoss`): the result is
            /// never warm-admitted as complete, and the observation never
            /// faults the enclosing compute's completeness (a contained
            /// member-level degradation stays entry-local).
            pub(super) fn into_type_expr(self, authority: &OutputAuthority) -> TypeExpr {
                // The payload opens only for the authority of the engine that
                // produced it.
                authority.verify_carrier(&self.engine);
                if !self.degraded_leaves.is_empty() {
                    crate::fact_tracing::note_non_cacheable_read_fan_out(
                        verter_session_query::facts::reuse::NonCacheableReadReason::OutputMaterializationLoss,
                    );
                }
                self.type_expr
            }

            /// Test-only borrow of the inner [`TypeExpr`] (no capability). The
            /// `pub(super)` visibility keeps it inside `carrier`; the carrier
            /// re-exposes it ONLY through the `#[cfg(any(test, feature =
            /// "test-support"))]`-gated carrier accessor, so this is
            /// COMPILE-ABSENT from production builds.
            #[cfg(any(test, feature = "test-support"))]
            pub(super) fn type_expr_for_test(&self) -> &TypeExpr {
                &self.type_expr
            }
        }
    }

    /// Sealed carrier for a plain-raise output [`TypeExpr`].
    ///
    /// The inner [`TypeExpr`] is locked in the [`payload::OutputPayload`]
    /// vault: there is NO readable `TypeExpr` field, NO `pub` `Deref` /
    /// `AsRef<TypeExpr>` / `into_inner` / pub field. The only way to read it
    /// out is [`Self::into_type_expr`], which requires the [`OutputAuthority`]
    /// — so code holding only query access cannot unwrap it (it can neither
    /// construct nor recover the authority, and the inner `TypeExpr` is not
    /// even a reachable field outside the vault).
    pub(crate) struct OutputTypeExpr(payload::OutputPayload);

    impl OutputTypeExpr {
        /// Seal a raw [`TypeExpr`] into the carrier from the raise side
        /// (`crate::project_semantic_dispatch::raise`).
        /// `pub(in crate::project_semantic_dispatch)` — visible to the raise
        /// module (a sibling of `output_materialization` within
        /// `project_semantic_dispatch`) and to the authority-gated
        /// [`super::wrap_output_type_expr`] minting helper, so the
        /// reduce-then-raise orchestrator and the shell-raise delegator
        /// construct the carrier here; code outside
        /// `project_semantic_dispatch` cannot reach this constructor and must
        /// go through the authority.
        /// This mirrors the pre-vault visibility (the carrier formerly lived
        /// directly in `output_materialization`, where `pub(super)` resolved to
        /// `project_semantic_dispatch`); the vault moved the carrier one level
        /// deeper, so the same reach is now spelled explicitly.
        pub(in crate::project_semantic_dispatch) fn from_raise<
            C: crate::resolver_core::ResolverCapabilities,
        >(
            dispatch: &crate::project_semantic_dispatch::ProjectSemanticDispatch<'_, C>,
            materialized: MaterializedTypeExpr,
        ) -> Self {
            Self(payload::OutputPayload::new(
                EngineIdentity::of(dispatch),
                materialized,
            ))
        }

        /// Seal a payload under `authority`'s engine (the raw-wrapping
        /// helpers' path).
        pub(super) fn sealed_by(
            authority: &OutputAuthority,
            materialized: MaterializedTypeExpr,
        ) -> Self {
            Self(payload::OutputPayload::new(
                authority.engine().clone(),
                materialized,
            ))
        }

        /// Test-only: seal a payload stamped with no engine (it unwraps under
        /// no authority).
        #[cfg(test)]
        pub(crate) fn unbound_for_test(materialized: MaterializedTypeExpr) -> Self {
            Self(payload::OutputPayload::new(
                EngineIdentity::unbound(),
                materialized,
            ))
        }

        /// Read the inner [`TypeExpr`] out, consuming the carrier. Requires an
        /// [`OutputAuthority`] — the compiler-enforced unwrap-locality gate.
        /// Delegates to the vault's authority-gated accessor; the borrowed
        /// authority is the proof the caller holds output power; it is not
        /// otherwise consulted.
        pub(crate) fn into_type_expr(self, authority: &OutputAuthority) -> TypeExpr {
            self.0.into_type_expr(authority)
        }

        /// `true` when the sealed tree carries any typed resolver-degradation
        /// leaf (at ANY depth, not only the root) — a genuine unmaterialized
        /// sentinel (`QueryError::Miss`, `UnmodeledPosition`,
        /// `BudgetExceeded`, …), never a deliberately-materialised
        /// placeholder (`RaiseMiss`, `TypeParamCycle`, …). Reading this flag
        /// does NOT require an [`OutputAuthority`]: it exposes a
        /// bool fact about the sealed payload, never the vaulted
        /// [`TypeExpr`] itself, so a caller can decide whether to unwrap at
        /// all before spending the capability-gated read.
        pub(crate) fn has_degradation(&self) -> bool {
            self.0.has_degradation()
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
            // CHOKE POINT: any degradation leaf riding the payload's sidecar
            // marks the result partial — the typed degradation channel feeds
            // the same partial bit the reducer supplies.
            let result_is_partial = result_is_partial || type_expr.0.has_degradation();
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
        /// root-sentinel gate in sink-private `reduce_field_value_node` and the
        /// node-domain shape comparison in `reduce_published_field_types` read
        /// node facts off this id instead of re-materialising a `TypeExpr`.
        /// (The former TypeExpr helper `reduce_field_type_expr_with_mode` is deleted.)
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

        /// Consume this materialized output carrier and unwrap its raised type
        /// payload at the owning output sink. The capability argument preserves
        /// the same compiler-enforced unwrap boundary as [`OutputTypeExpr`].
        #[allow(
            dead_code,
            reason = "part of the sealed output-carrier contract; current sinks use the plain carrier after graph-native reduction"
        )]
        pub(crate) fn into_type_expr(self, authority: &OutputAuthority) -> TypeExpr {
            self.type_expr.into_type_expr(authority)
        }

        /// Borrow the inner [`TypeExpr`] payload. Requires an
        /// [`OutputAuthority`] — the compiler-enforced
        /// unwrap-locality gate for the borrowing read sites. Delegates to the
        /// vault's capability-gated accessor. The carrier's documented borrow
        /// read-surface, paired with the live by-value [`Self::into_type_expr`]:
        /// the per-field sentinel gate now reads the node-domain root-sentinel
        /// fact off the carrier `node_id` instead of borrowing the `TypeExpr`, so
        /// the borrow accessor (like the sibling `node_id` accessor) has no
        /// current caller but stays as the carrier's read contract.
        /// Test-only carrier assembly from a raw [`TypeExpr`] (no capability
        /// required). Gated `#[cfg(any(test, feature = "test-support"))]` — NOT
        /// `#[cfg(any(test, feature = "test-support"))]` — so it is reachable ONLY from
        /// genuine test code (the in-crate `#[cfg(test)]` suites AND, via the
        /// non-default `test-support` feature, the separate integration-test
        /// binary's `ShapeCacheDb` synthetic-carrier proof helpers). Per-package
        /// artifact builds (`pnpm run build:lsp`, napi, wasm) omit it; a
        /// whole-workspace build compiles it in through the compile-contract
        /// variant crate's normal dependency. The carrier it builds is stamped
        /// with no engine, so it unwraps under no authority.
        /// `debug_assertions` is ON in the dev cargo profile, so a
        /// `debug_assertions`-OR gate would expose a capability-free carrier
        /// constructor in ordinary debug builds — the exact reverse-materialization
        /// laundering the fence forbids. `test-support` is absent from `default`,
        /// so a planted hot `MaterializedOutputTypeExpr::from_type_expr_for_test(..)`
        /// in a non-test module fails to compile in every per-package artifact
        /// build (debug AND release). The fence-shape inventory allows EXACTLY `#[cfg(test)]` /
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
                type_expr: OutputTypeExpr(payload::OutputPayload::new(
                    EngineIdentity::unbound(),
                    MaterializedTypeExpr::exact(type_expr),
                )),
                dep_signature,
                result_is_partial,
            }
        }

        /// Test-only borrow of the inner [`TypeExpr`] payload (no capability
        /// required). Gated `#[cfg(any(test, feature = "test-support"))]` — see
        /// [`Self::from_type_expr_for_test`] for why `debug_assertions` is excluded
        /// (it would re-open the carrier-unwrap hole in ordinary debug builds) and
        /// why the non-default `test-support` feature is the sanctioned way to
        /// reach this accessor from the separate integration-test binary.
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

/// Seal a raw [`TypeExpr`] into an [`OutputTypeExpr`] carrier. Requires the
/// [`OutputAuthority`] — code without it cannot mint a sealed payload. Used
/// by the sink sites that assemble a [`MaterializedOutputTypeExpr`] from a
/// freshly-computed [`TypeExpr`] rather than from a boundary-method carrier.
pub(crate) fn wrap_output_type_expr(
    authority: &OutputAuthority,
    type_expr: TypeExpr,
) -> OutputTypeExpr {
    // A raw freshly-computed `TypeExpr` carries no degradation — seal it with
    // an EXACT (empty) sidecar.
    OutputTypeExpr::sealed_by(
        authority,
        crate::project_semantic_dispatch::raise::MaterializedTypeExpr::exact(type_expr),
    )
}

/// Seal a TYPED unmaterialized failure (`Miss`-family [`QueryError`]) into an
/// [`OutputTypeExpr`], the terminal counterpart of [`wrap_output_type_expr`]
/// for a present-but-unraisable node: the sidecar carries the degradation
/// leaf, so the payload goes PARTIAL at the `from_parts` choke point (never
/// laundered to a complete empty `Unknown`). Requires the [`OutputAuthority`].
///
/// [`QueryError`]: crate::semantic_query::QueryError
pub(crate) fn wrap_degraded_output(
    authority: &OutputAuthority,
    reason: crate::semantic_query::QueryError,
) -> OutputTypeExpr {
    OutputTypeExpr::sealed_by(
        authority,
        crate::project_semantic_dispatch::raise::MaterializedTypeExpr::degraded(reason),
    )
}
