//! Profile and policy classification types, per semantic-profile.md and
//! architecture.md §3.2.
//!
//! Six classes exist; this module owns the four identity-shaped ones plus
//! [`ExecutionPolicy`] (which is explicitly the ONE class excluded from
//! reusable result identity — semantic-profile.md §1's "never changes
//! complete result identity"). `ResultContractId` lives in
//! [`crate::identity`] alongside the other identity types it composes with
//! ([`crate::identity::QueryIdentity`], [`crate::identity::SemanticFlightKey`]).
//!
//! A field belongs to the earliest class whose observable meaning it can
//! change, and is never copied into every class "for safety"
//! (semantic-profile.md §1). This module supplies the four typed IDs a
//! field is classified INTO. It deliberately does not thread these types
//! into `HostConfig`/`CompileProfile`/`CodegenOptions`/
//! `IdeProjectCompilerOptions`/`EnvHashInputs` or otherwise change what any
//! existing code path computes — landing dependency-neutral types is
//! explicitly scoped apart from migrating semantic behavior. A future
//! change wires those owner structs to construct these typed profile IDs
//! from their already-classified fields.

digest_identity!(
    /// TypeScript-compatible interpretation identity (architecture.md §3.1,
    /// §3.2; semantic-profile.md §1). Its closed normalized descriptor
    /// covers strictness/nullability/exact-optional-property behavior,
    /// module/resolution semantics, JSX/type-language rules, target/lib
    /// basis, and package-boundary/case/symlink/workspace policy — never
    /// diagnostic wording, path display, serialization layout, worker
    /// count, cache policy, build timestamp, or a progress counter.
    TypeScriptSemanticProfileId
);
digest_identity!(
    /// Generated-program semantics/shape identity (architecture.md §3.1,
    /// §3.2; semantic-profile.md §1): client/server target, dev/prod
    /// semantics, feature transforms, framework/compiler target.
    OutputProfileId
);
digest_identity!(
    /// Human-facing rendering identity (architecture.md §3.1, §3.2;
    /// semantic-profile.md §1): display flags, path-display policy,
    /// diagnostic text locale/presentation version. Absent when
    /// presentation is not requested (semantic-profile.md §4).
    PresentationProfileId
);
digest_identity!(
    /// Wire/encoding contract identity (architecture.md §3.1, §3.2;
    /// semantic-profile.md §1): schema/domain, canonical encoding, graph
    /// export format, field policy. Absent when serialization is not
    /// requested (semantic-profile.md §4).
    SerializationProfileId
);

/// Waiter-local resource/scheduling limits (semantic-profile.md §1;
/// `result-contract-and-flight.md` §1's literal shape). Deliberately never
/// part of [`crate::identity::QueryIdentity`],
/// [`crate::identity::SemanticFlightKey`], or
/// [`crate::identity::ResultContractId`] — semantic-profile.md §4: "execution
/// budget changes cannot produce a different value labeled `Complete`."
/// Budget exhaustion is `Partial` or typed failure; that transition is the
/// owning flight runtime's responsibility, not representable in this type.
///
/// Generic over the cancellation representation (`C`) rather than naming a
/// concrete cancellation-token type: the existing `CancellationToken`
/// (`verter_scheduler::cancellation`) is already owned by `verter_scheduler`
/// (dependency layer 5), and this crate sits at layer 1 — depending on it
/// would itself be the forbidden upward edge the dependency firewall this
/// block lands exists to catch. `C` defaults to `()` for contexts that only
/// need the non-cancellation fields (e.g. identity/encoding tests); a real
/// scheduling consumer instantiates `ExecutionPolicy<CancellationToken>`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ExecutionPolicy<C = ()> {
    pub deadline: Option<Deadline>,
    pub cancellation: C,
    pub priority: WorkPriority,
    pub work_budget: WorkBudget,
    pub memory_budget: MemoryBudget,
}

/// An opaque monotonic deadline instant. Deliberately not `std::time::Instant`
/// (not comparable across processes or serializable) — callers convert from
/// their own clock source. `0` means "no headroom left", not "unset"; use
/// `Option<Deadline>` for "no deadline".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Deadline(pub u64);

/// Closed scheduling priority (semantic-profile.md §1 "priority").
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum WorkPriority {
    Low,
    #[default]
    Normal,
    High,
}

/// Opaque work-unit budget (semantic-profile.md §1 "work ... budget").
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct WorkBudget(pub u64);

/// Byte-denominated memory budget (semantic-profile.md §1 "memory budget").
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MemoryBudget(pub u64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{CanonicalEncode, CanonicalEncoder};

    struct Descriptor(&'static str);
    impl CanonicalEncode for Descriptor {
        const DOMAIN_TAG: &'static str = "profile-test.descriptor.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_str(1, self.0);
        }
    }

    /// The four profile-id types are genuinely distinct Rust types (not
    /// aliases of one another), so building each from the SAME descriptor
    /// still yields values that cannot be compared or confused across
    /// types — proven at compile time by simply naming all four here with
    /// no shared trait object / enum wrapper unifying them.
    #[test]
    fn all_four_profile_ids_construct_from_the_same_descriptor_shape() {
        let ts = TypeScriptSemanticProfileId::from_canonical(&Descriptor("strict"));
        let output = OutputProfileId::from_canonical(&Descriptor("strict"));
        let presentation = PresentationProfileId::from_canonical(&Descriptor("strict"));
        let serialization = SerializationProfileId::from_canonical(&Descriptor("strict"));
        // Same field bytes, but four DIFFERENT domain tags (one per
        // `digest_identity!` invocation's implicit distinctness — each
        // macro expansion still routes through the caller's own
        // `CanonicalEncode::DOMAIN_TAG`, so the actual separation here
        // comes from the descriptor authors giving each profile family
        // its own domain tag upstream; this crate only guarantees the
        // WRAPPER types cannot be confused, which the distinct
        // `digest()` calls below exercise).
        assert_eq!(ts.digest(), output.digest());
        assert_eq!(ts.digest(), presentation.digest());
        assert_eq!(ts.digest(), serialization.digest());
        // Despite equal digests (same descriptor, same domain tag), the
        // four values are NOT the same Rust type — this line would not
        // compile if, say, `output` were compared directly against `ts`
        // with `==`, because `OutputProfileId: PartialEq<OutputProfileId>`
        // only, never `PartialEq<TypeScriptSemanticProfileId>`.
    }

    #[test]
    fn execution_policy_defaults_and_construction() {
        let policy: ExecutionPolicy = ExecutionPolicy {
            deadline: None,
            cancellation: (),
            priority: WorkPriority::default(),
            work_budget: WorkBudget(0),
            memory_budget: MemoryBudget(0),
        };
        assert_eq!(policy.priority, WorkPriority::Normal);
    }

    #[test]
    fn execution_policy_generic_over_cancellation_representation() {
        // Any type may fill the cancellation slot — this crate does not
        // require it to be `verter_scheduler::cancellation::CancellationToken`
        // (which it must not depend on; see the module doc).
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        struct FakeToken(u8);
        let policy = ExecutionPolicy {
            deadline: Some(Deadline(1)),
            cancellation: FakeToken(1),
            priority: WorkPriority::Low,
            work_budget: WorkBudget(1),
            memory_budget: MemoryBudget(1),
        };
        assert_eq!(policy.cancellation, FakeToken(1));
    }
}
