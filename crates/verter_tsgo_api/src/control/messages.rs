//! The versioned control-protocol wire contract: message types + the version
//! gate.
//!
//! The control protocol is JSON-RPC 2.0 over same-user local IPC (a Windows
//! named pipe / a Unix-domain socket). It is the SEMANTIC attach surface a
//! relay-shim exposes to a `verter_lsp` control client — NOT a raw arbitrary
//! LSP tunnel: the methods below are the entire vocabulary, and each maps to a
//! specific gated relay op. The method-name strings and message shapes are the
//! STABLE wire contract a later editor package wraps; they are VERSIONED from
//! message one via [`PROTOCOL_VERSION`], checked in [`verify_hello`].
//!
//! Wire field naming is camelCase throughout (the JSON-RPC / LSP idiom), so a
//! consuming TypeScript client reads the fields verbatim.

use serde::{Deserialize, Serialize};

/// The control-protocol version. Bumped on ANY breaking change to a method
/// name, param, or result shape below. A [`HelloParams::protocol`] that does
/// not equal this value is refused in [`verify_hello`] (fail closed, no
/// attach).
pub const PROTOCOL_VERSION: u32 = 1;

/// `verter/hello` — the mandatory first request: version + nonce handshake.
pub const METHOD_HELLO: &str = "verter/hello";
/// `verter/waitInitialized` — block until the relayed editor→tsgo `initialize`
/// response has passed, capturing the in-band `serverInfo.version` witness.
pub const METHOD_WAIT_INITIALIZED: &str = "verter/waitInitialized";
/// `verter/carrierDidOpenSynced` — inject a carrier overlay + sync barrier.
pub const METHOD_CARRIER_DID_OPEN_SYNCED: &str = "verter/carrierDidOpenSynced";
/// `verter/carrierDidChangeSynced` — update a carrier overlay + sync barrier.
pub const METHOD_CARRIER_DID_CHANGE_SYNCED: &str = "verter/carrierDidChangeSynced";
/// `verter/carrierDidClose` — retract a carrier overlay.
pub const METHOD_CARRIER_DID_CLOSE: &str = "verter/carrierDidClose";
/// `verter/initializeApiSession` — mint an `--api` session, return its endpoint.
pub const METHOD_INITIALIZE_API_SESSION: &str = "verter/initializeApiSession";
/// `verter/detach` — retract carriers (optional) and tear the control session down.
pub const METHOD_DETACH: &str = "verter/detach";
/// `verter/status` — a liveness / state snapshot.
pub const METHOD_STATUS: &str = "verter/status";
/// `verter/fatal` — a server→client NOTIFICATION: the shim/engine is going away.
pub const METHOD_FATAL: &str = "verter/fatal";

/// JSON-RPC error code: `verter/hello` protocol version mismatch (fail closed).
pub const ERROR_PROTOCOL_MISMATCH: i64 = -32010;
/// JSON-RPC error code: `verter/hello` nonce mismatch (rendezvous verification).
pub const ERROR_NONCE_MISMATCH: i64 = -32011;
/// JSON-RPC error code: a control method invoked before a successful `verter/hello`.
pub const ERROR_NOT_AUTHENTICATED: i64 = -32012;
/// JSON-RPC error code: a malformed control-protocol payload.
pub const ERROR_MALFORMED_PAYLOAD: i64 = -32013;
/// JSON-RPC error code: a control op failed against the relay (injection / session).
pub const ERROR_CONTROL_OP_FAILED: i64 = -32014;

/// `verter/hello` params: the version + rendezvous nonce + a client label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    /// The client's control-protocol version (checked against [`PROTOCOL_VERSION`]).
    pub protocol: u32,
    /// The rendezvous nonce the client read from the shim's advertisement.
    pub nonce: String,
    /// A free-form client identifier (e.g. `"verter_lsp"`), for diagnostics.
    pub client: String,
}

/// The confirmed control-server capabilities returned from a successful hello.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ControlCapabilities {
    /// Carrier overlay injection (didOpen/didChange/didClose) is available.
    pub carrier_injection: bool,
    /// `--api` session minting is available.
    pub api_session: bool,
    /// The initialized-witness barrier is available.
    pub wait_initialized: bool,
}

/// `verter/hello` result: the accepted version, session id, wire pin, editor
/// session generation, and the confirmed capability set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResult {
    /// The control-protocol version the server speaks ([`PROTOCOL_VERSION`]).
    pub protocol: u32,
    /// The opaque control-session id the server minted for this connection.
    pub session_id: String,
    /// The `--api` wire pin the shim's engine cleared (the codec fingerprint).
    pub wire_pin: u64,
    /// The shim's editor-session generation (rendezvous binding witness).
    pub editor_session_generation: u64,
    /// The capabilities the server confirmed.
    pub capabilities: ControlCapabilities,
}

/// `verter/waitInitialized` result: the in-band `initialize` witness the relay
/// captured when the editor→tsgo `initialize` response passed through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WaitInitializedResult {
    /// The engine `serverInfo.version` the relay observed in the `initialize`
    /// response (`None` if the server reported none — the gate handles that).
    pub server_info_version: Option<String>,
    /// The JSON-RPC id of the editor's `initialize` request the relay observed.
    pub observed_initialize_id: serde_json::Value,
    /// The `rootUri` the editor sent in `initialize` (the workspace witness).
    pub root_uri: Option<String>,
    /// The `workspaceFolders` the editor sent in `initialize`, if any.
    pub workspace_folders: Option<serde_json::Value>,
}

/// `verter/carrierDidOpenSynced` params: an off-disk carrier overlay to inject.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierDidOpenSyncedParams {
    /// The carrier URI (`file://…`).
    pub uri: String,
    /// The LSP language id (`typescript` / `typescriptreact`).
    pub language_id: String,
    /// The document version.
    pub version: i64,
    /// The full carrier text.
    pub text: String,
}

