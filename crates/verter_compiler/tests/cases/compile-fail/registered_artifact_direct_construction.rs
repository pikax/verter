use std::any::Any;
use std::sync::Arc;

use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::{
    CarrierParse, FrameworkAdapterId, FrameworkParseCommon, LanguageId,
};

struct Payload;

impl CarrierParse for Payload {
    fn __verter_as_any(&self) -> &dyn Any {
        self
    }

    fn __verter_as_any_arc(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

fn main() {
    let _ = FrameworkParseArtifact::new(
        FrameworkAdapterId::vue(),
        LanguageId::new("vue"),
        1,
        FrameworkParseCommon::default(),
        Arc::new(Payload),
    );
}
