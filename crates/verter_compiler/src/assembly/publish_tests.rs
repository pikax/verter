//! Unit tests for atomic artifact-set publication and for the identity
//! digest the four compile routes are compared with. Extracted from
//! `publish.rs` under the project rule that an inline `#[cfg(test)]`
//! module moves to a sibling file once it outgrows its host.

use super::*;

use crate::assembly::fragment::{
    DeclaredImportKind, Fragment, FragmentDialect, FrameworkDomain, PlacementSlot,
    SyntacticContract,
};
use crate::assembly::source_space::SourceSpaceKind;
use crate::assembly::source_unit::SourceUnitId;
use crate::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, IdeProductRequest,
    RuntimeProductRequest, VueCompileRequest,
};

fn unit(tag: &str) -> SourceUnitId {
    crate::assembly::source_unit::source_unit_id("source", tag)
}

fn fragment_with_helper(helper: &str) -> ValidatedFragment {
    Fragment {
        domain: FrameworkDomain::Vue,
        product: ProductKind::RuntimeClient,
        source_unit: unit("script"),
        source_space: SourceSpaceKind::GeneratedFragment,
        placement: PlacementSlot::ModuleBody,
        contract: SyntacticContract::StatementList,
        dialect: FragmentDialect::Tsx,
        code: "const x = 1;".to_string(),
        source_map: None,
        imports: vec![],
        exports: vec![],
        helpers: vec![DeclaredHelper {
            name: helper.to_string(),
        }],
        dependencies: vec![],
    }
    .validate()
    .expect("fixture parses")
}

fn plan_with(products: Vec<CompileProduct>) -> ProductPlan {
    let request = CompileRequest::new(
        products,
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
fn publishes_exactly_the_planned_runtime_client_artifact() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let fragment = fragment_with_helper("_openBlock");
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![&fragment],
        code: "import { _openBlock } from 'vue'".to_string(),
        emitted_imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            kind: DeclaredImportKind::Named(vec!["_openBlock".to_string()]),
        }],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let set = publish(&plan, vec![contribution]).expect("publish succeeds");
    assert_eq!(set.artifacts().len(), 1);
    assert!(set.artifact(ProductKind::RuntimeClient).is_some());
    assert!(
        set.artifact(ProductKind::IdeCompanion).is_none(),
        "an artifact never requested must never appear in the published set"
    );
}

#[test]
fn ide_companion_without_projection_map_is_refused() {
    let plan = plan_with(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let contribution = ArtifactContribution {
        kind: ProductKind::IdeCompanion,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let err = publish(&plan, vec![contribution]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::MissingRequiredSourceProjectionMap {
            kind: ProductKind::IdeCompanion
        }
    );
}

#[test]
fn ide_companion_with_projection_map_publishes_both_atomically() {
    let plan = plan_with(vec![CompileProduct::IdeCompanion(
        IdeProductRequest::default(),
    )]);
    let contribution = ArtifactContribution {
        kind: ProductKind::IdeCompanion,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: Some("{\"version\":3}".to_string()),
        runtime_source_map: None,
    };
    let set = publish(&plan, vec![contribution]).expect("publish succeeds");
    let artifact = set.artifact(ProductKind::IdeCompanion).unwrap();
    assert!(artifact.source_projection_map.is_some());
}

#[test]
fn runtime_map_absent_when_not_requested_is_a_true_none_not_an_empty_map() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: false,
        ..Default::default()
    })]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let set = publish(&plan, vec![contribution]).expect("publish succeeds");
    assert!(
        set.artifact(ProductKind::RuntimeClient)
            .unwrap()
            .runtime_source_map
            .is_none(),
        "an unrequested runtime map must be a true None, never an empty encoded map"
    );
}

#[test]
fn unrequested_runtime_map_attached_anyway_is_refused() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: false,
        ..Default::default()
    })]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: Some("{\"version\":3}".to_string()),
    };
    let err = publish(&plan, vec![contribution]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::UnrequestedRuntimeSourceMap {
            kind: ProductKind::RuntimeClient
        }
    );
}

