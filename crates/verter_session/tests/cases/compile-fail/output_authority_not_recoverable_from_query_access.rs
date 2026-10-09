//! Compile-fail fixture: query access never yields the output authority.
//!
//! The engine binding a request port attaches carries only query resources;
//! the host attachment a request port hands out is opaque to the engine and,
//! for the concrete host, holds only an inert lease that opens inside the
//! host's sink module. The project store holds no lease at all: the host's
//! construction root hands the one lease to the attachment.

use verter_type_engine::resolver_core::request_ports::{ExecutionSubmission, HostAttachmentPort};
use verter_type_engine::project_semantic_dispatch::engine_resources::OutputAuthority;

fn from_engine_binding<P: ExecutionSubmission + ?Sized>(port: &P) -> OutputAuthority {
    port.attach_engine().output_authority()
}

fn from_opaque_attachment<P: HostAttachmentPort + ?Sized>(port: &P) -> &OutputAuthority {
    port.host_attachment().output_authority()
}

fn from_host_attachment(host: &verter_session::VerterHost) -> &OutputAuthority {
    HostAttachmentPort::host_attachment(host)
        .output_lease()
        .authority()
}

fn from_project_store(host: &verter_session::VerterHost) -> &OutputAuthority {
    host.project_type_store().output_lease().authority()
}

fn main() {
    let _ = (from_host_attachment, from_project_store);
}
