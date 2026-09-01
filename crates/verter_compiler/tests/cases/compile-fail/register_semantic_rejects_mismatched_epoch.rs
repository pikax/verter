use verter_compiler::framework_common::{
    Present, TypedCapabilityRegistration, VueSemanticAuthority,
};
use verter_compiler::svelte::SvelteSfc5;
use verter_language::{FrameworkAdapterId, LanguageId};

fn main() {
    let _ = TypedCapabilityRegistration::register_semantic::<SvelteSfc5, _>(
        FrameworkAdapterId::new("vue"),
        LanguageId::new("vue"),
        Present(VueSemanticAuthority),
    );
}