/// `verter/carrierDidChangeSynced` params: a full-content carrier overlay update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarrierDidChangeSyncedParams {
    /// The carrier URI.
    pub uri: String,
    /// The new document version.
    pub version: i64,
    /// The full replacement text.
    pub text: String,
}

/// `verter/carrierDidClose` params: a carrier overlay to retract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierDidCloseParams {
    /// The carrier URI to close.
    pub uri: String,
}

/// `verter/initializeApiSession` result: the server-minted `--api` endpoint.
/// Exactly one of [`Self::pipe_name`] (Windows named pipe) or
/// [`Self::socket_path`] (Unix-domain socket) is set — the caller connects it
/// directly with the crate's attach client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeApiSessionResult {
    /// The Windows named-pipe path, when the shim runs on Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipe_name: Option<String>,
    /// The Unix-domain-socket path, when the shim runs on a Unix host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<String>,
    /// The `--api` wire pin (the codec fingerprint the engine cleared).
    pub wire_pin: u64,
    /// The opaque-handle kind the `--api` wire uses (`"integer"`).
    pub handle_kind: String,
}

impl InitializeApiSessionResult {
    /// The minted endpoint path, whichever platform variant is set (the value
    /// the caller passes verbatim to the `--api` attach connect).
    #[must_use]
    pub fn endpoint(&self) -> Option<&str> {
        self.pipe_name.as_deref().or(self.socket_path.as_deref())
    }
}

/// `verter/detach` params: whether to retract Verter's carriers on teardown.
///
/// [`Self::close_carriers`] is a TRI-STATE optional flag so the server FAILS CLOSED
/// on an unspecified value: `None` — an omitted field, OR a malformed body the server
/// maps to this default — means "unspecified" and RETRACTS; `Some(false)` is the ONLY
/// opt-out (deliberately leave the overlays open); `Some(true)` retracts. The wire
/// field stays the camelCase boolean `closeCarriers`, so an EXPLICIT sender is
/// unaffected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DetachParams {
    /// Whether to retract every carrier Verter opened before tearing the session
    /// down. `None` (omitted / unspecified, or a malformed body) fails CLOSED — the
    /// server retracts; `Some(false)` is the ONLY opt-out; `Some(true)` retracts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_carriers: Option<bool>,
}

/// A minimal ack for the carrier lifecycle + detach methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlAck {
    /// Whether the op succeeded.
    pub ok: bool,
}

/// `verter/status` result: a control-session state snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusResult {
    /// The control-protocol version the server speaks.
    pub protocol: u32,
    /// Whether a successful `verter/hello` has completed on this connection.
    pub hello_completed: bool,
    /// Whether the relay has observed the editor→tsgo `initialize` response.
    pub initialized: bool,
    /// How many carrier overlays are currently open on this control session.
    pub open_carriers: u32,
    /// Whether an `--api` session has been minted on this control session.
    pub api_session_active: bool,
}

/// Why the shim/engine is going away — the `verter/fatal` notification reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FatalReason {
    /// The real `tsgo` engine exited (the editor's relayed `exit`, or a crash).
    ServerExit,
    /// The stdio relay stopped pumping (editor disconnect / stream error).
    RelayDeath,
    /// A control-protocol version/nonce mismatch was detected.
    ProtocolMismatch,
}

/// `verter/fatal` notification params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FatalParams {
    /// The fatal reason.
    pub reason: FatalReason,
    /// A human-readable detail string.
    pub detail: String,
}

/// Why a `verter/hello` was refused. Mapped by the server to a typed JSON-RPC
/// error ([`ERROR_PROTOCOL_MISMATCH`] / [`ERROR_NONCE_MISMATCH`]); kept as a
/// pure enum so the gate is unit-testable without a live connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelloRejection {
    /// The client's protocol version does not match [`PROTOCOL_VERSION`].
    ProtocolMismatch {
        /// The version the server requires.
        expected: u32,
        /// The version the client presented.
        got: u32,
    },
    /// The client presented a nonce that does not match the shim's advertisement.
    NonceMismatch,
}

impl HelloRejection {
    /// The JSON-RPC error code this rejection maps to.
    #[must_use]
    pub fn error_code(&self) -> i64 {
        match self {
            HelloRejection::ProtocolMismatch { .. } => ERROR_PROTOCOL_MISMATCH,
            HelloRejection::NonceMismatch => ERROR_NONCE_MISMATCH,
        }
    }

    /// A human-readable rejection message.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            HelloRejection::ProtocolMismatch { expected, got } => format!(
                "verter control protocol mismatch: server speaks {expected}, client sent {got}"
            ),
            HelloRejection::NonceMismatch => {
                "verter control nonce mismatch: the presented nonce does not match the \
                 shim advertisement"
                    .to_string()
            }
        }
    }
}

/// The version + nonce gate run on every `verter/hello`, BEFORE any attach op.
///
/// Fail closed: a protocol version other than [`PROTOCOL_VERSION`] or a nonce
/// other than the shim's advertised `expected_nonce` is refused. The protocol
/// check runs FIRST so a version-mismatched client is rejected as a version
/// mismatch even if its nonce also differs.
pub fn verify_hello(params: &HelloParams, expected_nonce: &str) -> Result<(), HelloRejection> {
    if params.protocol != PROTOCOL_VERSION {
        return Err(HelloRejection::ProtocolMismatch {
            expected: PROTOCOL_VERSION,
            got: params.protocol,
        });
    }
    if params.nonce != expected_nonce {
        return Err(HelloRejection::NonceMismatch);
    }
    Ok(())
}

#[cfg(test)]
#[path = "messages_tests.rs"]
mod tests;
