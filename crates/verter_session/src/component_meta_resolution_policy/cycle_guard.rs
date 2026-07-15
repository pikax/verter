//! Cycle-guard normalized type-argument identity.
//!
//! Recursion guards inside the policy walker (the `rewrite_ref_node`
//! active-ref stack) key on `(DeclIdentity, NormalizedTypeArgs)`. Bare-name
//! strings collide across scopes (two `Foo`s in different files) and cannot
//! distinguish `Pick<X, 'a'>` from `Pick<X, 'b'>` inside a generic
//! instantiation chain. The active-refs set on `PolicyCtx` consumes these
//! types; [`NormalizedTypeArgs::normalize_nodes`] is the constructor the
//! cycle guard uses to discriminate generic instantiations — it consumes the
//! raised semantic-graph ARGUMENT NODES, never a materialized `TypeExpr`.
//!
//! This identity is an EPHEMERAL per-request recursion guard, NOT a cache
//! validity oracle: the structural fingerprint below discriminates the
//! active-set keys within one policy invocation; a collision at worst
//! over-cuts one request's recursion and never poisons a cross-request
//! cache (contrast the `ShapeSubject` cache subject, which keys the EXACT
//! lowered node and must never weaken to a fingerprint).

use std::hash::{Hash, Hasher};

use smallvec::SmallVec;
use verter_type_expr::LiteralValue;

use crate::semantic_query::{DeclIdentity, IndexKey, SemanticNodeData, SemanticNodeId};

use super::core::{DeclLookup, PolicyCtx};

/// Hash of an opaque anonymous shape used to discriminate cycle-guard keys
/// across structurally-identical inline type expressions. Identity equality
/// of the hash is what `NormalizedTypeArg` checks.
pub(crate) type ShapeHash = u64;

/// Hash of a `LiteralValue` used as a cheap discriminator for literal-arg
/// cycle-guard keys (e.g. `Pick<X, 'a'>` vs `Pick<X, 'b'>`).
pub(crate) type LiteralHash = u64;

/// One normalized type argument for the cycle-guard key. The five variants
/// cover every shape the cycle guard observes:
///
/// - `Decl(DeclIdentity)` — an argument-free named declaration reference
///   (resolved by the resolver). Two args resolve to the same
///   `DeclIdentity` when their bare-name lookups land on the same
///   declaration.
/// - `InstantiatedDecl { identity, args_fingerprint }` — a resolved named
///   declaration reference that itself carries positional type arguments
///   (`Wrapper<'a'>`). The head stays EXACT (`identity`), so a resolved
///   head never collides with a different resolved head; `args_fingerprint`
///   is the ordered, complete structural fingerprint of the positional
///   arguments, so `Foo<Wrapper<'a'>>` and `Foo<Wrapper<'b'>>` stay
///   distinct once `Wrapper` resolves instead of collapsing onto the same
///   `Decl(Wrapper)` key.
/// - `Literal(LiteralHash)` — a literal value (`'a'`, `42`, `true`).
///   Different literal values produce different hashes, so `Pick<X, 'a'>`
///   and `Pick<X, 'b'>` produce different `NormalizedTypeArgs`.
/// - `AnonymousShape(ShapeHash)` — an inline anonymous shape that has no
///   declaration identity (e.g. `{ a: string }` passed inline as a type
///   argument), or a reference head the resolver could not locate. The
///   hash carries enough structural information to distinguish unrelated
///   inline shapes.
/// - `None` — empty / missing argument slot. Reserved for ambient
///   reductions where the caller wants to discriminate between "no arg"
///   and "a real arg that happens to hash to zero".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum NormalizedTypeArg {
    Decl(DeclIdentity),
    InstantiatedDecl {
        identity: DeclIdentity,
        args_fingerprint: ShapeHash,
    },
    Literal(LiteralHash),
    AnonymousShape(ShapeHash),
    None,
}

