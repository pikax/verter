use verter_compiler::framework_common::{
    CarrierFrontend, FrameworkEpoch, Present, TypedCapabilityRegistration,
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

struct HtmlEpoch;
impl FrameworkEpoch for HtmlEpoch {
    const ID: &'static str = "html-v1";
}

fn main() {
    let row = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    );
    let _ = row.runtime();
}
