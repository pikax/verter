//! The assembled artifact `publish` returns parses as ESM, and its
//! declared imports match exactly what its contributing fragments
//! declared. An undeclared helper is refused — nothing publishes.

use verter_compiler::assembly::{
    ArtifactContribution, ContentId, DeclaredHelper, DeclaredImport, DeclaredImportKind, Fragment,
    FragmentDialect, FrameworkDomain, PlacementSlot, ProductPlan, SourceId, SourceRevision,
    SourceSpaceKind, SourceUnit, SyntacticContract,
};
use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
    VueCompileRequest,
};
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.tests.assembly.final_module_parse.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn source_unit_id(role: &str) -> verter_compiler::assembly::SourceUnitId {
    SourceUnit::mint(
        SourceId::from_canonical(&Tag("Comp.vue")),
        SourceRevision::from_canonical(&Tag("rev")),
        role,
        ContentId::from_content_bytes(role.as_bytes()),
    )
    .id()
    .clone()
}

fn script_fragment_with_helper(helper: &str) -> Fragment {
    Fragment {
        domain: FrameworkDomain::Vue,
        product: verter_compiler::compile_request::ProductKind::RuntimeClient,
        source_unit: source_unit_id("script"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract: SyntacticContract::StatementList,
        dialect: FragmentDialect::Tsx,
        code: "const _sfc_main = {}".to_string(),
        source_map: None,
        imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            kind: DeclaredImportKind::Named(vec![helper.to_string()]),
        }],
        exports: Vec::new(),
        helpers: vec![DeclaredHelper {
            name: helper.to_string(),
        }],
        dependencies: Vec::new(),
    }
}

fn runtime_client_plan() -> ProductPlan {
    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        None,
        None,
        false,
        false,
    )
    .expect("test request constructs");
    ProductPlan::from_request(&request)
}

#[test]
fn assembled_artifact_parses_as_esm_and_declared_imports_match() {
    let plan = runtime_client_plan();
    let script = script_fragment_with_helper("_openBlock")
        .validate()
        .expect("fixture fragment parses");
    let code = "import { _openBlock } from \"vue\"\nconst _sfc_main = {}\nexport default _sfc_main";
    let contribution = ArtifactContribution {
        kind: verter_compiler::compile_request::ProductKind::RuntimeClient,
        fragments: vec![&script],
        code: code.to_string(),
        emitted_imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            kind: DeclaredImportKind::Named(vec!["_openBlock".to_string()]),
        }],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let set = verter_compiler::assembly::publish(&plan, vec![contribution])
        .expect("publish succeeds when every emitted import was declared");
    let artifact = set
        .artifact(verter_compiler::compile_request::ProductKind::RuntimeClient)
        .unwrap();

    // The published artifact parses as a complete ESM module.
    let module_fragment = Fragment {
        domain: FrameworkDomain::Vue,
        product: verter_compiler::compile_request::ProductKind::RuntimeClient,
        source_unit: source_unit_id("assembled"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract: SyntacticContract::CompleteModule,
        dialect: FragmentDialect::Tsx,
        code: artifact.code().to_string(),
        source_map: None,
        imports: Vec::new(),
        exports: Vec::new(),
        helpers: Vec::new(),
        dependencies: Vec::new(),
    };
    assert!(
        module_fragment.validate().is_ok(),
        "published artifact must parse as a complete ESM module, got:\n{}",
        artifact.code()
    );
    assert!(artifact
        .code()
        .contains("import { _openBlock } from \"vue\""));
}

#[test]
fn undeclared_helper_in_assembled_bytes_refuses_publication() {
    let plan = runtime_client_plan();
    let script = script_fragment_with_helper("_openBlock")
        .validate()
        .expect("fixture fragment parses");
    // The composer wrote a SECOND helper import no fragment declared —
    // exactly the "generated-product reparsing would have missed this"
    // class this atomicity check exists to catch.
    let contribution = ArtifactContribution {
        kind: verter_compiler::compile_request::ProductKind::RuntimeClient,
        fragments: vec![&script],
        code: "import { _openBlock, _createVNode } from \"vue\"\nconst _sfc_main = {}".to_string(),
        emitted_imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            kind: DeclaredImportKind::Named(vec![
                "_openBlock".to_string(),
                "_createVNode".to_string(),
            ]),
        }],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let err = verter_compiler::assembly::publish(&plan, vec![contribution]).unwrap_err();
    assert_eq!(
        err,
        verter_compiler::assembly::AssemblyRefusal::UndeclaredHelper {
            kind: verter_compiler::compile_request::ProductKind::RuntimeClient,
            specifier: "vue".to_string(),
            name: "_createVNode".to_string(),
        }
    );
}
