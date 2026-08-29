use verter_compiler::framework_common::{
    CarrierFrontend, CatalogCapability, CatalogIdentity, FrameworkEpochId, Present,
    TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ToolingFrontend;

impl CarrierFrontend for ToolingFrontend {
    type ParseArtifact = ();
    type SyntaxReject = ();
    type ParseAdmission = ();
}

fn main() {
    let _ = TypedCapabilityRegistration::register_frontend(
        CatalogIdentity::new(
            FrameworkAdapterId::new("tooling"),
            LanguageId::new("html"),
            FrameworkEpochId::new("html-v1"),
            None,
            CatalogCapability::Runtime,
        ),
        Present(ToolingFrontend),
    );
}
