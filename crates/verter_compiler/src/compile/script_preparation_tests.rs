//! Tests for the single script/macro preparation lane.
//!
//! The setup (`<script setup>`) and companion (`<script>`) blocks are parsed
//! once into the compile allocator and shared by every consumer: the
//! invalid-macro-type diagnostics, the script-codegen macro surfaces, and the
//! force-js type-stripping inputs. These tests pin the two contracts that the
//! shared parse must not regress:
//!
//! 1. The public `XInvalidMacroType` diagnostic is unchanged on every
//!    [`CompileTarget`] — both its presence and its content-local span.
//! 2. A full compile OXC-parses each script block exactly once, no matter how
//!    many consumers read from it.

use super::*;
use crate::common::Span;
use crate::utils::oxc::script::type_surface::ResolvedElements;
use rustc_hash::FxHashMap;

/// Resolve a single external type, mirroring the host-provided `external_types`
/// map a real workspace compile supplies for a cross-file macro argument.
fn make_external_types(type_name: &str, dep_source: &str) -> FxHashMap<String, ResolvedElements> {
    let alloc = Allocator::default();
    let resolved = crate::utils::oxc::script::type_surface::resolve_external_type(
        type_name, dep_source, &alloc,
    )
    .expect("failed to resolve external type");
    let mut map = FxHashMap::default();
    map.insert(type_name.to_string(), resolved);
    map
}

fn compile_with(
    source: &str,
    target: CompileTarget,
    external_types: Option<FxHashMap<String, ResolvedElements>>,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        target,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        external_types,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// Every artifact-target bitset/preset that public compile paths use. The
/// invalid-macro-type diagnostic must surface on all of them — artifact-target
/// bits never gate the diagnostic. The final entry is the combined preset the
/// FFI `full` input maps to (`BUNDLER | TSX | TEMPLATE_DATA`).
fn diagnostic_target_matrix() -> Vec<(&'static str, CompileTarget)> {
    vec![
        ("STYLE", CompileTarget::STYLE),
        ("SCRIPT", CompileTarget::SCRIPT),
        ("TEMPLATE", CompileTarget::TEMPLATE),
        ("TSX", CompileTarget::TSX),
        ("TSC", CompileTarget::TSC),
        ("TEMPLATE_DATA", CompileTarget::TEMPLATE_DATA),
        ("BUNDLER", CompileTarget::BUNDLER),
        ("IDE", CompileTarget::IDE),
        ("ANALYSIS", CompileTarget::ANALYSIS),
        ("META", CompileTarget::META),
        (
            "BUNDLER|TSX|TEMPLATE_DATA",
            CompileTarget::BUNDLER | CompileTarget::TSX | CompileTarget::TEMPLATE_DATA,
        ),
    ]
}

const INVALID_PROPS_SFC: &str = "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>";

const INVALID_EMITS_SFC: &str = "<script setup lang=\"ts\">\nimport type { ExternalEmits } from './types'\ndefineEmits<ExternalEmits>()\n</script>\n<template><div>x</div></template>";

/// An unresolvable imported `defineProps<T>` surfaces `XInvalidMacroType` for
/// every target — the artifact-target bits never gate the diagnostic.
#[test]
fn invalid_define_props_surfaces_on_every_target() {
    for (name, target) in diagnostic_target_matrix() {
        let result = compile_with(INVALID_PROPS_SFC, target, None);
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code == "XInvalidMacroType"),
            "XInvalidMacroType must surface for target {name}; got {:?}",
            result.errors
        );
    }
}

/// An invalid imported `defineEmits<T>` (resolves to `string`, not emit
/// signatures) surfaces `XInvalidMacroType` for every target.
#[test]
fn invalid_define_emits_surfaces_on_every_target() {
    for (name, target) in diagnostic_target_matrix() {
        let external_types =
            make_external_types("ExternalEmits", "export type ExternalEmits = string");
        let result = compile_with(INVALID_EMITS_SFC, target, Some(external_types));
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.code == "XInvalidMacroType"),
            "XInvalidMacroType must surface for target {name}; got {:?}",
            result.errors
        );
    }
}

