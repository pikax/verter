//! The virtual-DOM branch of slot functions.
//!
//! A slot function rendered on the server also returns virtual nodes for
//! client-side slot rendering. They are written by the client target's own
//! emission ([`VueVdomExecutor::render_slot_content`]) with hoisting and
//! caching off; no second virtual-DOM emitter lives here.

use crate::framework::vue::vdom::VueVdomExecutor;
use crate::template::topology::VueTemplateNodeId;

use super::state::{SsrEmitter, SsrOut};

impl<'c, M: verter_audit::observe::ObserveExec> SsrEmitter<'c, M> {
    /// The client target writes the content; its helpers join the render
    /// function's imports and its name-resolved components join the
    /// function's preamble.
    pub(super) fn slot_content(
        &mut self,
        owner: VueTemplateNodeId,
        out: &mut SsrOut<'c>,
    ) -> Option<crate::emission::EmissionFragment<'c>> {
        match VueVdomExecutor::render_slot_content::<M>(self.inputs, owner) {
            Ok(content) => {
                out.add_runtime_names(&content.helpers);
                for reference in &content.resolved_components {
                    self.register_component_resolve(
                        &reference.tag,
                        &reference.variable,
                        reference.self_reference,
                        out,
                    );
                }
                Some(content.array)
            }
            Err(refusal) => {
                // The render function is refused as a whole; the text left
                // here is never written.
                self.refusal.get_or_insert(refusal);
                None
            }
        }
    }
    /// Allocate the generated closing text once and pair its array hole with
    /// the client fragment. The recorder retains this exact allocation.
    pub(super) fn slot_closing(
        &mut self,
        fill: Option<crate::emission::EmissionFragment<'c>>,
        start: u32,
        out: &SsrOut<'c>,
    ) -> &'c str {
        let content = out.alloc_str(&self.buf);
        if let Some(fill) = fill {
            self.slot_fills.push((content, start, fill));
        }
        content
    }
}