/// Normalized form of a type-argument list, used as the second component
/// of cycle-guard keys (the first is the resolved `DeclIdentity` of the
/// declaration being entered).
///
/// Normalization is deterministic: two argument-node lists that resolve to
/// the same declaration identities and literal values produce the same
/// `NormalizedTypeArgs` value, regardless of which raise produced the
/// nodes. The ordering of arguments IS preserved (positional arguments) —
/// different positions are different keys — and EVERY argument contributes
/// one entry (complete).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct NormalizedTypeArgs(SmallVec<[NormalizedTypeArg; 4]>);

impl NormalizedTypeArgs {
    /// Build an empty `NormalizedTypeArgs`. Used as the cycle-guard key
    /// for a declaration entered with no type arguments.
    #[must_use]
    pub(crate) fn empty() -> Self {
        Self(SmallVec::new())
    }

    /// Build a `NormalizedTypeArgs` from an iterator of normalized
    /// arguments. Caller is responsible for resolving each input node to
    /// its `NormalizedTypeArg` (the resolver-aware constructor lives in
    /// the bundle that consumes this type).
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn from_normalized<I>(args: I) -> Self
    where
        I: IntoIterator<Item = NormalizedTypeArg>,
    {
        Self(args.into_iter().collect())
    }

    /// Resolver-aware constructor — normalizes the raised semantic-graph
    /// ARGUMENT NODES of a reference carrier into the cycle-guard key form.
    /// Each input node maps to one `NormalizedTypeArg`, in positional
    /// order:
    ///
    /// * a reference-headed node resolves through the policy's declaration
    ///   lookup and produces `Decl(DeclIdentity)` keyed on the resolved
    ///   declaration's canonical source. Heads the resolver cannot locate
    ///   fall back to `AnonymousShape(structural hash)` so unresolved
    ///   references still discriminate by name.
    /// * a `Literal` node produces `Literal(hash_literal(v))` so
    ///   `Pick<X, 'a'>` and `Pick<X, 'b'>` produce distinct keys.
    /// * an `Infer` node produces `None`.
    /// * every other shape produces `AnonymousShape(structural hash)` — the
    ///   deterministic node-graph walk of [`hash_semantic_node_structurally`]
    ///   — so structurally-identical inline shapes share an identity but
    ///   distinct shapes do not collide.
    ///
    /// The function is `&mut PolicyCtx` because resolving a reference
    /// argument may consult the same declaration cache the cycle guard
    /// itself uses.
    #[must_use]
    pub(super) fn normalize_nodes(args: &[SemanticNodeId], ctx: &mut PolicyCtx<'_, '_>) -> Self {
        let mut out: SmallVec<[NormalizedTypeArg; 4]> = SmallVec::new();
        for arg in args {
            out.push(normalize_one_node(*arg, ctx));
        }
        Self(out)
    }

    /// Number of arguments (zero for `empty`).
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// True when no arguments are present.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterator over the normalized arguments in positional order.
    #[allow(dead_code)]
    pub(crate) fn iter(&self) -> impl Iterator<Item = &NormalizedTypeArg> {
        self.0.iter()
    }
}

impl Default for NormalizedTypeArgs {
    fn default() -> Self {
        Self::empty()
    }
}

/// Resolve one raised argument NODE into its `NormalizedTypeArg` shape.
fn normalize_one_node(node: SemanticNodeId, ctx: &mut PolicyCtx<'_, '_>) -> NormalizedTypeArg {
    if let Some((name, args)) = ctx.node_ref_head(node) {
        // Try to resolve the reference head to a declaration identity. When
        // the lookup succeeds the canonical source uniquely keys the head;
        // otherwise fall back to a structural hash so the cycle guard still
        // discriminates by name.
        if let Some(DeclLookup {
            canonical_source, ..
        }) = ctx.locate_declaration(name.as_str())
        {
            let identity = ctx.decl_identity_for(&canonical_source, name.as_str());
            // A resolved head keeps its EXACT `DeclIdentity`; its positional
            // type arguments are folded in structurally so distinct nested
            // instantiations stay distinct. An arg-free ref stays the exact
            // `Decl` form; `Wrapper<'a'>` and `Wrapper<'b'>` must NOT both
            // collapse onto `Decl(Wrapper)` (a false-positive cycle that
            // over-cuts one of the two requests).
            if args.is_empty() {
                return NormalizedTypeArg::Decl(identity);
            }
            return NormalizedTypeArg::InstantiatedDecl {
                identity,
                args_fingerprint: hash_arg_nodes(&args, ctx),
            };
        }
        return NormalizedTypeArg::AnonymousShape(hash_node(node, ctx));
    }
    match ctx.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(value)) => NormalizedTypeArg::Literal(hash_literal(value)),
        Some(SemanticNodeData::Infer { .. }) => NormalizedTypeArg::None,
        _ => NormalizedTypeArg::AnonymousShape(hash_node(node, ctx)),
    }
}

