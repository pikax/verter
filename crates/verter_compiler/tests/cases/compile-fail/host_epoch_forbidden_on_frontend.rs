use verter_compiler::framework_common::{
    CarrierFrontend, FrameworkEpochId, HostEpochId, Present, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ToolingFrontend;

impl CarrierFrontend for ToolingFrontend {
    type ParseArtifact = ();
    type SyntaxReject = ();
    type ParseAdmission = ();

    fn parse(
        &self,
        _source: &str,
        _opts: &verter_language::ParseOptions,
    ) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {
    let _ = TypedCapabilityRegistration::register_frontend(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        FrameworkEpochId::new("html-v1"),
        HostEpochId::new("session-v1"),
        Present(ToolingFrontend),
    );
}
