//! The apparent-type surface of a callable.
//!
//! A type that carries call signatures exposes, in addition to its own
//! members, the members of the ambient callable-function interface — that
//! is where `.call` / `.apply` / `.bind` / `.name` live. This producer owns
//! that widening for the `SemanticQueryKey::ApparentType` family, so the
//! PathWalker never performs an ambient lookup of its own: the walker asks
//! for the apparent type and continues its member step against whatever
//! surface comes back.
//!
//! The widening is registry-proved, never spelled:
//!
//! 1. the base must BE callable — a [`SemanticNodeData::Signature`], a
//!    [`SemanticNodeData::DeferredCallable`], or an object surface carrying
//!    call signatures — reached through alias hops;
//! 2. a scoping canonical resolves the project whose ambient registry is
//!    consulted. An AUTHORED callable is scoped by its declaring canonical
//!    (the same per-canonical env scoping every other context builder on
//!    this dispatch uses). A ROOTLESS callable (a parameter annotation, a
//!    local arrow, an object-type call signature — no authored occurrence)
//!    is scoped by the LEXICAL DEMAND canonical carried in the key's
//!    [`ApparentDemandScope::Rootless`] witness — the canonical containing
//!    the member-access/call site that demanded the surface;
//! 3. the interface is the ambient `Function` that project's registered
//!    corpus exposes (see [`CALLABLE_APPARENT_INTERFACE`]);
//! 4. the surface is the ambient declaration's own `Instantiate` result, so
//!    every member carries the ambient canonical as its declaration origin.
//!
//! A base that is not callable, a rootless callable whose key carries no
//! demand-scope witness, a canonical with no owning project, and a project
//! whose ambient corpus registers no such interface all return
//! [`QueryError::Miss`]. A ROOTLESS value resolves semantically but NEVER
//! enters a shared cache: the build is marked `cache_suppress`, and that
//! taint folds through every enclosing member/path/call query at the
//! universal read boundary. The primitive-to-wrapper widening (`string` →
//! `String`, `number` → `Number`, …) is a separate surface and is not
//! produced here; those bases also return `Miss`.

use std::sync::Arc;

use verter_semantic::resolver_core::ProjectStableKey;

use crate::semantic_query::{
    ApparentDemandScope, ApparentTypeContext, ProjectionMode, ProjectionReductionContext,
    QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput,
};

use super::ProjectSemanticDispatch;

/// The ambient interface a callable's apparent members come from.
///
/// TypeScript's `globalCallableFunctionType` is `CallableFunction` when
/// `strictBindCallApply` is on and `Function` otherwise. Verter's project
/// model carries no `strictBindCallApply` input — `type_env_hash` composes a
/// fixed non-strict type env — so the apparent callable surface is
/// `Function`.
const CALLABLE_APPARENT_INTERFACE: &str = "Function";

/// The producer's base classification: how (and whether) a callable node
/// anchors the ambient lookup.
enum CallableAnchor {
    /// The node carries no call signatures at all — no apparent surface.
    NotCallable,
    /// The callable carries an authored occurrence; its declaring canonical
    /// scopes the lookup.
    Authored(Arc<str>),
    /// The callable is rootless (no authored occurrence): only a lexical
    /// demand canonical can scope the lookup.
    Rootless,
    /// Signature discovery did not settle for the node, so whether it is
    /// callable is unknown: no apparent surface, and the discovery read's
    /// own partial rails keep the answer out of the shared memo.
    Undecided,
}

