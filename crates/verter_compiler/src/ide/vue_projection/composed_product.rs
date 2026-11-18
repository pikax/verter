//! The contributions a product supplies to the checking-module composer.

use super::branches::TemplateBlock;
use super::checking_fragment::{BlockHead, ObligationFamily, PlacedStatement, PreludeDeclaration};
use super::checking_module::WithheldReason;

/// To wire a product, implement this and add it to the list in `compose_template_scope`.
pub(crate) trait ComposedProduct {
    fn family(&self) -> ObligationFamily;
    fn withheld_reason(&self) -> Option<WithheldReason>;
    fn preludes(&self) -> Vec<PreludeDeclaration> {
        Vec::new()
    }
    fn statements(&self) -> Vec<PlacedStatement> {
        Vec::new()
    }
    fn block_head(&self, _block: &TemplateBlock) -> Option<BlockHead> {
        None
    }
}

pub(super) fn withheld_reason(pending: bool, complete: bool) -> Option<WithheldReason> {
    if pending {
        Some(WithheldReason::Pending)
    } else if !complete {
        Some(WithheldReason::Incomplete)
    } else {
        None
    }
}

impl ComposedProduct for super::branches::TemplateBranchProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Branch
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(false, self.complete)
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }

    fn block_head(&self, block: &TemplateBlock) -> Option<BlockHead> {
        if matches!(block.kind, super::branches::TemplateBlockKind::Arm { .. }) {
            self.arm_head(block.id)
        } else {
            None
        }
    }
}

impl ComposedProduct for super::iteration::IterationProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Iteration
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn block_head(&self, block: &TemplateBlock) -> Option<BlockHead> {
        if matches!(block.kind, super::branches::TemplateBlockKind::Loop { .. }) {
            self.loop_head(block.id)
        } else {
            None
        }
    }
}

impl ComposedProduct for super::event_payloads::EventPayloadProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Listener
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::models::ModelProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Model
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::provided_slots::ProvidedSlotProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::ProvidedSlot
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }

    fn block_head(&self, block: &TemplateBlock) -> Option<BlockHead> {
        if matches!(
            block.kind,
            super::branches::TemplateBlockKind::SlotBody { .. }
        ) {
            self.body_head(block.id)
        } else {
            None
        }
    }
}

impl ComposedProduct for super::slot_outlets::SlotOutletProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::SlotOutlet
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::native_elements::NativeElementProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::NativeElement
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::native_models::NativeModelProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::NativeModel
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::directives::DirectiveProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Directive
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::builtins::BuiltinProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Builtin
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::fallthrough::FallthroughProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::Fallthrough
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}

impl ComposedProduct for super::public_instance::PublicInstanceRefProjection {
    fn family(&self) -> ObligationFamily {
        ObligationFamily::TemplateRef
    }

    fn withheld_reason(&self) -> Option<WithheldReason> {
        withheld_reason(self.pending, self.complete)
    }

    fn preludes(&self) -> Vec<PreludeDeclaration> {
        self.preludes()
    }

    fn statements(&self) -> Vec<PlacedStatement> {
        self.statements()
    }
}
