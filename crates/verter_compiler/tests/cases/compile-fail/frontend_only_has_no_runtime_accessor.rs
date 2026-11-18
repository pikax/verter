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
    const MANIFEST: verter_compiler::framework_common::profile::FrameworkProfileManifest =
        verter_compiler::framework_common::profile::FrameworkProfileManifest {
        family: "html-v1",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

fn main() {
    let row = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted");
    let _ = row.runtime();
}
