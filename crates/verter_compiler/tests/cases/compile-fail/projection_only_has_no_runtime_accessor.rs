use verter_compiler::framework_common::{
    FrameworkEpoch, Present, ProjectionBackend, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ProjectionOnly;

impl ProjectionBackend for ProjectionOnly {
    type IdeCompanion = ();
    type PublicApi = ();
    type Declarations = ();
}

struct DtsEpoch;
impl FrameworkEpoch for DtsEpoch {
    const ID: &'static str = "dts-v1";
}

fn main() {
    let row = TypedCapabilityRegistration::register_projection::<DtsEpoch, _>(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        Present(ProjectionOnly),
    );
    let _ = row.runtime();
}