#[test]
fn undeclared_import_name_is_refused() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let fragment = fragment_with_helper("_openBlock");
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![&fragment],
        code: "import { _createVNode } from 'vue'".to_string(),
        emitted_imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            // No fragment declared this helper.
            kind: DeclaredImportKind::Named(vec!["_createVNode".to_string()]),
        }],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let err = publish(&plan, vec![contribution]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::UndeclaredHelper {
            kind: ProductKind::RuntimeClient,
            specifier: "vue".to_string(),
            name: "_createVNode".to_string(),
        }
    );
}

/// Publishes one artifact whose `code`, runtime map and diagnostics are
/// supplied by the caller, so a test can vary exactly one digest input.
fn digest_of(
    code: &str,
    runtime_source_map: Option<&str>,
    diagnostics: Vec<crate::compile::types::CompileDiagnostic>,
) -> [u8; 32] {
    // `publish` fail-closes on a map the plan did not request, so the plan
    // has to ask for one whenever this helper supplies one.
    let plan = plan_with(vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
        runtime_source_map: runtime_source_map.is_some(),
        ..RuntimeProductRequest::default()
    })]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: code.to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: runtime_source_map.map(str::to_string),
    };
    crate::standalone::direct_compile_output_digest(&crate::standalone::DirectCompileOutput {
        artifacts: publish(&plan, vec![contribution]).expect("publish"),
        styles: Vec::new(),
        diagnostics,
    })
}

fn one_diagnostic(message: &str) -> Vec<crate::compile::types::CompileDiagnostic> {
    vec![crate::compile::types::CompileDiagnostic {
        severity: crate::compile::types::CompileDiagnosticSeverity::Warning,
        code: "X_TEST".to_string(),
        message: message.to_string(),
        span: None,
    }]
}

// The identity digest is the ONLY oracle the four compile routes are compared
// with, so every field it covers needs a test that fails if the hasher stops
// covering it. Each test below varies exactly one hashed field and asserts the
// digest moves.
//
// Two of them need a pair a single-record comparison cannot express: a
// diagnostic span's `0`/`1` presence tag and the `styles.len()` prefix. The
// encoding is prefix-free WITHIN one record but not ACROSS concatenated ones,
// so isolating either needs two inputs whose contents trade places once the
// delimiter is gone. Both have such a test.
//
// TWO inputs are covered only in aggregate, and saying so is the point:
// `artifacts.len()` and `diagnostics.len()` can each be deleted with every
// test in this module still green. They are kept because they make the encoding
// self-delimiting, which is what makes every other field's discriminator
// sound — but no claim is made here that no input could isolate them, and the
// evidence ledger records each one's actual status (a rigidity argument and a
// failed attempt respectively).
//
// Neither aggregate-only entry is proven un-isolatable. Both isolators that DO
// exist below were found at the one place the encoding stops being prefix-free
// — a section boundary, where one section's bytes can be re-read as the next
// section's header — so that is where to look before believing the pair count
// above is final.

