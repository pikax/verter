//! One fixture per [`SyntacticContract`] variant: valid bytes validate,
//! and a genuinely malformed fixture is refused. This module never
//! second-guesses "parses under the declared grammar" with a shape check
//! stronger than the grammar itself allows — a bare expression statement
//! is syntactically legal at the top of any ECMAScript module, so
//! [`SyntacticContract::CompleteModule`]'s negative case below exercises a
//! genuinely malformed fixture (unterminated syntax), not merely a
//! "too simple" one.

use verter_compiler::assembly::{
    ContentId, Fragment, FragmentDialect, FragmentRefusal, FrameworkDomain, PlacementSlot,
    SourceId, SourceRevision, SourceSpaceKind, SourceUnit, SyntacticContract,
};
use verter_compiler::compile_request::ProductKind;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};

struct Tag(&'static str);
impl CanonicalEncode for Tag {
    const DOMAIN_TAG: &'static str = "verter.compiler.tests.assembly.fragment_parse_contract.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        e.field_str(1, self.0);
    }
}

fn fragment(contract: SyntacticContract, code: &str) -> Fragment {
    let source_unit = SourceUnit::mint(
        SourceId::from_canonical(&Tag("Comp.vue")),
        SourceRevision::from_canonical(&Tag("rev")),
        "fixture",
        ContentId::from_content_bytes(code.as_bytes()),
    );
    Fragment {
        domain: FrameworkDomain::Vue,
        product: ProductKind::RuntimeClient,
        source_unit: source_unit.id().clone(),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract,
        dialect: FragmentDialect::Tsx,
        code: code.to_string(),
        source_map: None,
        imports: Vec::new(),
        exports: Vec::new(),
        helpers: Vec::new(),
        dependencies: Vec::new(),
    }
}

#[test]
fn complete_module_with_import_and_export_validates() {
    let f = fragment(
        SyntacticContract::CompleteModule,
        "import { ref } from 'vue'\nexport default { setup() { return { ref } } }",
    );
    assert!(f.validate().is_ok());
}

#[test]
fn complete_module_with_malformed_syntax_is_refused() {
    let f = fragment(SyntacticContract::CompleteModule, "import { from 'vue'");
    let err = f.validate().unwrap_err();
    assert!(matches!(
        err,
        FragmentRefusal::ContractViolation {
            contract: SyntacticContract::CompleteModule,
            ..
        }
    ));
}

#[test]
fn statement_list_with_multiple_statements_validates() {
    let f = fragment(
        SyntacticContract::StatementList,
        "const a = 1;\nfunction f() { return a; }",
    );
    assert!(f.validate().is_ok());
}

#[test]
fn statement_list_with_malformed_syntax_is_refused() {
    let f = fragment(SyntacticContract::StatementList, "const a = ;");
    assert!(f.validate().is_err());
}

#[test]
fn expression_with_a_single_expression_validates() {
    let f = fragment(SyntacticContract::Expression, "a + b * 2");
    assert!(f.validate().is_ok());
}

#[test]
fn expression_with_a_statement_sequence_is_refused() {
    let f = fragment(SyntacticContract::Expression, "const a = 1; a + 1;");
    let err = f.validate().unwrap_err();
    assert!(matches!(
        err,
        FragmentRefusal::ContractViolation {
            contract: SyntacticContract::Expression,
            ..
        }
    ));
}

#[test]
fn declaration_with_a_class_declaration_validates() {
    let f = fragment(SyntacticContract::Declaration, "class Foo {}");
    assert!(f.validate().is_ok());
}

#[test]
fn declaration_with_a_bare_expression_is_refused() {
    let f = fragment(SyntacticContract::Declaration, "foo()");
    let err = f.validate().unwrap_err();
    assert!(matches!(
        err,
        FragmentRefusal::ContractViolation {
            contract: SyntacticContract::Declaration,
            ..
        }
    ));
}

#[test]
fn style_and_metadata_contracts_accept_any_non_empty_opaque_payload() {
    // Not ECMAScript — this module never JS-parses these (CSS ownership is
    // out of scope; see `SyntacticContract::Style`'s own doc).
    assert!(fragment(SyntacticContract::Style, ".a { color: red }")
        .validate()
        .is_ok());
    assert!(fragment(SyntacticContract::Metadata, "{\"en\":{}}")
        .validate()
        .is_ok());
}

#[test]
fn style_and_metadata_contracts_refuse_an_empty_payload() {
    assert!(fragment(SyntacticContract::Style, "").validate().is_err());
    assert!(fragment(SyntacticContract::Metadata, "")
        .validate()
        .is_err());
}
