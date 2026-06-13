//! The compiler-side blessed carrier downcast (D-m).
//!
//! [`CarrierCompilerCtx`] is one of the THREE blessed homes of the raw
//! `verter_language` carrier downcast (`__carrier_downcast_ref` /
//! `__carrier_downcast_arc`) — the others are
//! `verter_language::parse_artifact` (the helpers themselves) and
//! `verter_session/src/framework/ctx.rs` (the session-side wrapper). A
//! carrier compiler reaches its OWN typed parse carrier back out of the
//! type-erased [`FrameworkParseArtifact`] through this ctx; nothing else
//! in the compiler calls the raw downcast (pinned by the
//! `carrier_downcast_confined_to_owning_adapter` architecture guard).
//!
//! The ctx holds the adapter's [`CarrierAccessToken`] — RECEIVED from
//! `verter_language`'s carrier-row registration channel
//! (`LanguageRegistry::__built_in_with_carrier_tokens`), never minted
//! here (D-ba: `verter_language` is the sole minting authority).

use std::sync::Arc;

use verter_language::{CarrierAccessToken, CarrierParse, FrameworkParseArtifact};

/// Compiler-side token-gated typed carrier access.
///
/// Wraps a single adapter's [`CarrierAccessToken`]. A carrier compiler
/// (e.g. the Vue bridge) holds one ctx and reaches its own typed carrier
/// through [`Self::carrier_for`] / [`Self::carrier_for_arc`].
#[derive(Debug, Clone)]
pub struct CarrierCompilerCtx {
    token: CarrierAccessToken,
}

impl CarrierCompilerCtx {
    /// Build the ctx over an adapter's RECEIVED carrier registration proof.
    ///
    /// The token must be the proof `verter_language` minted for the
    /// adapter whose carrier this ctx opens — a mismatched token simply
    /// returns `None` from the downcast (the adapter-id gate inside the
    /// raw helper), never a forged access.
    #[must_use]
    pub fn new(token: CarrierAccessToken) -> Self {
        Self { token }
    }

    /// The adapter id this ctx grants carrier access for.
    #[must_use]
    pub fn adapter_id(&self) -> &verter_language::FrameworkAdapterId {
        self.token.adapter_id()
    }

    /// The typed carrier of type `T` behind `artifact`, by reference, or
    /// `None`.
    ///
    /// Returns `None` cleanly when the artifact belongs to another adapter
    /// (the token's adapter-id gate) or the erased payload is not a `T`.
    #[must_use]
    pub fn carrier_for<'a, T: CarrierParse>(
        &self,
        artifact: &'a FrameworkParseArtifact,
    ) -> Option<&'a T> {
        verter_language::__carrier_downcast_ref::<T>(artifact, &self.token)
    }

    /// The typed carrier of type `T` behind `artifact`, as a shared
    /// handle, or `None`.
    #[must_use]
    pub fn carrier_for_arc<T: CarrierParse>(
        &self,
        artifact: &FrameworkParseArtifact,
    ) -> Option<Arc<T>> {
        verter_language::__carrier_downcast_arc::<T>(artifact, &self.token)
    }
}

/// Receive the Vue carrier registration proof from `verter_language`.
///
/// The token is minted ONCE inside `verter_language` during built-in
/// carrier-row construction; this is the compiler-side receipt site (the
/// Vue bridge's ctx is built from it). Returns the proof for the `.vue`
/// carrier row.
///
/// CRATE-PRIVATE: a token-returning receipt function must NOT be public
/// API — only the in-crate Vue bridge constructs its `CarrierCompilerCtx`
/// from this proof, so no downstream crate can receive a
/// `CarrierAccessToken` through it (the token-receipt confinement
/// property; pinned by `carrier_access_token_minted_only_in_verter_language`).
#[must_use]
pub(crate) fn receive_vue_carrier_token() -> CarrierAccessToken {
    let (_registry, tokens) = verter_language::LanguageRegistry::__built_in_with_carrier_tokens();
    // The built-in carrier rows are returned in row order: `.vue` first,
    // then `.svelte`. The Vue proof is the one whose adapter id is Vue —
    // selected by identity, not by positional assumption.
    tokens
        .into_iter()
        .find(|token| token.adapter_id().is_vue())
        .expect("the built-in registry mints a Vue carrier registration proof")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;
    use verter_language::{FrameworkAdapterId, FrameworkParseCommon, LanguageId};

    #[derive(Debug)]
    struct FixtureCarrier {
        value: u32,
    }
    impl CarrierParse for FixtureCarrier {
        fn __verter_as_any(&self) -> &dyn Any {
            self
        }
        fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
            self
        }
    }

    fn vue_artifact(value: u32) -> FrameworkParseArtifact {
        FrameworkParseArtifact::new(
            FrameworkAdapterId::vue(),
            LanguageId::new("vue"),
            1,
            FrameworkParseCommon::default(),
            Arc::new(FixtureCarrier { value }),
        )
    }

    #[test]
    fn vue_ctx_opens_its_own_carrier() {
        let ctx = CarrierCompilerCtx::new(receive_vue_carrier_token());
        assert!(ctx.adapter_id().is_vue());
        let artifact = vue_artifact(11);
        let carrier = ctx
            .carrier_for::<FixtureCarrier>(&artifact)
            .expect("the Vue ctx opens a Vue artifact's carrier");
        assert_eq!(carrier.value, 11);
        let arc = ctx
            .carrier_for_arc::<FixtureCarrier>(&artifact)
            .expect("the Arc form opens it too");
        assert_eq!(arc.value, 11);
    }

    #[test]
    fn wrong_adapter_artifact_downcast_returns_none() {
        let ctx = CarrierCompilerCtx::new(receive_vue_carrier_token());
        // An artifact stamped for another adapter is gated out by id.
        let foreign = FrameworkParseArtifact::new(
            FrameworkAdapterId::new("svelte"),
            LanguageId::new("svelte"),
            1,
            FrameworkParseCommon::default(),
            Arc::new(FixtureCarrier { value: 3 }),
        );
        assert!(
            ctx.carrier_for::<FixtureCarrier>(&foreign).is_none(),
            "the Vue token must NOT open a non-Vue artifact's carrier"
        );
    }
}