impl ProjectSemanticDispatch<'_> {
    /// Build the `ApparentType` value for `base`.
    pub(super) fn build_apparent_type(
        &self,
        base: SemanticNodeId,
        context: &ApparentTypeContext,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let fence = self.project_generation_signature();
        let miss = || -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
            (
                QueryResult::Error(QueryError::Miss),
                self.project_generation_signature(),
            )
                .into()
        };
        // The classification authority is the NODE; the key's demand-scope
        // witness is consulted only for a rootless base. A rootless value
        // is transaction-local: `cache_suppress` keeps it (and every
        // enclosing projection that read it) out of the shared memo.
        let (canonical, rootless): (Arc<str>, bool) = match self.callable_anchor(base) {
            CallableAnchor::NotCallable | CallableAnchor::Undecided => return miss(),
            CallableAnchor::Authored(canonical) => (canonical, false),
            CallableAnchor::Rootless => match &context.demand_scope {
                ApparentDemandScope::Rootless { canonical } => (Arc::clone(canonical), true),
                // No demand-scope witness — no project to scope the ambient
                // lookup by. Fail closed.
                ApparentDemandScope::Anchored => return miss(),
            },
        };
        // The global the project declares, through the one global lookup:
        // the consumer depends on the library it reads (a re-registration
        // invalidates it through the standard dependency-fact validators)
        // and, when the library declares none, on the program's global
        // contributors. For a rootless base the recorded consumer is the
        // demand canonical — the demand-project read.
        let Some(declaration) =
            self.first_global_declaration(canonical.as_ref(), CALLABLE_APPARENT_INTERFACE)
        else {
            return miss();
        };

        let slot = self.type_slot_for(
            Arc::clone(&declaration.canonical_id),
            declaration.owner,
            Arc::from(CALLABLE_APPARENT_INTERFACE),
        );
        let surface = self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                slot,
                Arc::from(Vec::new().into_boxed_slice()),
                self.instantiate_context_for(
                    declaration.canonical_id.as_ref(),
                    ProjectionReductionContext::published(ProjectionMode::Expanded),
                ),
            ),
        ));
        match surface {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => {
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                    (QueryResult::Value(value), fence).into();
                // A rootless apparent value never enters the shared memo;
                // the value still flows to the caller, and the suppress
                // taint folds through every enclosing member/path/call
                // query at the universal read boundary.
                output.cache_suppress |= rootless;
                output
            }
            QueryResult::Recursive(_) | QueryResult::Error(_) => miss(),
        }
    }

    /// The apparent type of `base`, or `None` when the family produced no
    /// surface. The single walker-side entry: it derives the key's context
    /// from the same canonical the producer scopes its lookup by, so key
    /// identity and value basis agree. An authored callable is scoped by
    /// its declaring canonical; a rootless callable is scoped by the
    /// innermost lexical demand canonical (the member-access/call site
    /// currently being evaluated) and carries that canonical in the key's
    /// demand-scope witness. A rootless base with NO demand site on the
    /// stack fails closed.
    pub(super) fn apparent_type_of(&self, base: SemanticNodeId) -> Option<SemanticNodeId> {
        let (canonical, demand_scope) = match self.callable_anchor(base) {
            CallableAnchor::NotCallable | CallableAnchor::Undecided => return None,
            CallableAnchor::Authored(canonical) => (canonical, ApparentDemandScope::Anchored),
            CallableAnchor::Rootless => {
                let canonical = self.lexical_demand_scope.borrow().last().cloned()?;
                (
                    Arc::clone(&canonical),
                    ApparentDemandScope::Rootless { canonical },
                )
            }
        };
        let key = SemanticQueryKey::ApparentType {
            base,
            context: self.apparent_type_context_scoped(canonical.as_ref(), demand_scope),
        };
        match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => Some(value),
            QueryResult::Recursive(_) | QueryResult::Error(_) => None,
        }
    }

    /// Production constructor for the env-bearing [`ApparentTypeContext`]:
    /// the key has no decl slot, so the R21 `T L J` dimensions the
    /// apparent surface depends on ride in the context, plus the
    /// demand-scope witness. The env dims derive from the SAME canonical
    /// the scope resolves by (the declaring canonical for `Anchored`, the
    /// witness canonical for `Rootless`), so key identity and value basis
    /// agree on both arms. No content/version hash enters the
    /// query-identity key (R6).
    #[must_use]
    fn apparent_type_context_scoped(
        &self,
        canonical: &str,
        demand_scope: ApparentDemandScope,
    ) -> ApparentTypeContext {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical);
        ApparentTypeContext {
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity_for(canonical).fold_u32(),
            demand_scope,
        }
    }

    /// Classify how `node` anchors a callable-apparent lookup, reached
    /// through alias hops. [`CallableAnchor::NotCallable`] for a node that
    /// carries no call signatures at all; [`CallableAnchor::Authored`] with
    /// the declaring canonical for a callable with an authored occurrence;
    /// [`CallableAnchor::Rootless`] for a callable with none (an inline
    /// function value, a function-typed parameter, an object-type call
    /// signature).
    fn callable_anchor(&self, node: SemanticNodeId) -> CallableAnchor {
        let graph = self.graph();
        let mut node = node;
        let mut visited: rustc_hash::FxHashSet<SemanticNodeId> = rustc_hash::FxHashSet::default();
        loop {
            if !visited.insert(node) {
                return CallableAnchor::NotCallable;
            }
            let Some(data) = graph.node_data(node) else {
                return CallableAnchor::NotCallable;
            };
            match &*data {
                SemanticNodeData::Alias(target) => node = *target,
                SemanticNodeData::Signature { occurrence, .. } => {
                    return match occurrence {
                        Some(occurrence) => CallableAnchor::Authored(Arc::clone(
                            &occurrence.function.anchor.canonical_id,
                        )),
                        None => CallableAnchor::Rootless,
                    };
                }
                SemanticNodeData::DeferredCallable(callable) => {
                    return CallableAnchor::Authored(Arc::clone(callable.declaring_canonical()));
                }
                // A surface with call signatures is callable too; its FIRST
                // call signature anchors the lookup (every signature of one
                // callable position is authored in the same file). The
                // signatures are the discovery authority's, never the
                // surface's own lists: a surface merged for presentation
                // carries lists the kernel did not produce.
                SemanticNodeData::Object(_) => {
                    drop(data);
                    match self
                        .shared_signature_nodes(node, crate::semantic_query::SignatureKind::Call)
                    {
                        super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => {
                            let Some(first) = nodes.first().copied() else {
                                return CallableAnchor::NotCallable;
                            };
                            node = first;
                        }
                        super::signature_discovery::SharedSignatureNodes::Incomplete(_) => {
                            return CallableAnchor::Undecided;
                        }
                    }
                }
                _ => return CallableAnchor::NotCallable,
            }
        }
    }

    /// Resolve `canonical`'s owning project to the stable key the ambient
    /// registry is partitioned by.
    pub(super) fn project_stable_key_for_canonical(
        &self,
        canonical: &str,
    ) -> Option<ProjectStableKey> {
        let host = self.ctx.host_for_fact_tracer_install();
        let project = host.resolve_project_for_canonical(canonical)?;
        host.workspace().project_stable_key(project)
    }
}

