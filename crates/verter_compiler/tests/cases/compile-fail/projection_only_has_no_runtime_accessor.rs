use verter_compiler::framework_common::{
    FrameworkEpoch, Present, ProjectionBackend, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ProjectionOnly;

impl ProjectionBackend for ProjectionOnly {
    type IdeCompanion = ();
    type PublicApi = ();
    type Declarations = ();
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();

    fn project_ide(
        &self,
        _: verter_compiler::framework_common::ProductExecutionGrant,
        _: &str,
        _: &(),
        _: &(),
        _: &(),
    ) -> Result<(), ()> {
        Ok(())
    }
}

struct DtsEpoch;
impl FrameworkEpoch for DtsEpoch {
    const MANIFEST: verter_compiler::framework_common::profile::FrameworkProfileManifest =
        verter_compiler::framework_common::profile::FrameworkProfileManifest {
        family: "dts-v1",
        release: "1",
        admitted_majors: &[1],
        claimed_carriers: &[],
    };
}

fn main() {
    let row = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    )
    .expect("the test manifest is admitted");
    let _ = row.runtime();
}
