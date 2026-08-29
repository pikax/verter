use verter_compiler::framework_common::{
    FrameworkEpochId, FrameworkHostIntegrationBackend, FrameworkSemanticAuthority, HostEpoch,
    RuntimeCompilerBackend,
};

struct SemanticCapable;
struct RuntimeCapable;
struct HostCapable;

struct ValidHostEpoch;
impl HostEpoch for ValidHostEpoch {
    const ID: &'static str = "session-v1";
}

impl FrameworkSemanticAuthority<FrameworkEpochId> for SemanticCapable {
    type EvalSource = ();
    type TemplateFacts = ();
    type StyleMeaning = ();
    type SemanticAdmission = ();
    type ParseArtifact = ();

    fn eval_source(&self, _source: &str, _artifact: &()) {}
    fn template_facts(&self, _source: &str, _artifact: &()) {}
}

impl RuntimeCompilerBackend<FrameworkEpochId> for RuntimeCapable {
    type RuntimeClient = ();
    type RuntimeServer = ();
}

impl FrameworkHostIntegrationBackend<FrameworkEpochId, ValidHostEpoch> for HostCapable {
    type CompileAdmission = ();
}

fn main() {}