#[cfg(test)]
#[path = "apparent_type_tests.rs"]
mod apparent_type_tests;

/// A global wrapper interface read for an apparent type
/// ([`ProjectSemanticDispatch::global_wrapper_surface`]).
pub(super) enum GlobalWrapper {
    /// The instantiated interface's surface.
    Surface(SemanticNodeId),
    /// The project declares no such global (`noLib`): the checker's empty
    /// apparent type.
    Absent,
    /// The canonical names no project, or the interface's instantiation did
    /// not settle.
    Unsettled,
}

impl ProjectSemanticDispatch<'_> {
    /// The global wrapper interface `name` (`String`, `Number`, `Array`, …)
    /// instantiated with `args` — the apparent type of a primitive, a
    /// literal or an array — as `canonical`'s project declares it, through
    /// the one global lookup ([`Self::first_global_declaration`]): its
    /// library's, else the program's own, merged with every later
    /// declaration. The lookup records `canonical`'s dependency on the
    /// library it reads and on the program's global contributors, so a
    /// re-registration, or a declaration appearing after a miss, invalidates
    /// the read.
    pub(super) fn global_wrapper_surface(
        &self,
        name: &str,
        args: &[SemanticNodeId],
        canonical: &str,
    ) -> GlobalWrapper {
        if self.project_stable_key_for_canonical(canonical).is_none() {
            return GlobalWrapper::Unsettled;
        }
        let Some(declaration) = self.first_global_declaration(canonical, name) else {
            return GlobalWrapper::Absent;
        };
        let slot = self.type_slot_for(
            Arc::clone(&declaration.canonical_id),
            declaration.owner,
            Arc::from(name),
        );
        match self
            .execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    Arc::from(args.to_vec().into_boxed_slice()),
                    self.instantiate_context_for(
                        declaration.canonical_id.as_ref(),
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    ),
                ),
            ))
            .value
        {
            QueryResult::Value(surface) => GlobalWrapper::Surface(surface),
            _ => GlobalWrapper::Unsettled,
        }
    }

    /// The file a wrapper read that names no declaration of its own is
    /// scoped by: the request's file, else the innermost demand site.
    pub(super) fn wrapper_demand_canonical(&self) -> Option<Arc<str>> {
        crate::request_context::current_request_canonical()
            .or_else(|| self.lexical_demand_scope.borrow().last().cloned())
    }

    /// The global wrapper whose signatures and members `node` has — its
    /// apparent type (`getApparentType`): `String` for a string, a string
    /// literal or a template, `Number`, `Boolean`, `BigInt` and `Symbol`
    /// likewise (a `unique symbol` too), and `Array` / `ReadonlyArray` over
    /// an array's element type. `None` for any other node.
    pub(super) fn apparent_wrapper_of(
        &self,
        node: SemanticNodeId,
    ) -> Option<(&'static str, Vec<SemanticNodeId>)> {
        use crate::semantic_query::{LiteralValue, PrimitiveKind};
        let data = self.graph().node_data(node)?;
        let name = match &*data {
            SemanticNodeData::Primitive(PrimitiveKind::String)
            | SemanticNodeData::Literal(LiteralValue::String(_))
            | SemanticNodeData::TemplateLiteral { .. } => "String",
            SemanticNodeData::Primitive(PrimitiveKind::Number)
            | SemanticNodeData::Literal(LiteralValue::Number(_)) => "Number",
            SemanticNodeData::Primitive(PrimitiveKind::Boolean)
            | SemanticNodeData::Literal(LiteralValue::Boolean(_)) => "Boolean",
            SemanticNodeData::Primitive(PrimitiveKind::BigInt)
            | SemanticNodeData::Literal(LiteralValue::BigInt(_)) => "BigInt",
            SemanticNodeData::Primitive(PrimitiveKind::Symbol)
            | SemanticNodeData::TypeOfNominal(_) => "Symbol",
            SemanticNodeData::Array { element, readonly } => {
                let name = if *readonly { "ReadonlyArray" } else { "Array" };
                return Some((name, vec![*element]));
            }
            _ => return None,
        };
        Some((name, Vec::new()))
    }

    /// Whether `key` is a `SignaturesOfType` read whose subject takes its
    /// signatures from a global wrapper ([`Self::apparent_wrapper_of`]).
    pub(super) fn key_reads_apparent_signatures(&self, key: &SemanticQueryKey) -> bool {
        matches!(
            key,
            SemanticQueryKey::SignaturesOfType { subject, .. }
                if self.apparent_wrapper_of(*subject).is_some()
        )
    }

    /// Rewrite a `SignaturesOfType` read of a subject every project shares
    /// (`string`, `1`, `T[]`) to the same read over the wrapper the
    /// demanding project declares, BEFORE memo admission, so the admitted
    /// entry's key names that project's wrapper and a warm repeat is served
    /// from it. A project declaring no such wrapper gives the subject no
    /// apparent signature — the answer `never` has, which every project
    /// shares — and the lookup facts of the miss root that entry, so a
    /// declaration appearing later misses it. With no demand site the key
    /// stays as it is: its build reads the wrapper for the demand and stays
    /// out of the shared memo.
    pub(super) fn scope_apparent_signature_subject(
        &self,
        key: SemanticQueryKey,
    ) -> SemanticQueryKey {
        let SemanticQueryKey::SignaturesOfType {
            subject,
            kind,
            context,
        } = key
        else {
            return key;
        };
        let scoped = self
            .apparent_wrapper_of(subject)
            .zip(self.wrapper_demand_canonical())
            .and_then(|((name, args), canonical)| {
                match self.global_wrapper_surface(name, &args, canonical.as_ref()) {
                    GlobalWrapper::Surface(surface) => Some(surface),
                    GlobalWrapper::Absent => Some(self.graph().intern_node(
                        SemanticNodeData::Primitive(crate::semantic_query::PrimitiveKind::Never),
                    )),
                    GlobalWrapper::Unsettled => None,
                }
            });
        SemanticQueryKey::SignaturesOfType {
            subject: scoped.unwrap_or(subject),
            kind,
            context,
        }
    }
}
