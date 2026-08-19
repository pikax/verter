use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::FrameworkParseCommon;

fn replace_registered_inventory(mut artifact: FrameworkParseArtifact) {
    artifact.common = FrameworkParseCommon::default();
}

fn main() {}
