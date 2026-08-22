/**
 * @ai-generated - J1 A10a: Native CSS-Modules class *analysis* extends to all
 * 5 native dialects, unconditional on `dialect == Css` — one named test per
 * dialect. Analysis only; runtime class-name rewriting (row 19, `css/modules.rs`)
 * is untouched by this suite.
 */
use verter_compiler::style_planner::{analyze_css_module_classes, AuthoredStyleInput};
use verter_css_syntax::CssDialect;

fn assert_analyzes_active_class(dialect: CssDialect, source: &str) {
    let input = AuthoredStyleInput::new(
        source,
        dialect,
        "probe.style",
        "space:probe",
        "artifact:probe",
    );
    let classes = analyze_css_module_classes(input, "sc1")
        .unwrap_or_else(|e| panic!("{dialect:?} class analysis must not be refused: {e}"));
    assert_eq!(classes.len(), 1, "{dialect:?}: {classes:?}");
    assert_eq!(classes[0].0, "active", "{dialect:?}: {classes:?}");
    assert_ne!(
        classes[0].1, "active",
        "{dialect:?}: hashed name must not pass the class name through unhashed"
    );
}

#[test]
fn module_class_analysis_native_for_css() {
    assert_analyzes_active_class(CssDialect::Css, ".active { color: red; }");
}

#[test]
fn module_class_analysis_native_for_scss() {
    assert_analyzes_active_class(CssDialect::Scss, ".active { color: red; }");
}

#[test]
fn module_class_analysis_native_for_sass() {
    assert_analyzes_active_class(CssDialect::Sass, ".active\n  color: red\n");
}

#[test]
fn module_class_analysis_native_for_less() {
    assert_analyzes_active_class(CssDialect::Less, ".active { color: red; }");
}

#[test]
fn module_class_analysis_native_for_stylus() {
    assert_analyzes_active_class(CssDialect::Stylus, ".active\n  color: red\n");
}

// @ai-generated - The shared module-class walk's refusal must report the
// AUTHORED dialect it was invoked with, not a hardcoded `CssDialect::Css` —
// a class-analysis refusal for SCSS/Sass/Less/Stylus input previously always
// reported `CssDialect::Css` regardless of what was actually being analyzed.
#[test]
fn module_class_analysis_refusal_reports_the_authored_dialect() {
    let input = AuthoredStyleInput::new(
        ".icon-#{$name} { color: red; }",
        CssDialect::Scss,
        "probe.style",
        "space:probe",
        "artifact:probe",
    );
    let error = analyze_css_module_classes(input, "sc1")
        .expect_err("an interpolated class selector must refuse module-class analysis");
    assert_eq!(
        error.dialect,
        CssDialect::Scss,
        "refusal must report the authored SCSS dialect, not CssDialect::Css: {error:?}"
    );
}
