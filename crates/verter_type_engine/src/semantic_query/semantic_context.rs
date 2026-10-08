//! Interned semantic context: effective options, environment, and one
//! context-owned immutable policy set.
//!
//! Context is interned by exact equality behind a digest; the digest
//! accelerates lookup, not semantic equality. There is one production
//! default policy set. Private projections derive union-order and order-
//! domain from the context — a leaf key cannot be constructed with a
//! policy different from its context's projection.
//!
//! **Ownership.** A [`SemanticContextId`] and an [`OrderDomainId`] OWN
//! their records: each is one pointer, and the last handle dropping frees
//! the record together with its weak deduplication entry (see
//! [`crate::semantic_query_memo::intern_table`]). Neither record retains a
//! child handle. The production context is the one permanent record. A
//! [`SemanticPolicySetId`] is the policy set's exact value — a few bytes —
//! so it needs no table at all.

use std::sync::OnceLock;

use verter_session_query::analysis::types::Hash16;
use verter_session_query::resolution::{EnvHashes, SemanticCompilerOptions};

use super::SemanticNodeId;
use crate::semantic_query_memo::intern_table::{intern_domain, Interned};

/// Owning interned identity of one [`SemanticContext`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticContextId(Interned<SemanticContext>);

/// Identity of one [`SemanticPolicySet`]: the set's exact value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticPolicySetId(SemanticPolicySet);

/// Union-order policy. Production is [`Self::VerterStableV1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticOrderPolicyId {
    VerterStableV1,
}

/// Derived order-domain identity: facts needed to interpret stable
/// identities under a context, not the currently loaded object set. Owns
/// its interned record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderDomainId(Interned<OrderDomainKey>);

/// Closed intersection-policy bundle keyed by purpose. Production carries
/// one mapping; callers cannot supply a contradictory version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct IntersectionPoliciesByPurpose {
    _private: (),
}

/// One immutable policy set, selected when forming a semantic context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticPolicySet {
    pub compatibility_version: u32,
    pub union_order: SemanticOrderPolicyId,
    pub intersection_policies_by_purpose: IntersectionPoliciesByPurpose,
}

impl SemanticPolicySet {
    /// The single production default.
    #[must_use]
    pub const fn production() -> Self {
        Self {
            compatibility_version: 1,
            union_order: SemanticOrderPolicyId::VerterStableV1,
            intersection_policies_by_purpose: IntersectionPoliciesByPurpose { _private: () },
        }
    }

    /// This set's identity.
    #[must_use]
    pub const fn id(self) -> SemanticPolicySetId {
        SemanticPolicySetId(self)
    }
}

impl Default for SemanticPolicySetId {
    fn default() -> Self {
        SemanticPolicySet::production().id()
    }
}

impl SemanticPolicySetId {
    /// The identified policy set.
    #[must_use]
    pub const fn policy_set(self) -> SemanticPolicySet {
        self.0
    }
}

/// Exact semantic context: effective options, resolver/library/project
/// environment, one policy set, and the material R/T/L/J axes (R/T/L on
/// the env bundle, J as `project_identity`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticContext {
    pub effective_semantic_options: SemanticCompilerOptions,
    pub resolver_library_project_environment: EnvHashes,
    pub policy_set: SemanticPolicySetId,
    /// Project-isolation axis (`J`).
    pub project_identity: Hash16,
}

impl SemanticContext {
    /// Production default: default effective options, the canonical
    /// all-zero env bundle (spelled field-by-field — the bundle's
    /// `Default` is reserved for test fixtures), production policy set.
    #[must_use]
    pub fn production() -> Self {
        Self {
            effective_semantic_options: SemanticCompilerOptions::default(),
            resolver_library_project_environment: EnvHashes {
                parse_env_hash: Hash16::default(),
                resolve_env_hash: Hash16::default(),
                type_env_hash: Hash16::default(),
                lib_env_hash: Hash16::default(),
            },
            policy_set: SemanticPolicySetId::default(),
            project_identity: Hash16::default(),
        }
    }

    /// Intern this context by exact equality. The digest is a lookup
    /// accelerator, not a substitute for equality.
    #[must_use]
    pub fn intern(self) -> SemanticContextId {
        SemanticContextId(Interned::new(self))
    }
}

impl SemanticContextId {
    /// The interned context.
    #[must_use]
    pub fn context(&self) -> &SemanticContext {
        self.0.value()
    }

    /// The production default: the one permanent context record.
    #[must_use]
    pub fn production() -> Self {
        static PRODUCTION: OnceLock<SemanticContextId> = OnceLock::new();
        PRODUCTION
            .get_or_init(|| SemanticContext::production().intern())
            .clone()
    }
}

intern_domain!(SemanticContext);
intern_domain!(OrderDomainKey);

/// Private projection: the context's union-order policy.
#[must_use]
pub fn project_union_order(ctx: &SemanticContext) -> SemanticOrderPolicyId {
    ctx.policy_set.policy_set().union_order
}

/// Private projection: the order domain derived from this context.
#[must_use]
pub fn project_order_domain(ctx: &SemanticContext) -> OrderDomainId {
    OrderDomainId(Interned::new(OrderDomainKey {
        env: ctx.resolver_library_project_environment,
        project_identity: ctx.project_identity,
        policy_set: ctx.policy_set,
    }))
}

/// Leaf key for a `SemanticUnionMembers`-style view. The only constructor
/// projects policy and domain from the context, so an independently
/// supplied policy is unrepresentable. The key owns its order domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SemanticUnionMembersKey {
    union: SemanticNodeId,
    policy: SemanticOrderPolicyId,
    domain: OrderDomainId,
}

impl SemanticUnionMembersKey {
    /// Project the leaf identity from `union` and `ctx`. No public
    /// constructor accepts a policy override.
    #[must_use]
    pub fn from_context(union: SemanticNodeId, ctx: &SemanticContext) -> Self {
        Self {
            union,
            policy: project_union_order(ctx),
            domain: project_order_domain(ctx),
        }
    }

    #[must_use]
    pub fn union(&self) -> SemanticNodeId {
        self.union
    }

    #[must_use]
    pub fn policy(&self) -> SemanticOrderPolicyId {
        self.policy
    }

    #[must_use]
    pub fn domain(&self) -> &OrderDomainId {
        &self.domain
    }
}

/// The interned content of one [`OrderDomainId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderDomainKey {
    env: EnvHashes,
    project_identity: Hash16,
    policy_set: SemanticPolicySetId,
}

/// Whether a context with `ctx`'s exact content is resident, and whether
/// the order domain it projects is.
#[cfg(test)]
pub(crate) fn context_records_resident(ctx: &SemanticContext) -> (bool, bool) {
    use crate::semantic_query_memo::intern_table::InternDomain;
    let domain = OrderDomainKey {
        env: ctx.resolver_library_project_environment,
        project_identity: ctx.project_identity,
        policy_set: ctx.policy_set,
    };
    (
        SemanticContext::index().get(ctx).is_some(),
        OrderDomainKey::index().get(&domain).is_some(),
    )
}
