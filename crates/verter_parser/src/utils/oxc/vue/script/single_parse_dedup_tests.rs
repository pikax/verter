//! Discriminating tests pinning the single-parse dedup contract: one
//! `parse_script_with_companion(Setup)` builds the type-resolution context
//! once and resolves each macro type argument once — never twice.
//!
//! Before the dedup these counts were 2/2 (the setup-statement pass and the
//! binding pass each built their own context and independently re-resolved
//! `defineProps<T>`). After the dedup they are 1/1.

use super::{parse_script_with_companion, ScriptMode};
use crate::utils::oxc::script::type_surface::call_counters;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// One parse of a `defineProps<{ ... }>()` setup must build the type context
/// exactly once and resolve the macro type argument exactly once.
#[test]
fn setup_parse_builds_type_context_and_resolves_macro_once() {
    let source = "const props = defineProps<{ a: string; b: number }>()";
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
    assert!(ret.errors.is_empty(), "parse errors: {:?}", ret.errors);

    call_counters::reset();
    let result = parse_script_with_companion(&ret.program, ScriptMode::Setup, 0, source, None);

    assert_eq!(
        call_counters::build_type_context_calls(),
        1,
        "build_type_context must run exactly once per setup parse, not once per consumer"
    );
    assert_eq!(
        call_counters::resolve_type_elements_calls(),
        1,
        "the macro type argument must resolve exactly once, not once per consumer"
    );

    // Negative guard: the single resolution must still produce the real props,
    // so the counts above reflect genuine work — not a skipped resolution.
    let prop_names: Vec<&str> = result
        .bindings
        .iter()
        .filter(|(_, bt)| *bt == crate::types::BindingType::Props)
        .map(|(span, _)| &source[span.start as usize..span.end as usize])
        .collect();
    assert!(
        prop_names.contains(&"a") && prop_names.contains(&"b"),
        "expected props a and b to be resolved once, got {prop_names:?}"
    );
}

/// A standalone `defineProps<T>()` (no declarator) exercises the same dedup:
/// both the setup pass and the binding pass reach the macro type argument, but
/// it must still resolve exactly once.
#[test]
fn standalone_define_props_resolves_macro_once() {
    let source = "defineProps<{ title: string }>()";
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
    assert!(ret.errors.is_empty(), "parse errors: {:?}", ret.errors);

    call_counters::reset();
    let _ = parse_script_with_companion(&ret.program, ScriptMode::Setup, 0, source, None);

    assert_eq!(call_counters::build_type_context_calls(), 1);
    assert_eq!(call_counters::resolve_type_elements_calls(), 1);
}
