//! The TypeInfo capability handshake: the read-only answer a client
//! discovers the host's wire contract from before it sends a request.
//!
//! It reports the server's current and accepted schema versions (the same
//! closed set an `UnknownSchemaVersion` error echoes) and every framework
//! profile this host admits (schema 11). A `ComponentSelector` names a
//! profile by the reference advertised here; the framework-surface executor
//! admits exactly these references, through the same registry.

use verter_protocol::typeinfo::graph::{
    TypeInfoCapabilityHandshakeRequest, TypeInfoCapabilityHandshakeResponse,
    TYPEINFO_GRAPH_SCHEMA_VERSION,
};

use crate::typeinfo::request_validation::SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS;
use crate::VerterHost;

impl VerterHost {
    /// Answer the TypeInfo capability handshake.
    ///
    /// The supported versions are the closed set the request validator
    /// accepts; the client intersects it with its own. The framework
    /// profiles are the registry's admitted profiles in digest order —
    /// already filtered by the host's `FrameworkOptions`, so a disabled
    /// framework is neither advertised nor admitted. The request's
    /// `client_version` selects nothing: the answer is the host's contract.
    #[must_use]
    pub fn typeinfo_capabilities(
        &self,
        _request: &TypeInfoCapabilityHandshakeRequest,
    ) -> TypeInfoCapabilityHandshakeResponse {
        TypeInfoCapabilityHandshakeResponse {
            server_version: TYPEINFO_GRAPH_SCHEMA_VERSION,
            server_supported_versions: SUPPORTED_TYPEINFO_GRAPH_SCHEMA_VERSIONS.to_vec(),
            framework_profiles: self
                .framework_registry()
                .advertised_profiles()
                .map(|profile| profile.advertisement())
                .collect(),
        }
    }

    /// The advertised reference of the profile admitted for `adapter_id`'s
    /// registration, if that adapter is registered.
    ///
    /// A client-side convenience for a caller that starts from a file it has
    /// already classified (the MCP tool takes a path) and must name the
    /// matching advertised profile on the wire. TypeInfo dispatch never uses
    /// it: the executor admits the reference through the profile index.
    #[must_use]
    pub fn advertised_framework_profile_for_adapter(
        &self,
        adapter_id: &crate::framework::FrameworkAdapterId,
    ) -> Option<verter_protocol::typeinfo::graph::FrameworkProfileRef> {
        self.framework_registry()
            .profile_for_adapter(adapter_id)
            .map(crate::framework::registry::RegisteredFrameworkProfile::wire_ref)
    }
}