#[test]
fn identity_digest_changes_when_one_byte_of_artifact_code_changes() {
    assert_ne!(
        digest_of("export default {}", None, Vec::new()),
        digest_of("export default {};", None, Vec::new()),
        "a digest that skipped artifact code would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_the_runtime_source_map_changes() {
    assert_ne!(
        digest_of(
            "export default {}",
            Some("{\"version\":3,\"a\":1}"),
            Vec::new()
        ),
        digest_of(
            "export default {}",
            Some("{\"version\":3,\"a\":2}"),
            Vec::new()
        ),
        "a digest that skipped the runtime source map would leave these equal"
    );
}

#[test]
fn identity_digest_distinguishes_a_present_source_map_from_an_absent_one() {
    assert_ne!(
        digest_of("export default {}", None, Vec::new()),
        digest_of("export default {}", Some("{\"version\":3}"), Vec::new()),
        "a digest that skipped the runtime source map would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_a_diagnostic_message_changes() {
    assert_ne!(
        digest_of("export default {}", None, one_diagnostic("first")),
        digest_of("export default {}", None, one_diagnostic("second")),
        "a digest that skipped diagnostic messages would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_a_diagnostic_appears() {
    assert_ne!(
        digest_of("export default {}", None, Vec::new()),
        digest_of("export default {}", None, one_diagnostic("first")),
        "a digest that skipped diagnostics entirely would leave these equal"
    );
}

/// Every input [`crate::standalone::direct_compile_output_digest`]
/// hashes, held at a fixed baseline so a test can vary exactly one and
/// attribute the digest move to it.
#[derive(Clone)]
struct DigestFixture {
    product: CompileProduct,
    code: String,
    dialect: FragmentDialect,
    source_projection_map: Option<String>,
    runtime_source_map: Option<String>,
    styles: Vec<crate::framework_common::carrier_compiler::RuntimeStyleBlock>,
    diagnostics: Vec<crate::compile::types::CompileDiagnostic>,
}

impl DigestFixture {
    /// The `RuntimeClient` baseline: no maps, no styles, no diagnostics.
    fn runtime() -> Self {
        Self {
            product: CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            code: "export default {}".to_string(),
            dialect: FragmentDialect::Tsx,
            source_projection_map: None,
            runtime_source_map: None,
            styles: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// The `IdeCompanion` baseline. `publish` REQUIRES a projection map
    /// on this kind and REFUSES one on a runtime kind, so this is the
    /// only shape in which `source_projection_map` can be varied without
    /// also changing the product kind.
    fn ide(projection_map: &str) -> Self {
        Self {
            product: CompileProduct::IdeCompanion(IdeProductRequest::default()),
            source_projection_map: Some(projection_map.to_string()),
            ..Self::runtime()
        }
    }

    fn digest(&self) -> [u8; 32] {
        let plan = plan_with(vec![self.product.clone()]);
        let contribution = ArtifactContribution {
            kind: self.product.kind(),
            fragments: vec![],
            code: self.code.clone(),
            emitted_imports: Vec::new(),
            dialect: self.dialect,
            source_projection_map: self.source_projection_map.clone(),
            runtime_source_map: self.runtime_source_map.clone(),
        };
        crate::standalone::direct_compile_output_digest(&crate::standalone::DirectCompileOutput {
            artifacts: publish(&plan, vec![contribution]).expect("publish"),
            styles: self.styles.clone(),
            diagnostics: self.diagnostics.clone(),
        })
    }
}

#[test]
fn identity_digest_changes_when_the_product_kind_changes() {
    let mut server = DigestFixture::runtime();
    server.product = CompileProduct::RuntimeServer(RuntimeProductRequest::default());
    assert_ne!(
        DigestFixture::runtime().digest(),
        server.digest(),
        "a digest that skipped the artifact kind would leave these equal — \
         the code and dialect are byte-identical on both arms"
    );
}

#[test]
fn identity_digest_changes_when_the_dialect_changes() {
    let mut javascript = DigestFixture::runtime();
    javascript.dialect = FragmentDialect::JavaScript;
    assert_ne!(
        DigestFixture::runtime().digest(),
        javascript.digest(),
        "a digest that skipped the artifact dialect would leave these equal — \
         `export default {{}}` is byte-identical in both dialects"
    );
}

#[test]
fn identity_digest_changes_when_the_source_projection_map_changes() {
    assert_ne!(
        DigestFixture::ide("{\"version\":3,\"a\":1}").digest(),
        DigestFixture::ide("{\"version\":3,\"a\":2}").digest(),
        "a digest that skipped the source projection map would leave these equal"
    );
}

/// A style block whose non-descriptor fields the caller chooses. The
/// descriptor is built from `descriptor_source`, held separate from
/// `code` on purpose: `RuntimeOutputDescriptor::generated` derives
/// itself from the bytes it is given, so deriving it from `code` would
/// make every `code` test also move the descriptor and prove neither.
fn style_block(
    code: &str,
    source_map: Option<&str>,
    lang: Option<&str>,
    scope_hash: Option<&str>,
    has_global: bool,
    descriptor_source: &str,
) -> crate::framework_common::carrier_compiler::RuntimeStyleBlock {
    use crate::framework_common::carrier_compiler::{RuntimeOutputDescriptor, RuntimeStyleBlock};
    RuntimeStyleBlock {
        code: code.to_string(),
        source_map: source_map.map(str::to_string),
        lang: lang.map(str::to_string),
        scope_hash: scope_hash.map(str::to_string),
        has_global,
        output_descriptor: RuntimeOutputDescriptor::generated(
            descriptor_source,
            None,
            &[("space", "artifact")],
            crate::framework_common::SourceMapFidelity::Approximate,
        ),
    }
}

fn digest_with_style(
    style: crate::framework_common::carrier_compiler::RuntimeStyleBlock,
) -> [u8; 32] {
    let mut fixture = DigestFixture::runtime();
    fixture.styles = vec![style];
    fixture.digest()
}

const BASE_STYLE_DESCRIPTOR_SOURCE: &str = ".fixed {}";

#[test]
fn identity_digest_changes_when_style_code_changes() {
    assert_ne!(
        digest_with_style(style_block(
            ".a { color: red }",
            None,
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        digest_with_style(style_block(
            ".a { color: blue }",
            None,
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        "a digest that skipped style code would leave these equal — the output \
         descriptor is held identical on both arms"
    );
}

#[test]
fn identity_digest_changes_when_a_style_source_map_changes() {
    assert_ne!(
        digest_with_style(style_block(
            ".a {}",
            Some("{\"version\":3,\"a\":1}"),
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        digest_with_style(style_block(
            ".a {}",
            Some("{\"version\":3,\"a\":2}"),
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        "a digest that skipped the style source map would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_style_lang_changes() {
    assert_ne!(
        digest_with_style(style_block(
            ".a {}",
            None,
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        digest_with_style(style_block(
            ".a {}",
            None,
            Some("scss"),
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        "a digest that skipped the style language would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_a_style_scope_hash_changes() {
    assert_ne!(
        digest_with_style(style_block(
            ".a {}",
            None,
            None,
            Some("svelte-aaaaaa"),
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        digest_with_style(style_block(
            ".a {}",
            None,
            None,
            Some("svelte-bbbbbb"),
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        "a digest that skipped the style scope hash would leave these equal — \
         the scoped class is baked into the component's markup, so two components \
         scoped differently are not the same result"
    );
}

#[test]
fn identity_digest_changes_when_style_has_global_changes() {
    assert_ne!(
        digest_with_style(style_block(
            ".a {}",
            None,
            None,
            None,
            false,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        digest_with_style(style_block(
            ".a {}",
            None,
            None,
            None,
            true,
            BASE_STYLE_DESCRIPTOR_SOURCE
        )),
        "a digest that skipped `has_global` would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_the_style_output_descriptor_changes() {
    assert_ne!(
        digest_with_style(style_block(".a {}", None, None, None, false, ".fixed {}")),
        digest_with_style(style_block(".a {}", None, None, None, false, ".moved {}")),
        "a digest that skipped the style output descriptor would leave these equal — \
         both arms publish byte-identical style code, and only the descriptor's own \
         declared-source identity differs"
    );
}

#[test]
fn identity_digest_changes_when_the_style_count_changes() {
    let one = style_block(
        ".a {}",
        None,
        None,
        None,
        false,
        BASE_STYLE_DESCRIPTOR_SOURCE,
    );
    let mut two = DigestFixture::runtime();
    two.styles = vec![one.clone(), one.clone()];
    assert_ne!(
        digest_with_style(one),
        two.digest(),
        "a digest that skipped the style count would leave these equal"
    );
}

fn diagnostic(
    severity: crate::compile::types::CompileDiagnosticSeverity,
    code: &str,
    message: &str,
    span: Option<crate::common::Span>,
) -> Vec<crate::compile::types::CompileDiagnostic> {
    vec![crate::compile::types::CompileDiagnostic {
        severity,
        code: code.to_string(),
        message: message.to_string(),
        span,
    }]
}

fn digest_with_diagnostics(diagnostics: Vec<crate::compile::types::CompileDiagnostic>) -> [u8; 32] {
    let mut fixture = DigestFixture::runtime();
    fixture.diagnostics = diagnostics;
    fixture.digest()
}

#[test]
fn identity_digest_changes_when_a_diagnostic_severity_changes() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            None
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Error,
            "X_TEST",
            "same",
            None
        )),
        "a digest that skipped diagnostic severity would leave these equal — \
         an error and a warning with the same text are not the same result"
    );
}

#[test]
fn identity_digest_changes_when_a_diagnostic_code_changes() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_ONE",
            "same",
            None
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TWO",
            "same",
            None
        )),
        "a digest that skipped diagnostic codes would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_a_diagnostic_span_changes() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(0, 4))
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(2, 6))
        )),
        "a digest that skipped diagnostic spans would leave these equal"
    );
}

/// `span.start` on its own. The value test below varies BOTH components at
/// once (`(0, 4)` versus `(2, 6)`), so either one alone carries it and a digest
/// that stopped observing starts would be invisible. These two hold the other
/// component fixed.
#[test]
fn identity_digest_changes_when_only_a_diagnostic_span_start_changes() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(0, 4))
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(2, 4))
        )),
        "a digest that skipped a diagnostic span's start would leave these equal"
    );
}

#[test]
fn identity_digest_changes_when_only_a_diagnostic_span_end_changes() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(0, 4))
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(0, 6))
        )),
        "a digest that skipped a diagnostic span's end would leave these equal"
    );
}

/// An absent span and a present zero-length span at offset 0 are different
/// results and must not share a digest. This is REDUNDANTLY covered rather
/// than isolating: it stays green with the presence tag deleted (the
/// span's own eight bytes separate the arms — `Span` is two `u32`s, so
/// `Some(Span::new(0, 0))` encodes eight ZERO bytes rather than nothing) AND
/// with the span bytes deleted (the tag separates them). The presence
/// discriminator is
/// `identity_digest_changes_when_diagnostic_span_presence_moves_between_diagnostics`.
#[test]
fn identity_digest_distinguishes_a_present_diagnostic_span_from_an_absent_one() {
    use crate::compile::types::CompileDiagnosticSeverity;
    assert_ne!(
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            None
        )),
        digest_with_diagnostics(diagnostic(
            CompileDiagnosticSeverity::Warning,
            "X_TEST",
            "same",
            Some(crate::common::Span::new(0, 0))
        )),
        "an absent span and a present zero-length span are different results and \
         must not share a digest"
    );
}

/// Span PRESENCE, isolated from the span's own bytes.
///
/// A single present-versus-absent pair cannot isolate the `0`/`1` tag: the
/// present arm also contributes the span's eight encoded bytes, so the
/// digests differ with the tag deleted. Two diagnostics whose presence is
/// SWAPPED do isolate it. Both diagnostics below are `Error` with an empty
/// code and message, so every field either side contributes is zero bytes:
/// with the tags deleted both streams are the same run of zeros, because
/// the present span's eight zero bytes and the other diagnostic's
/// eight-byte zero severity simply exchange positions.
#[test]
fn identity_digest_changes_when_diagnostic_span_presence_moves_between_diagnostics() {
    use crate::compile::types::{CompileDiagnostic, CompileDiagnosticSeverity};
    fn blank(span: Option<crate::common::Span>) -> CompileDiagnostic {
        CompileDiagnostic {
            severity: CompileDiagnosticSeverity::Error,
            code: String::new(),
            message: String::new(),
            span,
        }
    }
    let present = Some(crate::common::Span::new(0, 0));
    assert_ne!(
        digest_with_diagnostics(vec![blank(None), blank(present)]),
        digest_with_diagnostics(vec![blank(present), blank(None)]),
        "a digest that skipped a diagnostic span's presence tag would leave these equal — \
         both streams are the same run of zero bytes without it"
    );
}

/// A `RuntimeOutputDescriptor` whose every string is empty and every length is
/// zero — 91 encoded bytes, all of them zero except the two discriminants,
/// which are the zero variants. Written as a literal because
/// `RuntimeOutputDescriptor::generated` hashes whatever it is given and can
/// never produce this.
fn empty_output_descriptor() -> crate::framework_common::carrier_compiler::RuntimeOutputDescriptor {
    use crate::framework_common::carrier_compiler::{
        OutputContentArtifactDescriptor, OutputSourceSpaceDescriptor, OutputSourceSpaceKind,
        QualifiedOutputSourceMap, RuntimeOutputDescriptor,
    };
    use crate::framework_common::SourceMapFidelity;
    RuntimeOutputDescriptor {
        source_space: OutputSourceSpaceDescriptor {
            token: String::new(),
            kind: OutputSourceSpaceKind::Owner,
            source_token: String::new(),
            content_hash: String::new(),
            utf8_byte_len: 0,
        },
        content_artifact: OutputContentArtifactDescriptor {
            token: String::new(),
            source_space_token: String::new(),
            content_hash: String::new(),
            utf8_byte_len: 0,
        },
        source_map: QualifiedOutputSourceMap {
            map_hash: String::new(),
            destination_space_token: String::new(),
            declared_space_tokens: Vec::new(),
            raw_map: None,
            fidelity: SourceMapFidelity::Exact,
        },
    }
}

/// The style-COUNT prefix, isolated, by the same section-boundary argument.
///
/// Both arms encode to exactly 786 bytes once the style count is deleted. One
/// style block whose `code` is 24 bytes is re-read as a 24-diagnostic count;
/// that code's ninth byte is `g` (0x67 = 103), re-read as the length of the
/// first diagnostic's code; the style's remaining bytes plus its four
/// option/flag bytes plus its 91-byte all-empty output descriptor are re-read
/// as that code's 103 zero content bytes; and the arm's own diagnostic count
/// is re-read as the first diagnostic's message length.
///
/// Every literal is load-bearing — the 103, the 24, the `g`, the 8/15 NUL
/// split, `Error` on all twenty-four, `span: None` throughout, and an
/// all-empty descriptor with both `utf8_byte_len` zero. Change one and re-run
/// the plant before believing the test.
#[test]
fn identity_digest_changes_when_a_style_block_is_replaced_by_diagnostic_bytes() {
    use crate::compile::types::{CompileDiagnostic, CompileDiagnosticSeverity};
    fn blank_diagnostic() -> CompileDiagnostic {
        CompileDiagnostic {
            severity: CompileDiagnosticSeverity::Error,
            code: String::new(),
            message: String::new(),
            span: None,
        }
    }
    // No styles; the first diagnostic carries the bytes the other arm spends on
    // a style block.
    let mut without_style = DigestFixture::runtime();
    let mut absorbed = vec![CompileDiagnostic {
        severity: CompileDiagnosticSeverity::Error,
        code: "\u{0}".repeat(103),
        message: "\u{0}".repeat(24),
        span: None,
    }];
    absorbed.extend(std::iter::repeat_with(blank_diagnostic).take(23));
    without_style.diagnostics = absorbed;

    // One style block, twenty-four minimal diagnostics.
    let mut with_style = DigestFixture::runtime();
    with_style.styles = vec![
        crate::framework_common::carrier_compiler::RuntimeStyleBlock {
            code: format!("{}g{}", "\u{0}".repeat(8), "\u{0}".repeat(15)),
            source_map: None,
            lang: None,
            scope_hash: None,
            has_global: false,
            // Built as a literal, NOT through `RuntimeOutputDescriptor::generated`:
            // that constructor derives sha256 tokens from the bytes it is handed,
            // and a 71-character token in any slot destroys the alignment. Every
            // string here must be empty and every length zero.
            output_descriptor: empty_output_descriptor(),
        },
    ];
    with_style.diagnostics = std::iter::repeat_with(blank_diagnostic).take(24).collect();

    assert_ne!(
        without_style.digest(),
        with_style.digest(),
        "a digest that skipped the style count would leave these equal — one style block and \
         zero style blocks are different published results"
    );
}

#[test]
fn declared_import_name_publishes_cleanly() {
    // Positive control for the undeclared-helper refusal above.
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let fragment = fragment_with_helper("_openBlock");
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![&fragment],
        code: "import { _openBlock } from 'vue'".to_string(),
        emitted_imports: vec![DeclaredImport {
            specifier: "vue".to_string(),
            kind: DeclaredImportKind::Named(vec!["_openBlock".to_string()]),
        }],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    assert!(publish(&plan, vec![contribution]).is_ok());
}

#[test]
fn multi_product_request_publishes_exactly_those_products_and_nothing_else() {
    // Positive control for the missing/unplanned-artifact refusals below:
    // a request naming MULTIPLE products publishes exactly that set in
    // one atomic call — never a subset, never an extra virtual artifact.
    let plan = plan_with(vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
        CompileProduct::Declarations(Default::default()),
    ]);
    let runtime = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let declarations = ArtifactContribution {
        kind: ProductKind::Declarations,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let set = publish(&plan, vec![runtime, declarations]).expect("publish succeeds");
    assert_eq!(set.artifacts().len(), 2);
    assert!(set.artifact(ProductKind::RuntimeClient).is_some());
    assert!(set.artifact(ProductKind::Declarations).is_some());
    assert!(set.artifact(ProductKind::IdeCompanion).is_none());
}

#[test]
fn missing_planned_artifact_is_refused_and_publishes_nothing() {
    let plan = plan_with(vec![
        CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
        CompileProduct::IdeCompanion(IdeProductRequest::default()),
    ]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    // Only RuntimeClient supplied; IdeCompanion was planned too.
    let err = publish(&plan, vec![contribution]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::MissingPlannedArtifact {
            kind: ProductKind::IdeCompanion
        }
    );
}

#[test]
fn unplanned_artifact_contribution_is_refused() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    // The plan is satisfied (RuntimeClient present); the extra
    // IdeCompanion contribution was never requested at all.
    let runtime = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let unplanned = ArtifactContribution {
        kind: ProductKind::IdeCompanion,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: Some("{}".to_string()),
        runtime_source_map: None,
    };
    let err = publish(&plan, vec![runtime, unplanned]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::UnplannedArtifactProduced {
            kind: ProductKind::IdeCompanion
        }
    );
}

#[test]
fn duplicate_contribution_for_the_same_kind_is_refused() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let first = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let second = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default { a: 1 }".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let err = publish(&plan, vec![first, second]).unwrap_err();
    assert_eq!(
        err,
        AssemblyRefusal::DuplicateArtifactContribution {
            kind: ProductKind::RuntimeClient
        }
    );
}

