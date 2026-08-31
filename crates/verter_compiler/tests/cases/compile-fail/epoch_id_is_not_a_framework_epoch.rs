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
    type ParseArtifact = ();
    type Request = ();
    type ExecutionInputs = ();
    type Error = ();
    type Output = ();

    fn compile_runtime(
        &self,
        _: verter_compiler::framework_common::ProductExecutionGrant,
        _: &str,
        _: &(),
        _: &(),
        _: &(),
    ) -> Result<(), ()> {
        Ok(())
    }
}

impl FrameworkHostIntegrationBackend<FrameworkEpochId, ValidHostEpoch> for HostCapable {
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

fn main() {}
