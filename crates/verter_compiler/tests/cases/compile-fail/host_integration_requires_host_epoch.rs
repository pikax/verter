use verter_compiler::framework_common::{
    FrameworkEpochId, FrameworkHostIntegrationBackend, Present, TypedCapabilityRegistration,
};
use verter_language::{FrameworkAdapterId, LanguageId};

struct HostCapable;

impl FrameworkHostIntegrationBackend<FrameworkEpochId, ()> for HostCapable {
    type CompileAdmission = ();
}

fn main() {
    let _ = TypedCapabilityRegistration::register_host_integration(
        FrameworkAdapterId::new("host"),
        LanguageId::new("vue"),
        FrameworkEpochId::new("vue-sfc-v3"),
        Present(HostCapable),
    );
}
