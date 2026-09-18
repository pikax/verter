//! Interned semantic context: effective options, environment, and one
//! context-owned immutable policy set.
//!
//! Context is interned by exact equality behind a digest; the digest
//! accelerates lookup, not semantic equality. There is one production
//! default policy set. Private projections derive union-order and order-
//! domain from the context — a leaf key cannot be constructed with a
//! policy different from its context's projection.

use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHasher};
use verter_semantic::analysis::Hash16;
use verter_semantic::resolver_core::{EnvHashes, SemanticCompilerOptions};

use super::SemanticNodeId;

/// Interned identity of one [`SemanticContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticContextId(u32);

/// Interned identity of one [`SemanticPolicySet`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticPolicySetId(u32);

/// Union-order policy. Production is [`Self::VerterStableV1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticOrderPolicyId {
    VerterStableV1,
}

/// Derived order-domain identity: facts needed to interpret stable
/// identities under a context, not the currently loaded object set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderDomainId(u32);

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

    /// Intern this set, returning its id.
    #[must_use]
    pub fn intern(self) -> SemanticPolicySetId {
        intern_policy_set(self)
    }
}

impl Default for SemanticPolicySetId {
    fn default() -> Self {
        static PRODUCTION: OnceLock<SemanticPolicySetId> = OnceLock::new();
        *PRODUCTION.get_or_init(|| SemanticPolicySet::production().intern())
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
        intern_context(self)
    }
}

/// Private projection: the context's union-order policy.
#[must_use]
pub fn project_union_order(ctx: &SemanticContext) -> SemanticOrderPolicyId {
    lookup_policy_set(ctx.policy_set).union_order
}

/// Private projection: the order domain derived from this context.
#[must_use]
pub fn project_order_domain(ctx: &SemanticContext) -> OrderDomainId {
    intern_order_domain(OrderDomainKey {
        env: ctx.resolver_library_project_environment,
        project_identity: ctx.project_identity,
        policy_set: ctx.policy_set,
    })
}

/// Leaf key for a `SemanticUnionMembers`-style view. The only constructor
/// projects policy and domain from the context, so an independently
/// supplied policy is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    pub fn union(self) -> SemanticNodeId {
        self.union
    }

    #[must_use]
    pub fn policy(self) -> SemanticOrderPolicyId {
        self.policy
    }

    #[must_use]
    pub fn domain(self) -> OrderDomainId {
        self.domain
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct OrderDomainKey {
    env: EnvHashes,
    project_identity: Hash16,
    policy_set: SemanticPolicySetId,
}

struct InternTable<T> {
    by_hash: FxHashMap<u64, Vec<u32>>,
    items: Vec<T>,
}

impl<T> Default for InternTable<T> {
    fn default() -> Self {
        Self {
            by_hash: FxHashMap::default(),
            items: Vec::new(),
        }
    }
}

fn digest<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

fn intern_value<T: Clone + Eq + Hash>(table: &Mutex<InternTable<T>>, value: T) -> u32 {
    let hash = digest(&value);
    let mut guard = table.lock();
    if let Some(ids) = guard.by_hash.get(&hash) {
        for &id in ids {
            if guard.items[id as usize] == value {
                return id;
            }
        }
    }
    let id = u32::try_from(guard.items.len()).expect("intern table overflow");
    guard.items.push(value);
    guard.by_hash.entry(hash).or_default().push(id);
    id
}

fn policy_table() -> &'static Mutex<InternTable<SemanticPolicySet>> {
    static TABLE: OnceLock<Mutex<InternTable<SemanticPolicySet>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(InternTable::default()))
}

fn intern_policy_set(set: SemanticPolicySet) -> SemanticPolicySetId {
    SemanticPolicySetId(intern_value(policy_table(), set))
}

fn lookup_policy_set(id: SemanticPolicySetId) -> SemanticPolicySet {
    let guard = policy_table().lock();
    guard
        .items
        .get(id.0 as usize)
        .copied()
        .unwrap_or_else(SemanticPolicySet::production)
}

fn context_table() -> &'static Mutex<InternTable<SemanticContext>> {
    static TABLE: OnceLock<Mutex<InternTable<SemanticContext>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(InternTable::default()))
}

fn intern_context(ctx: SemanticContext) -> SemanticContextId {
    SemanticContextId(intern_value(context_table(), ctx))
}

fn lookup_context(id: SemanticContextId) -> Option<SemanticContext> {
    let guard = context_table().lock();
    guard.items.get(id.0 as usize).cloned()
}

fn intern_order_domain(key: OrderDomainKey) -> OrderDomainId {
    static TABLE: OnceLock<Mutex<InternTable<OrderDomainKey>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(InternTable::default()));
    OrderDomainId(intern_value(table, key))
}

impl SemanticContextId {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Lookup the interned context. `None` if the id was never interned.
    #[must_use]
    pub fn lookup(self) -> Option<SemanticContext> {
        lookup_context(self)
    }

    /// Intern the production default.
    #[must_use]
    pub fn production() -> Self {
        SemanticContext::production().intern()
    }
}

impl SemanticPolicySetId {
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}
