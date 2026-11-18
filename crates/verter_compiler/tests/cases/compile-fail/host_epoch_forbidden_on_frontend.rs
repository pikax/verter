use verter_compiler::framework_common::{
    CarrierFrontend, FrameworkEpoch, HostEpoch, HostEpochId, Present, TypedCapabilityRegistration,
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

struct SessionHostEpoch;
impl HostEpoch for SessionHostEpoch {
    const ID: &'static str = "session-v1";
}

fn main() {
    let _ = TypedCapabilityRegistration::register_frontend::<HtmlEpoch, _>(
        FrameworkAdapterId::new("tooling"),
        LanguageId::new("html"),
        HostEpochId::from_type::<SessionHostEpoch>(),
        Present(ToolingFrontend),
    )
    .expect("the test manifest is admitted");
}
