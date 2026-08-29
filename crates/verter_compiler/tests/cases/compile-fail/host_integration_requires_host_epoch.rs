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
}

fn main() {
    let _ = TypedCapabilityRegistration::register_host_integration::<TestEpoch, (), _>(
        FrameworkAdapterId::new("host"),
        LanguageId::new("vue"),
        Present(HostCapable),
    );
}