#[test]
fn a_single_contribution_per_kind_still_publishes_cleanly() {
    // Positive control for the duplicate-cardinality refusal above.
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "export default {}".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    assert!(publish(&plan, vec![contribution]).is_ok());
}

#[test]
fn code_bearing_artifact_that_fails_to_parse_is_refused() {
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        // Genuinely malformed — not a stand-in for "too simple".
        code: "export default {".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    let err = publish(&plan, vec![contribution]).unwrap_err();
    assert!(matches!(
        err,
        AssemblyRefusal::FinalParseFailed {
            kind: ProductKind::RuntimeClient,
            ..
        }
    ));
}

#[test]
fn code_bearing_artifact_that_parses_cleanly_publishes() {
    // Positive control for the final-parse refusal above.
    let plan = plan_with(vec![CompileProduct::RuntimeClient(
        RuntimeProductRequest::default(),
    )]);
    let contribution = ArtifactContribution {
        kind: ProductKind::RuntimeClient,
        fragments: vec![],
        code: "const _sfc_main = {}\nexport default _sfc_main".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    assert!(publish(&plan, vec![contribution]).is_ok());
}

#[test]
fn non_code_bearing_artifact_is_not_final_parse_checked() {
    // `Analysis` may carry a non-JS payload — publish must not treat
    // it as ECMAScript and refuse it for failing to parse as such.
    let plan = plan_with(vec![CompileProduct::Analysis(Default::default())]);
    let contribution = ArtifactContribution {
        kind: ProductKind::Analysis,
        fragments: vec![],
        code: "{ not json or js at all !!".to_string(),
        emitted_imports: vec![],
        dialect: FragmentDialect::Tsx,
        source_projection_map: None,
        runtime_source_map: None,
    };
    assert!(publish(&plan, vec![contribution]).is_ok());
}
