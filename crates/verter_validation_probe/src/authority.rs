//! Durable capability ids a probe cell cites as its owner.
//!
//! The id is the whole contract on this side. The framework, dimensions,
//! atoms and owning implementation behind each id live in the
//! validation-authority catalog and are checked by the authority validator; this
//! enum deliberately carries no framework or dimension logic, so a citation
//! can never be widened by code.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A closed set of citable authorities. An unknown id is unrepresentable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Authority {
    /// The public typed compile-request route: callability and typed refusal.
    #[serde(rename = "compiler.public-request-route")]
    CompilerPublicRequestRoute,
    /// The Vue runtime-client compiler product.
    #[serde(rename = "vue.runtime-client-product")]
    VueRuntimeClientProduct,
    /// The Svelte runtime-client compiler product.
    #[serde(rename = "svelte.runtime-client-product")]
    SvelteRuntimeClientProduct,
    /// The compiler's equivalent-work ledger.
    #[serde(rename = "compiler.equivalent-work-ledger")]
    CompilerEquivalentWorkLedger,
    /// The compiler's physical-execution accounting.
    #[serde(rename = "compiler.physical-execution")]
    CompilerPhysicalExecution,
    /// The native addon's memory-audit snapshot.
    #[serde(rename = "native.memory-audit-snapshot")]
    NativeMemoryAuditSnapshot,
}

impl Authority {
    /// Every citable authority, in declaration order.
    pub const ALL: [Authority; 6] = [
        Authority::CompilerPublicRequestRoute,
        Authority::VueRuntimeClientProduct,
        Authority::SvelteRuntimeClientProduct,
        Authority::CompilerEquivalentWorkLedger,
        Authority::CompilerPhysicalExecution,
        Authority::NativeMemoryAuditSnapshot,
    ];

    /// The serialized citation id.
    pub const fn id(self) -> &'static str {
        match self {
            Authority::CompilerPublicRequestRoute => "compiler.public-request-route",
            Authority::VueRuntimeClientProduct => "vue.runtime-client-product",
            Authority::SvelteRuntimeClientProduct => "svelte.runtime-client-product",
            Authority::CompilerEquivalentWorkLedger => "compiler.equivalent-work-ledger",
            Authority::CompilerPhysicalExecution => "compiler.physical-execution",
            Authority::NativeMemoryAuditSnapshot => "native.memory-audit-snapshot",
        }
    }
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.id())
    }
}