/// Ordered, complete structural fingerprint of a resolved reference's
/// POSITIONAL type arguments, folded behind an arity prefix. Reuses the
/// cycle-safe, arg-complete [`hash_node_rec`] walker (one shared `seen`
/// back-reference table plus the depth fuse) so a resolved head's nested
/// arguments discriminate — `Foo<Wrapper<'a'>>` and `Foo<Wrapper<'b'>>`
/// fingerprint differently once `Wrapper` resolves — while the head itself
/// stays EXACT via its `DeclIdentity`. A fingerprint collision only
/// over-cuts one request's recursion (it never under-cuts a real cycle and
/// never poisons a cross-request cache), matching the ephemeral-guard
/// contract documented on this module.
fn hash_arg_nodes(args: &[SemanticNodeId], ctx: &PolicyCtx<'_, '_>) -> ShapeHash {
    let resolver = ctx.resolver_ctx();
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    let mut seen: rustc_hash::FxHashMap<SemanticNodeId, u64> = rustc_hash::FxHashMap::default();
    bridge.write_u64(args.len() as u64);
    for arg in args {
        hash_node_rec(resolver, *arg, &mut bridge, &mut seen, 0);
    }
    hasher.digest()
}

/// Compute a deterministic 64-bit structural fingerprint of a semantic-graph
/// node through [`hash_semantic_node_structurally`], digested via `xxh3` to
/// match the shape used elsewhere in the cycle guard's identity space.
fn hash_node(node: SemanticNodeId, ctx: &PolicyCtx<'_, '_>) -> ShapeHash {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    hash_semantic_node_structurally(ctx.resolver_ctx(), node, &mut bridge);
    hasher.digest()
}

/// The ONE structural fingerprint walker over semantic-graph nodes for the
/// policy cycle guard: a deterministic pre-order walk that writes, per node,
/// a variant tag plus the node's discriminating payloads (reference head
/// names + declaration identities, literal values, primitive kinds, member
/// names / flags, arity counts) and descends into child nodes in source
/// order. Shared / cyclic child references write a back-reference ordinal
/// instead of recursing, so the walk terminates on any graph and two
/// traversals of the same structure produce the same digest.
///
/// The fingerprint is ORDERED (children hash in positional order behind
/// explicit arity counts), COMPLETE (every child position contributes), and
/// NON-LOSSY in the discriminating dimensions (resolved reference heads,
/// literal values, and composite structural shape stay distinct). It is an
/// ephemeral recursion-guard identity only — never a cache key.
pub(crate) fn hash_semantic_node_structurally<H: std::hash::Hasher>(
    ctx: &dyn crate::resolver_core::ResolverContext,
    root: SemanticNodeId,
    hasher: &mut H,
) {
    use rustc_hash::FxHashMap;

    // Pre-order ordinals for back-reference encoding on shared / cyclic
    // child edges.
    let mut seen: FxHashMap<SemanticNodeId, u64> = FxHashMap::default();
    hash_node_rec(ctx, root, hasher, &mut seen, 0);
}

