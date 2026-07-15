//! The relay-shim control protocol: the versioned semantic attach surface a
//! `verter-relay-shim` exposes to a `verter_lsp` control client.
//!
//! The shim is spawned by the EDITOR (as its `tsgo`), relays the editor↔tsgo
//! `--lsp` stdio, and owns the carrier egress taint. A `verter_lsp` control
//! client — a SEPARATE process — drives carrier injection through this
//! protocol, never a raw wire. The protocol is JSON-RPC 2.0 over same-user
//! local IPC (a Windows named pipe / a Unix-domain socket), VERSIONED from
//! message one.
//!
//! ## Module layout
//!
//! - [`messages`] — the versioned message types + method-name constants +
//!   the [`messages::verify_hello`] version/nonce gate. This is the STABLE
//!   wire contract.

pub mod advertisement;
pub mod client;
pub mod messages;
pub mod server;
pub mod transport;

pub use client::ControlClient;
pub use server::ControlServer;
pub use transport::{
    connect_control_endpoint, control_endpoint_path, ControlListener, ControlReadHalf,
    ControlWriteHalf,
};

pub use advertisement::{
    advertisement_file_name, remove_advertisement, sanitize_component, stable_hash_str,
    Advertisement, AdvertisementError, ADVERTISEMENT_VERSION,
};
pub use messages::{
    CarrierDidChangeSyncedParams, CarrierDidCloseParams, CarrierDidOpenSyncedParams, ControlAck,
    ControlCapabilities, DetachParams, FatalParams, FatalReason, HelloParams, HelloRejection,
    HelloResult, InitializeApiSessionResult, StatusResult, WaitInitializedResult,
    ERROR_CONTROL_OP_FAILED, ERROR_MALFORMED_PAYLOAD, ERROR_NONCE_MISMATCH,
    ERROR_NOT_AUTHENTICATED, ERROR_PROTOCOL_MISMATCH, METHOD_CARRIER_DID_CHANGE_SYNCED,
    METHOD_CARRIER_DID_CLOSE, METHOD_CARRIER_DID_OPEN_SYNCED, METHOD_DETACH, METHOD_FATAL,
    METHOD_HELLO, METHOD_INITIALIZE_API_SESSION, METHOD_STATUS, METHOD_WAIT_INITIALIZED,
    PROTOCOL_VERSION,
};
