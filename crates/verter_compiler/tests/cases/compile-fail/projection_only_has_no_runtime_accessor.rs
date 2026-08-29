use verter_compiler::framework_common::{
    FrameworkEpochId, Present, ProjectionBackend, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct ProjectionOnly;

impl ProjectionBackend for ProjectionOnly {
    type IdeCompanion = ();
    type PublicApi = ();
    type Declarations = ();
}

fn main() {
    let row = TypedCapabilityRegistration::register_projection(
        FrameworkAdapterId::new("api"),
        LanguageId::new("dts"),
        FrameworkEpochId::new("dts-v1"),
        Present(ProjectionOnly),
    );
    let _ = row.runtime();
}
