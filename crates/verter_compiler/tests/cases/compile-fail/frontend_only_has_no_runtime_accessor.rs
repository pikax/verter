use verter_compiler::framework_common::{
    CarrierFrontend, FrameworkEpochId, Present, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ToolingFrontend;

impl CarrierFrontend for ToolingFrontend {
    type ParseArtifact = ();
    type SyntaxReject = ();
    type ParseAdmission = ();
}

fn main() {
    let row = TypedCapabilityRegistration::register_frontend(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        FrameworkEpochId::new("html-v1"),
        Present(ToolingFrontend),
    );
    let _ = row.runtime();
}
