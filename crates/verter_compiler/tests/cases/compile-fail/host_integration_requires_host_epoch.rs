use verter_compiler::framework_common::{
    FrameworkEpoch, FrameworkHostIntegrationBackend, Present, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct TestEpoch;
impl FrameworkEpoch for TestEpoch {
    const ID: &'static str = "vue-sfc-v3";
}

struct HostCapable;

impl FrameworkHostIntegrationBackend<TestEpoch, ()> for HostCapable {
    type CompileAdmission = ();
    type ParseArtifact = ();
    type MultiProductDemand = ();
    type RuntimeRenderDemand = ();
    type AdmissionRefusal = ();

    fn admit_host_products(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }

    fn admit_runtime_render(&self, _artifact: &(), _demand: ()) -> Result<(), ()> {
        Err(())
    }
}

fn main() {
    let _ = TypedCapabilityRegistration::register_host_integration::<TestEpoch, (), _>(
        FrameworkAdapterId::new("host"),
        LanguageId::new("vue"),
        Present(HostCapable),
    );
}
