//! Nested `:global` / `:deep` / `:slotted` facts stay on the typed list;
//! projecting them must not start a second `parse_selector_structure`.

use verter_css_syntax::{
    parse_selector_structure_thread_invocations, parse_style_ir_thread_invocations,
};
use verter_semantic::analysis::{
    build_css_style_analysis, SpecialPseudoKind, StyleAnalysisFlags, VueStyleInput,
};

#[test]
fn nested_special_pseudo_facts_unchanged_and_no_secondary_parse() {
    let src = ".a :global(.g) { color: red; }\n:deep(.d :slotted(.s)) { color: blue; }";
    let parses_before = parse_style_ir_thread_invocations();
    let secondary_before = parse_selector_structure_thread_invocations();
    let analysis = build_css_style_analysis(src, VueStyleInput::default(), true, false, None, 0);
    assert_eq!(
        parse_style_ir_thread_invocations() - parses_before,
        1,
        "canonical project_style parse stays exactly 1"
    );
    assert_eq!(
        parse_selector_structure_thread_invocations() - secondary_before,
        0,
        "nested special-pseudo facts must not start a secondary selector parse"
    );

    let kinds: Vec<_> = analysis.special_pseudos.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&SpecialPseudoKind::Global));
    assert!(kinds.contains(&SpecialPseudoKind::Deep));
    assert!(kinds.contains(&SpecialPseudoKind::Slotted));
    let flags = analysis.analysis_flags();
    assert!(flags.contains(StyleAnalysisFlags::HAS_GLOBAL));
    assert!(flags.contains(StyleAnalysisFlags::HAS_DEEP));
    assert!(flags.contains(StyleAnalysisFlags::HAS_SLOTTED));

    let css = analysis.css.as_ref().expect("css analysis");
    let names: Vec<&str> = css.classes.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"g"));
    assert!(names.contains(&"d"));
    assert!(names.contains(&"s"));
}
