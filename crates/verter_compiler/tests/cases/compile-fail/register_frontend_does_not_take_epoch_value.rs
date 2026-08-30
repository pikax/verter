use verter_compiler::framework_common::{
    Present, ProjectionBackend, TypedCapabilityRegistration, VueCarrierFrontend, VueSfcV3,
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

    fn project_ide(&self, _: &str, _: &(), _: &(), _: &()) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {
    let _ = TypedCapabilityRegistration::register_frontend::<VueSfcV3, _>(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        "svelte",
        Present(VueCarrierFrontend),
    );
    let _ = TypedCapabilityRegistration::register_projection::<VueSfcV3, _>(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        "svelte",
        Present(ProjectionOnly),
    );
}