fn hash_node_rec<H: std::hash::Hasher>(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
    hasher: &mut H,
    seen: &mut rustc_hash::FxHashMap<SemanticNodeId, u64>,
    depth: u32,
) {
    // Depth fuse: a pathological graph degrades to a truncation tag (the
    // guard over-cuts at worst; it never overflows the stack).
    if depth > 128 {
        hasher.write_u8(0xFF);
        return;
    }
    if let Some(ordinal) = seen.get(&node) {
        hasher.write_u8(0xFE);
        hasher.write_u64(*ordinal);
        return;
    }
    let ordinal = seen.len() as u64;
    seen.insert(node, ordinal);

    let Some(data) = crate::project_semantic_dispatch::node_data_for(ctx, node) else {
        hasher.write_u8(0xFD);
        return;
    };

    // Reference carriers: head name (+ scope-free declaration identity when
    // carried) + arity + arg descent.
    if let Some((name, _scope)) = data.bare_ref_head() {
        hasher.write_u8(1);
        hasher.write(name.as_bytes());
        let args = data.carrier_type_args();
        hasher.write_u64(args.len() as u64);
        for arg in args {
            hash_node_rec(ctx, *arg, hasher, seen, depth + 1);
        }
        return;
    }
    match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => {
            hasher.write_u8(2);
            identity.hash(hasher);
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            hasher.write_u8(3);
            base.hash(hasher);
            hasher.write_u64(args.len() as u64);
            for arg in args.iter() {
                hash_node_rec(ctx, *arg, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::Literal(value) => {
            hasher.write_u8(4);
            value.hash(hasher);
        }
        SemanticNodeData::Primitive(kind) => {
            hasher.write_u8(5);
            kind.hash(hasher);
        }
        SemanticNodeData::Alias(target) => {
            hasher.write_u8(6);
            hash_node_rec(ctx, *target, hasher, seen, depth + 1);
        }
        SemanticNodeData::Object(surface) => {
            hasher.write_u8(7);
            hasher.write_u64(surface.members.len() as u64);
            for member in surface.members.iter() {
                hasher.write(member.name.as_bytes());
                hasher.write_u8(u8::from(member.optional));
                hasher.write_u8(u8::from(member.readonly));
                hasher.write_u8(u8::from(member.is_method));
                hash_node_rec(ctx, member.value, hasher, seen, depth + 1);
            }
            hasher.write_u64(surface.call_signatures.len() as u64);
            for signature in surface.call_signatures.iter() {
                hash_node_rec(ctx, *signature, hasher, seen, depth + 1);
            }
            hasher.write_u64(surface.construct_signatures.len() as u64);
            for signature in surface.construct_signatures.iter() {
                hash_node_rec(ctx, *signature, hasher, seen, depth + 1);
            }
            hasher.write_u64(surface.index_signatures.len() as u64);
            for signature in surface.index_signatures.iter() {
                hash_node_rec(ctx, signature.key_type, hasher, seen, depth + 1);
                hash_node_rec(ctx, signature.value_type, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::Union(arms) => {
            hasher.write_u8(8);
            hasher.write_u64(arms.len() as u64);
            for arm in arms.iter() {
                hash_node_rec(ctx, *arm, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::Intersection(arms) => {
            hasher.write_u8(9);
            hasher.write_u64(arms.len() as u64);
            for arm in arms.iter() {
                hash_node_rec(ctx, *arm, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::Array { element, readonly } => {
            hasher.write_u8(10);
            hasher.write_u8(u8::from(*readonly));
            hash_node_rec(ctx, *element, hasher, seen, depth + 1);
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            hasher.write_u8(11);
            hasher.write_u8(u8::from(*readonly));
            hasher.write_u64(elements.len() as u64);
            for element in elements.iter() {
                if let Some(label) = element.label.as_deref() {
                    hasher.write_u8(1);
                    hasher.write(label.as_bytes());
                } else {
                    hasher.write_u8(0);
                }
                hasher.write_u8(u8::from(element.optional));
                hasher.write_u8(u8::from(element.rest));
                hash_node_rec(ctx, element.value, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            hasher.write_u8(12);
            hasher.write_u64(quasis.len() as u64);
            for quasi in quasis.iter() {
                hasher.write(quasi.as_bytes());
            }
            hasher.write_u64(expressions.len() as u64);
            for expr in expressions.iter() {
                hash_node_rec(ctx, *expr, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::KeyOf { base } => {
            hasher.write_u8(13);
            hash_node_rec(ctx, *base, hasher, seen, depth + 1);
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            hasher.write_u8(14);
            hash_node_rec(ctx, *object, hasher, seen, depth + 1);
            match index {
                IndexKey::String(key) => {
                    hasher.write_u8(1);
                    hasher.write(key.as_bytes());
                }
                IndexKey::Number(number) => {
                    hasher.write_u8(2);
                    number.hash(hasher);
                }
                IndexKey::TypeNode(index_node) => {
                    hasher.write_u8(3);
                    hash_node_rec(ctx, *index_node, hasher, seen, depth + 1);
                }
            }
        }
        SemanticNodeData::Mapped { source, mapper } => {
            hasher.write_u8(15);
            hash_node_rec(ctx, *source, hasher, seen, depth + 1);
            mapper.hash(hasher);
        }
        SemanticNodeData::TypeOf(_) => {
            hasher.write_u8(16);
            if let Some((value_root, path)) = data.typeof_head() {
                value_root.hash(hasher);
                hasher.write_u64(path.len() as u64);
                for segment in path.iter() {
                    hasher.write(segment.as_bytes());
                }
            }
            for arg in data.carrier_type_args() {
                hash_node_rec(ctx, *arg, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::TypeParam {
            decl, param_index, ..
        } => {
            hasher.write_u8(17);
            decl.hash(hasher);
            hasher.write_u16(*param_index);
        }
        SemanticNodeData::Infer { name } => {
            hasher.write_u8(18);
            hasher.write(name.as_bytes());
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
        } => {
            hasher.write_u8(19);
            hasher.write_u8(u8::from(*distributive));
            hash_node_rec(ctx, *check, hasher, seen, depth + 1);
            hash_node_rec(ctx, *extends, hasher, seen, depth + 1);
            hash_node_rec(ctx, *true_branch_ref, hasher, seen, depth + 1);
            hash_node_rec(ctx, *false_branch_ref, hasher, seen, depth + 1);
        }
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            ..
        } => {
            hasher.write_u8(20);
            hasher.write_u64(params.len() as u64);
            for param in params.iter() {
                if let Some(name) = param.name.as_deref() {
                    hasher.write_u8(1);
                    hasher.write(name.as_bytes());
                } else {
                    hasher.write_u8(0);
                }
                hasher.write_u8(u8::from(param.optional));
                hasher.write_u8(u8::from(param.rest));
                hash_node_rec(ctx, param.ty, hasher, seen, depth + 1);
            }
            hash_node_rec(ctx, *return_type, hasher, seen, depth + 1);
            hasher.write_u64(type_parameters.len() as u64);
            for param in type_parameters.iter() {
                hasher.write(param.name.as_bytes());
            }
        }
        SemanticNodeData::ConstructorType { signature } => {
            hasher.write_u8(21);
            hash_node_rec(ctx, *signature, hasher, seen, depth + 1);
        }
        SemanticNodeData::MergedDecl { contributors } => {
            hasher.write_u8(22);
            hasher.write_u64(contributors.len() as u64);
            for contributor in contributors.iter() {
                hash_node_rec(ctx, *contributor, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::Opaque(error) => {
            hasher.write_u8(23);
            error.hash(hasher);
        }
        SemanticNodeData::RawFallback { raw } => {
            hasher.write_u8(24);
            hasher.write(raw.as_bytes());
        }
        SemanticNodeData::ImportType(_) => {
            hasher.write_u8(25);
            if let Some((specifier, qualifier, typeof_query)) = data.import_type_head() {
                hasher.write(specifier.as_bytes());
                hasher.write_u64(qualifier.len() as u64);
                for segment in qualifier.iter() {
                    hasher.write(segment.as_bytes());
                }
                hasher.write_u8(u8::from(typeof_query));
            }
            for arg in data.carrier_type_args() {
                hash_node_rec(ctx, *arg, hasher, seen, depth + 1);
            }
        }
        SemanticNodeData::SyntheticBinding { id, .. } => {
            hasher.write_u8(26);
            id.hash(hasher);
        }
        // `BareRef` is consumed by the `bare_ref_head` arm above the match;
        // every other reference / literal / primitive / alias variant has an
        // explicit arm.
        SemanticNodeData::BareRef(_) => unreachable!("handled by the bare_ref_head arm"),
    }
}

/// Stable hash of a `LiteralValue` for use inside `NormalizedTypeArg::Literal`.
/// Mirrors the manual `Hash` impl on `LiteralValue` — same input bytes
/// produce the same digest across builds.
#[must_use]
pub(crate) fn hash_literal(value: &LiteralValue) -> LiteralHash {
    let mut hasher = xxhash_rust::xxh3::Xxh3::new();
    let mut bridge = LiteralHashBridge(&mut hasher);
    value.hash(&mut bridge);
    hasher.digest()
}

struct LiteralHashBridge<'a>(&'a mut xxhash_rust::xxh3::Xxh3);

impl<'a> std::hash::Hasher for LiteralHashBridge<'a> {
    fn finish(&self) -> u64 {
        self.0.digest()
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::semantic_query::NodeScopeId;
    use crate::types::HostConfig;
    use crate::VerterHost;

    fn scope() -> NodeScopeId {
        NodeScopeId::File {
            canonical_id: Arc::from("/fixture/owner.vue"),
            whole_hash: Default::default(),
            local_scope: None,
        }
    }

    fn digest(host: &VerterHost, node: crate::semantic_query::SemanticNodeId) -> u64 {
        let mut hasher = xxhash_rust::xxh3::Xxh3::new();
        let mut bridge = LiteralHashBridge(&mut hasher);
        crate::resolver_core::with_bare_host_ctx_for_test(host, |ctx| {
            hash_semantic_node_structurally(ctx, node, &mut bridge);
        });
        hasher.digest()
    }

    /// Structurally-equal node graphs (independently interned literal args)
    /// fingerprint identically, and a structurally-distinct graph
    /// fingerprints differently.
    ///
    /// Discrimination: the raised argument nodes of `Pick<X, 'a'>` vs
    /// `Pick<X, 'b'>` must produce DIFFERENT `ShapeHash` values (the
    /// cycle-guard property the hash exists for), while two
    /// separately-interned `'a'` literals must produce the SAME value
    /// (deterministic structural identity, not allocation identity).
    #[test]
    fn node_structural_hash_discriminates_structure_not_interning() {
        use crate::semantic_query::SemanticNodeData;
        use verter_type_expr::LiteralValue;

        let host = VerterHost::new_standalone(HostConfig::default());
        let graph = host.project_type_store().semantic_graph();
        let lit = |text: &str| {
            graph.intern_node_with_scope(
                SemanticNodeData::Literal(LiteralValue::String(text.to_string())),
                scope(),
            )
        };
        let first = lit("a");
        let second = lit("a");
        let third = lit("b");
        assert_eq!(
            digest(&host, first),
            digest(&host, second),
            "structurally-equal nodes must share one fingerprint",
        );
        assert_ne!(
            digest(&host, first),
            digest(&host, third),
            "'a' and 'b' literal argument nodes must fingerprint differently \
             (Pick<X, 'a'> vs Pick<X, 'b'> discrimination)",
        );
    }

    /// Composite structural shape stays discriminating: two unions over the
    /// same arms in DIFFERENT order fingerprint differently (ordered), and
    /// a union is distinct from its own single arm (complete).
    #[test]
    fn node_structural_hash_is_ordered_and_complete() {
        use crate::semantic_query::{PrimitiveKind, SemanticNodeData};

        let host = VerterHost::new_standalone(HostConfig::default());
        let graph = host.project_type_store().semantic_graph();
        let string_node = graph
            .intern_node_with_scope(SemanticNodeData::Primitive(PrimitiveKind::String), scope());
        let number_node = graph
            .intern_node_with_scope(SemanticNodeData::Primitive(PrimitiveKind::Number), scope());
        let union_ab = graph.intern_node_with_scope(
            SemanticNodeData::Union(Arc::from([string_node, number_node])),
            scope(),
        );
        let union_ba = graph.intern_node_with_scope(
            SemanticNodeData::Union(Arc::from([number_node, string_node])),
            scope(),
        );
        assert_ne!(
            digest(&host, union_ab),
            digest(&host, union_ba),
            "argument ORDER is part of the structural identity",
        );
        assert_ne!(
            digest(&host, union_ab),
            digest(&host, string_node),
            "a composite shape must not collapse to its first arm",
        );
    }

    /// A self-referential node graph terminates deterministically: the
    /// back-reference encoding closes the cycle instead of recursing, and
    /// the digest is stable across walks.
    #[test]
    fn node_structural_hash_terminates_on_cyclic_graph() {
        use crate::semantic_query::{
            MacroOwnBodyStamp, MergeRoleStamp, SemanticNodeData, SurfaceMember, SurfaceView,
        };

        let host = VerterHost::new_standalone(HostConfig::default());
        let graph = host.project_type_store().semantic_graph();
        // Build `type Self = { next: Self }` shape by first interning a
        // placeholder member value, then an object referencing it — the
        // shared-child back-reference arm is what this exercises (true
        // in-arena id cycles cannot be built through interning, but a
        // DIAMOND of shared children walks the same code path).
        let leaf = graph.intern_node_with_scope(
            SemanticNodeData::Primitive(crate::semantic_query::PrimitiveKind::String),
            scope(),
        );
        let member = |name: &str| SurfaceMember {
            name: Arc::from(name),
            value: leaf,
            optional: false,
            readonly: false,
            is_method: false,
            visibility: verter_type_expr::MemberVisibility::Public,
            spans: verter_type_expr::MemberSpans::default(),
            declaration_origin: None,
            declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
            merge_role: MergeRoleStamp::NEUTRAL,
        };
        let object = graph.intern_node_with_scope(
            SemanticNodeData::Object(SurfaceView {
                members: Arc::from([member("a"), member("b")]),
                call_signatures: Arc::from([]),
                construct_signatures: Arc::from([]),
                index_signatures: Arc::from([]),
                keyspace: None,
                has_index_signature: false,
            }),
            scope(),
        );
        let first = digest(&host, object);
        let second = digest(&host, object);
        assert_eq!(first, second, "the walk must be deterministic");
    }

    /// Resolved-nested-arg completeness (DISCRIMINATING). A resolved
    /// reference head that carries DIFFERENT nested generic arguments must
    /// produce DISTINCT cycle-guard keys: `Foo<Wrapper<'a'>>` and
    /// `Foo<Wrapper<'b'>>` collapse onto the SAME `Decl(Wrapper)` key iff
    /// the resolved-head branch of `normalize_one_node` drops its positional
    /// arguments — a false-positive cycle that over-cuts one of the two
    /// requests.
    ///
    /// Unlike `normalized_type_args_distinguishes_distinct_decl_instantiations`
    /// (which hand-builds `NormalizedTypeArg` values and never exercises the
    /// resolver-aware constructor), this test drives `normalize_nodes`
    /// through a real `PolicyCtx` where `Wrapper` RESOLVES (registry-seeded),
    /// so the buggy path takes the resolved branch and drops `'a'` / `'b'`.
    ///
    /// Pre-fix: both normalize to `[Decl(Wrapper)]` (args dropped) → EQUAL →
    /// the `assert_ne!` FAILS. Post-fix: `[InstantiatedDecl { Wrapper,
    /// H('a') }]` vs `[InstantiatedDecl { Wrapper, H('b') }]` → DISTINCT →
    /// PASSES, with the head staying EXACT (same `Wrapper` identity).
    #[test]
    fn normalize_nodes_discriminates_resolved_nested_generic_args() {
        use rustc_hash::FxHashSet;
        use verter_semantic::analysis::component_meta::ResolvedTypeAnalysis;
        use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;
        use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
        use verter_type_expr::LiteralValue;

        use super::super::core::{PolicyCtx, PolicyRegistry};
        use crate::resolver_core::component_meta::ResolvedTypeRegistryMeta;
        use crate::resolver_core::{
            ComponentMetaQueryEngine, ResolvedDeclarationKind, ResolvedTypeDeclaration,
        };
        use crate::semantic_query::{DeclIdentity, SemanticNodeData};

        let host = VerterHost::new_standalone(HostConfig::default());
        let graph = host.project_type_store().semantic_graph();

        // Intern `Wrapper<'a'>` and `Wrapper<'b'>` argument nodes — an
        // `InstantiationRef` head named `Wrapper` differing ONLY in the
        // nested literal argument.
        let wrapper_base = DeclIdentity {
            canonical_id: Arc::from("/wrapper.ts"),
            whole_hash: Default::default(),
            decl_name: Arc::from("Wrapper"),
        };
        let lit = |text: &str| {
            graph.intern_node_with_scope(
                SemanticNodeData::Literal(LiteralValue::String(text.to_string())),
                scope(),
            )
        };
        let wrapper_of = |arg| {
            graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: wrapper_base.clone(),
                    args: Arc::from([arg]),
                },
                scope(),
            )
        };
        let wrapper_a = wrapper_of(lit("a"));
        let wrapper_b = wrapper_of(lit("b"));

        // Seed the registry so `locate_declaration("Wrapper")` resolves via
        // the registry body (a NON-self-referential ref) and the
        // resolved-head branch is taken — without this the heads fall to the
        // already-discriminating `AnonymousShape` branch and the test would
        // not exercise the fix.
        let registry = vec![ResolvedTypeAnalysis {
            name: "Wrapper".to_string(),
            type_source: verter_type_expr::facts::SourcePosition::Present(
                SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(
                    "WrapperBody".to_string(),
                ))),
            ),
            type_expansion: None,
        }];
        let registry_meta = vec![ResolvedTypeRegistryMeta {
            name: "Wrapper".to_string(),
            declaration: ResolvedTypeDeclaration {
                requested_name: "Wrapper".to_string(),
                declaration_id: None,
                resolved_name: "Wrapper".to_string(),
                canonical_source: "/wrapper.ts".to_string(),
                span: verter_span::Span::default(),
                kind: ResolvedDeclarationKind::TypeAlias,
                text: None,
            },
        }];
        let policy_registry = PolicyRegistry::build(&registry, &registry_meta);
        let empty_idents: FxHashSet<ResolvedRootIdentity> = FxHashSet::default();

        crate::resolver_core::with_bare_host_ctx_for_test(&host, |ctx| {
            let mut engine = ComponentMetaQueryEngine::new(ctx);
            let mut pctx = PolicyCtx {
                registry: &policy_registry,
                engine: &mut engine,
                owner_canonical: "/owner.vue",
                host: &host,
                macro_participating_idents: &empty_idents,
                active_refs: FxHashSet::default(),
                active_refs_max_depth: 0,
            };
            let args_a = NormalizedTypeArgs::normalize_nodes(&[wrapper_a], &mut pctx);
            let args_b = NormalizedTypeArgs::normalize_nodes(&[wrapper_b], &mut pctx);

            // Primary discriminating property: distinct nested resolved args
            // ⇒ distinct cycle-guard keys.
            assert_ne!(
                args_a, args_b,
                "Foo<Wrapper<'a'>> and Foo<Wrapper<'b'>> MUST produce DISTINCT \
                 cycle-guard keys once `Wrapper` resolves — the resolved-head \
                 branch must not drop its positional type arguments (dropping \
                 them collapses both onto `Decl(Wrapper)` and over-cuts one \
                 request's recursion)."
            );

            // The head stays EXACT: both are `InstantiatedDecl` over the SAME
            // `Wrapper` identity (not degraded to a bare structural hash, and
            // not two different heads); only the positional-arg fingerprint
            // differs. This also asserts the head DID resolve (the resolved
            // branch, not the `AnonymousShape` fallback, was taken).
            match (args_a.iter().next(), args_b.iter().next()) {
                (
                    Some(NormalizedTypeArg::InstantiatedDecl {
                        identity: ia,
                        args_fingerprint: fa,
                    }),
                    Some(NormalizedTypeArg::InstantiatedDecl {
                        identity: ib,
                        args_fingerprint: fb,
                    }),
                ) => {
                    assert_eq!(ia, ib, "the resolved head identity must stay EXACT and shared");
                    assert_eq!(ia.decl_name.as_ref(), "Wrapper");
                    assert_ne!(
                        fa, fb,
                        "the positional-arg fingerprints must discriminate 'a' from 'b'"
                    );
                }
                other => panic!(
                    "expected two InstantiatedDecl args over a resolved `Wrapper` head, got {other:?}"
                ),
            };
        });
    }
}