/// The `XInvalidMacroType` diagnostic span is content-local — relative to the
/// setup block content, not the whole SFC. The prepared setup parse runs at the
/// SFC content offset (so its spans are SFC-absolute for the codegen lanes), but
/// the public diagnostic span is localized back to content coordinates, leaving
/// it byte-identical to the historical span.
#[test]
fn invalid_macro_type_diagnostic_span_is_content_local() {
    let result = compile_with(INVALID_PROPS_SFC, CompileTarget::BUNDLER, None);

    let diagnostic = result
        .errors
        .iter()
        .find(|error| error.code == "XInvalidMacroType")
        .expect("expected an XInvalidMacroType diagnostic");
    let span = diagnostic
        .span
        .expect("XInvalidMacroType diagnostic must carry a span");

    // The setup block does not start at offset 0, so content-local and
    // SFC-absolute coordinates are distinguishable.
    let content_start = INVALID_PROPS_SFC.find('>').expect("setup open tag") + 1;
    let content_end = INVALID_PROPS_SFC
        .find("</script>")
        .expect("setup close tag");
    assert!(content_start > 0, "setup block must not begin at offset 0");
    let setup_content = &INVALID_PROPS_SFC[content_start..content_end];

    // The exact content-local span covers the `ExternalProps` type argument
    // inside `defineProps<…>` (the last occurrence — the first is the import).
    let type_arg_local = setup_content
        .rfind("ExternalProps")
        .expect("type argument occurrence in setup content");
    let expected = Span::new(
        type_arg_local as u32,
        (type_arg_local + "ExternalProps".len()) as u32,
    );
    assert_eq!(
        span, expected,
        "diagnostic span must be byte-identical to the base content-local coordinates"
    );

    // Negative: the span is NOT SFC-absolute. An SFC-absolute span would be
    // shifted past the end of the setup content slice.
    assert!(
        (span.end as usize) <= setup_content.len(),
        "content-local span {:?} must fit within the {}-byte setup content; \
         an SFC-absolute span overshoots by content_start ({content_start})",
        span,
        setup_content.len()
    );
    assert_eq!(
        setup_content[span.start as usize..span.end as usize].trim(),
        "ExternalProps",
        "content-local span must cover the offending type argument"
    );
}

/// `withDefaults(defineProps<Imported>(), getDefaults())` with an unresolvable
/// imported type and a defaults fallback must NOT surface `XInvalidMacroType` —
/// the runtime props synthesize from the defaults expression. This suppression
/// holds identically on every target.
#[test]
fn with_defaults_unresolved_import_suppression_holds_on_every_target() {
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nimport { getDefaults } from './defaults'\n\nconst props = withDefaults(defineProps<Props>(), getDefaults())\n</script>\n<template><div>{{ props.foo }}</div></template>";
    for (name, target) in diagnostic_target_matrix() {
        let result = compile_with(source, target, None);
        assert!(
            !result
                .errors
                .iter()
                .any(|error| error.code == "XInvalidMacroType"),
            "withDefaults fallback must suppress XInvalidMacroType for target {name}; got {:?}",
            result.errors
        );
    }
}

/// A full compile (diagnostics + script codegen + force-js lanes all active)
/// OXC-parses the setup block exactly once and the companion block exactly once.
/// Before the shared preparation lane each consumer re-parsed the same content
/// (the duplication map recorded five setup parses and three companion parses
/// for this shape); now all three lanes read from the single prepared parse.
#[test]
fn full_compile_prepares_each_script_block_once() {
    use crate::script::prepared::parse_counters;

    let source = "<script lang=\"ts\">\nexport interface CompanionProps { a: string }\n</script>\n<script setup lang=\"ts\">\nconst p = defineProps<CompanionProps>()\n</script>\n<template><div>{{ p.a }}</div></template>";

    // BUNDLER + force_js is exactly the path where the duplication lived: the
    // style/script/template runtime lanes plus the force-js strip. The IDE (TSX)
    // and TSC backends are separate codegen paths with their own parses, out of
    // scope for the shared script-preparation lane.
    parse_counters::reset();
    let result = compile_with(source, CompileTarget::BUNDLER, None);

    assert_eq!(
        parse_counters::setup_block_parses(),
        1,
        "setup block must be OXC-parsed exactly once across diagnostics + codegen + force-js"
    );
    assert_eq!(
        parse_counters::companion_block_parses(),
        1,
        "companion block must be OXC-parsed exactly once"
    );

    // Negative guard: the single parse still produced real work — the companion
    // type resolved into the props binding, so the counts reflect a genuine
    // shared parse, not a skipped one.
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("__props"),
        "defineProps must lower against the shared parse.\nOutput:\n{}",
        script.code
    );
}

/// The parse counter tracks reality, not a constant: an SFC with no companion
/// `<script>` parses the setup once and the companion zero times.
#[test]
fn compile_without_companion_parses_no_companion_block() {
    use crate::script::prepared::parse_counters;

    let source = "<script setup lang=\"ts\">\nconst p = defineProps<{ a: string }>()\n</script>\n<template><div>{{ p.a }}</div></template>";

    parse_counters::reset();
    let _ = compile_with(source, CompileTarget::BUNDLER, None);

    assert_eq!(
        parse_counters::setup_block_parses(),
        1,
        "setup block must be OXC-parsed exactly once"
    );
    assert_eq!(
        parse_counters::companion_block_parses(),
        0,
        "no companion block present — it must never be parsed"
    );
}
