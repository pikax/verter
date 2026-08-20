//! Profile and policy classification types (`semantic-profile.md`).
//!
//! Four identity-shaped profile IDs plus [`ExecutionPolicy`] — the one
//! class excluded from reusable result identity ("never changes complete
//! result identity"). `ResultContractId` lives in [`crate::identity`].
//! A field belongs to the earliest class whose observable meaning it can
//! change, and is never copied into every class "for safety". These IDs
//! are not threaded into `HostConfig`/`CompileProfile`/`CodegenOptions`.

digest_identity!(
    /// TypeScript interpretation identity: strictness/nullability, module
    /// resolution, JSX/type-language, target/lib, package-boundary policy.
    /// Never diagnostic wording, path display, serialization, worker
    /// count, cache policy, or a timestamp.
    TypeScriptSemanticProfileId
);
digest_identity!(
    /// Generated-program semantics: client/server, dev/prod, feature
    /// transforms, framework/compiler target.
    OutputProfileId
);
digest_identity!(
    /// Human-facing rendering: display flags, path policy, diagnostic
    /// locale. Absent when presentation is not requested.
    PresentationProfileId
);
digest_identity!(
    /// Wire/encoding contract: schema, canonical encoding, graph export,
    /// field policy. Absent when serialization is not requested.
    SerializationProfileId
);

/// Waiter-local resource/scheduling limits. Never part of
/// [`crate::identity::QueryIdentity`],
/// [`crate::identity::SemanticFlightKey`], or
/// [`crate::identity::ResultContractId`]: budget changes cannot produce a
/// different value labeled `Complete`. Exhaustion is `Partial` or typed
/// failure in the flight runtime.
///
/// Generic over cancellation `C` so this layer-1 crate does not depend on
/// `verter_scheduler::CancellationToken` (layer 5). Defaults to `()`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ExecutionPolicy<C = ()> {
    pub deadline: Option<Deadline>,
    pub cancellation: C,
    pub priority: WorkPriority,
    pub work_budget: WorkBudget,
    pub memory_budget: MemoryBudget,
}

/// Opaque monotonic deadline. Not `std::time::Instant` (not comparable
/// across processes). `0` means no headroom left, not unset — use
/// `Option<Deadline>` for "no deadline".
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Deadline(pub u64);

/// Closed scheduling priority.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum WorkPriority {
    Low,
    #[default]
    Normal,
    High,
}

/// Opaque work-unit budget.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct WorkBudget(pub u64);

/// Byte-denominated memory budget.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
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

    #[test]
    fn all_four_profile_ids_construct_from_the_same_descriptor_shape() {
        let ts = TypeScriptSemanticProfileId::from_canonical(&Descriptor("strict"));
        let output = OutputProfileId::from_canonical(&Descriptor("strict"));
        let presentation = PresentationProfileId::from_canonical(&Descriptor("strict"));
        let serialization = SerializationProfileId::from_canonical(&Descriptor("strict"));
        // Same descriptor bytes / domain tag → equal digests. The four
        // wrapper types still cannot `==` each other (`PartialEq` is
        // per-newtype).
        assert_eq!(ts.digest(), output.digest());
        assert_eq!(ts.digest(), presentation.digest());
        assert_eq!(ts.digest(), serialization.digest());
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
