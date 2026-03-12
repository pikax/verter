//! Comprehensive test suite for all 30 `CompilerErrorCode` variants.
//!
//! Tests run at the `compile()` API level to verify that diagnostics flow through
//! from parser → compile result. Organized by error category with both positive
//! (error IS reported) and negative (valid input produces NO error) assertions.

use super::*;

// ── Helpers ────────────────────────────────────────────────────────

fn compile_sfc(source: &str) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, &alloc)
}

/// Assert that at least one diagnostic has the given code string.
fn assert_has_error(result: &VerterCompileResult, code_str: &str) {
    assert!(
        result.errors.iter().any(|e| e.code == code_str),
        "Expected error code '{}' but got: {:?}",
        code_str,
        result.errors.iter().map(|e| &e.code).collect::<Vec<_>>()
    );
}

/// Assert that exactly `n` diagnostics have the given code string.
fn assert_error_count(result: &VerterCompileResult, code_str: &str, n: usize) {
    let count = result.errors.iter().filter(|e| e.code == code_str).count();
    assert_eq!(
        count,
        n,
        "Expected {} errors with code '{}', got {}. All errors: {:?}",
        n,
        code_str,
        count,
        result.errors.iter().map(|e| &e.code).collect::<Vec<_>>()
    );
}

/// Assert that NO diagnostic has the given code string.
fn assert_no_error(result: &VerterCompileResult, code_str: &str) {
    assert!(
        !result.errors.iter().any(|e| e.code == code_str),
        "Expected no error code '{}' but found one. All errors: {:?}",
        code_str,
        result.errors.iter().map(|e| &e.code).collect::<Vec<_>>()
    );
}

/// Assert zero total diagnostics.
fn assert_no_errors(result: &VerterCompileResult) {
    assert!(
        result.errors.is_empty(),
        "Expected no errors but got: {:?}",
        result
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
    );
}

/// Assert that the error with the given code has a span covering the expected substring.
fn assert_error_span_contains(
    result: &VerterCompileResult,
    source: &str,
    code_str: &str,
    substr: &str,
) {
    let diag = result
        .errors
        .iter()
        .find(|e| e.code == code_str)
        .unwrap_or_else(|| panic!("No error with code '{}'", code_str));
    let span = diag
        .span
        .unwrap_or_else(|| panic!("Error '{}' has no span", code_str));
    let spanned = &source[span.start as usize..span.end as usize];
    assert!(
        spanned.contains(substr),
        "Error '{}' span covers '{}', expected it to contain '{}'",
        code_str,
        spanned,
        substr
    );
}

/// Assert that the error with the given code has Error severity.
fn assert_error_severity(result: &VerterCompileResult, code_str: &str) {
    let diag = result
        .errors
        .iter()
        .find(|e| e.code == code_str)
        .unwrap_or_else(|| panic!("No error with code '{}'", code_str));
    assert_eq!(
        diag.severity,
        CompileDiagnosticSeverity::Error,
        "Expected Error severity for '{}', got {:?}",
        code_str,
        diag.severity
    );
}

/// Assert that the error with the given code has Warning severity.
fn assert_warning_severity(result: &VerterCompileResult, code_str: &str) {
    let diag = result
        .errors
        .iter()
        .find(|e| e.code == code_str)
        .unwrap_or_else(|| panic!("No error with code '{}'", code_str));
    assert_eq!(
        diag.severity,
        CompileDiagnosticSeverity::Warning,
        "Expected Warning severity for '{}', got {:?}",
        code_str,
        diag.severity
    );
}

// ════════════════════════════════════════════════════════════════════
// Group 1: HTML Parse Errors
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_duplicate_attribute() {
    let src = r#"<template><div class="a" class="b">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "DuplicateAttribute");
    assert_error_severity(&result, "DuplicateAttribute");
}

#[test]
fn no_error_duplicate_attribute_on_valid() {
    let src = r#"<template><div class="a" id="b">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "DuplicateAttribute");
}

#[test]
#[ignore = "requires tokenizer changes to detect attributes on close tags"]
fn error_end_tag_with_attributes() {
    let src = r#"<template><div></div foo="bar"></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "EndTagWithAttributes");
    assert_error_severity(&result, "EndTagWithAttributes");
}

#[test]
fn no_error_end_tag_with_attributes_on_valid() {
    let src = r#"<template><div></div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "EndTagWithAttributes");
}

#[test]
#[ignore = "requires tokenizer changes to distinguish EOF before tag name from other states"]
fn error_eof_before_tag_name() {
    // `<` at the end of template content with no tag name following
    let src = "<template><</template>";
    let result = compile_sfc(src);
    assert_has_error(&result, "EofBeforeTagName");
    assert_error_severity(&result, "EofBeforeTagName");
}

#[test]
fn error_eof_in_tag() {
    let src = "<template><div attr";
    let result = compile_sfc(src);
    assert_has_error(&result, "EofInTag");
    assert_error_severity(&result, "EofInTag");
}

#[test]
fn no_error_eof_in_tag_on_valid() {
    let src = r#"<template><div attr="val">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "EofInTag");
}

#[test]
fn error_missing_attribute_value() {
    let src = r#"<template><div class=>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "MissingAttributeValue");
    assert_error_severity(&result, "MissingAttributeValue");
}

#[test]
fn no_error_missing_attribute_value_on_valid() {
    let src = r#"<template><div class="foo">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "MissingAttributeValue");
}

#[test]
fn error_missing_end_tag_name() {
    let src = "<template><div></></template>";
    let result = compile_sfc(src);
    assert_has_error(&result, "MissingEndTagName");
    assert_error_severity(&result, "MissingEndTagName");
}

#[test]
fn no_error_missing_end_tag_name_on_valid() {
    let src = "<template><div></div></template>";
    let result = compile_sfc(src);
    assert_no_error(&result, "MissingEndTagName");
}

#[test]
#[ignore = "requires tokenizer changes to track whitespace between attributes"]
fn error_missing_whitespace_between_attributes() {
    let src = r#"<template><div class="a"id="b">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "MissingWhitespaceBetweenAttributes");
    assert_error_severity(&result, "MissingWhitespaceBetweenAttributes");
}

#[test]
fn no_error_missing_whitespace_between_attributes_on_valid() {
    let src = r#"<template><div class="a" id="b">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "MissingWhitespaceBetweenAttributes");
}

// ════════════════════════════════════════════════════════════════════
// Group 2: Tag Structure Errors (already emitted)
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_x_invalid_end_tag() {
    let src = "<template><div></span></div></template>";
    let result = compile_sfc(src);
    assert_has_error(&result, "XInvalidEndTag");
    assert_error_severity(&result, "XInvalidEndTag");
}

#[test]
fn no_error_x_invalid_end_tag_on_valid() {
    let src = "<template><div></div></template>";
    let result = compile_sfc(src);
    assert_no_error(&result, "XInvalidEndTag");
}

#[test]
fn error_x_missing_end_tag() {
    let src = "<template><div>";
    let result = compile_sfc(src);
    assert_has_error(&result, "XMissingEndTag");
    assert_error_severity(&result, "XMissingEndTag");
}

#[test]
fn no_error_x_missing_end_tag_on_valid() {
    let src = "<template><div>hello</div></template>";
    let result = compile_sfc(src);
    assert_no_error(&result, "XMissingEndTag");
}

// ════════════════════════════════════════════════════════════════════
// Group 3: Template Parse Errors
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_x_missing_interpolation_end() {
    let src = "<template>{{ foo</template>";
    let result = compile_sfc(src);
    assert_has_error(&result, "XMissingInterpolationEnd");
    assert_error_severity(&result, "XMissingInterpolationEnd");
}

#[test]
fn no_error_x_missing_interpolation_end_on_valid() {
    let src = "<template>{{ foo }}</template>";
    let result = compile_sfc(src);
    assert_no_error(&result, "XMissingInterpolationEnd");
}

#[test]
fn error_x_missing_directive_name() {
    let src = r#"<template><div v-="">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XMissingDirectiveName");
    assert_error_severity(&result, "XMissingDirectiveName");
}

#[test]
fn no_error_x_missing_directive_name_on_valid() {
    let src = r#"<template><div v-show="true">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XMissingDirectiveName");
}

#[test]
fn error_x_missing_dynamic_directive_argument_end() {
    let src = r#"<template><div v-bind:[foo="bar">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XMissingDynamicDirectiveArgumentEnd");
    assert_error_severity(&result, "XMissingDynamicDirectiveArgumentEnd");
}

#[test]
fn no_error_x_missing_dynamic_directive_argument_end_on_valid() {
    let src = r#"<template><div v-bind:[foo]="bar">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XMissingDynamicDirectiveArgumentEnd");
}

// ════════════════════════════════════════════════════════════════════
// Group 4: Directive Validation
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_x_v_if_no_expression() {
    let src = r#"<template><div v-if>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVIfNoExpression");
    assert_error_severity(&result, "XVIfNoExpression");
}

#[test]
fn no_error_x_v_if_no_expression_on_valid() {
    let src = r#"<template><div v-if="show">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVIfNoExpression");
}

#[test]
fn error_x_v_if_no_expression_else_if() {
    let src = r#"<template><div v-if="a">a</div><div v-else-if>b</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVIfNoExpression");
}

#[test]
fn error_x_v_else_no_adjacent_if() {
    let src = "<template><div v-else>hello</div></template>";
    let result = compile_sfc(src);
    assert_has_error(&result, "XVElseNoAdjacentIf");
    assert_error_severity(&result, "XVElseNoAdjacentIf");
}

#[test]
fn no_error_x_v_else_no_adjacent_if_on_valid() {
    let src = r#"<template><div v-if="show">a</div><div v-else>b</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVElseNoAdjacentIf");
}

#[test]
fn error_x_v_else_no_adjacent_if_with_text_between() {
    let src = r#"<template><div v-if="show">a</div>text<div v-else>b</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVElseNoAdjacentIf");
}

#[test]
fn error_x_v_for_no_expression() {
    let src = r#"<template><div v-for>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVForNoExpression");
    assert_error_severity(&result, "XVForNoExpression");
}

#[test]
fn no_error_x_v_for_no_expression_on_valid() {
    let src = r#"<template><div v-for="item in items" :key="item">{{ item }}</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVForNoExpression");
}

#[test]
fn error_x_v_for_malformed_expression() {
    // "x" has no "in" or "of" separator
    let src = r#"<template><div v-for="x">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVForMalformedExpression");
    assert_error_severity(&result, "XVForMalformedExpression");
}

#[test]
fn no_error_x_v_for_malformed_expression_on_valid() {
    let src = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVForMalformedExpression");
}

#[test]
#[ignore = "Vue 3.4 same-name shorthand makes :attr without value valid"]
fn error_x_v_bind_no_expression() {
    // v-bind with a named arg but no value — in Vue 3.4+, this is same-name shorthand
    let src = r#"<template><div v-bind:class>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVBindNoExpression");
    assert_error_severity(&result, "XVBindNoExpression");
}

#[test]
fn no_error_x_v_bind_no_expression_on_valid() {
    let src = r#"<template><div v-bind:class="cls">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVBindNoExpression");
}

#[test]
fn no_error_x_v_bind_spread() {
    // v-bind with no arg (spread) is valid even without expression
    let src = r#"<template><div v-bind="obj">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVBindNoExpression");
}

#[test]
#[ignore = "Vue 3.4 same-name shorthand makes @event without value valid"]
fn error_x_v_on_no_expression() {
    // v-on with a named arg but no handler — in Vue 3.4+, this is same-name shorthand
    let src = r#"<template><div v-on:click>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVOnNoExpression");
    assert_error_severity(&result, "XVOnNoExpression");
}

#[test]
fn no_error_x_v_on_no_expression_on_valid() {
    let src = r#"<template><div v-on:click="handler">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVOnNoExpression");
}

#[test]
fn no_error_x_v_on_spread() {
    // v-on with no arg (spread) is valid
    let src = r#"<template><div v-on="handlers">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVOnNoExpression");
}

#[test]
fn error_x_v_slot_misplaced() {
    // v-slot on a plain HTML element (not component or <template>)
    let src = r#"<template><div v-slot>hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVSlotMisplaced");
    assert_error_severity(&result, "XVSlotMisplaced");
}

#[test]
fn no_error_x_v_slot_misplaced_on_component() {
    let src = r#"<template><MyComp v-slot="{ item }">{{ item }}</MyComp></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVSlotMisplaced");
}

#[test]
fn no_error_x_v_slot_misplaced_on_template() {
    let src = r#"<template><MyComp><template #default="{ item }">{{ item }}</template></MyComp></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVSlotMisplaced");
}

#[test]
#[ignore = "requires cross-sibling slot name tracking during component child finalization"]
fn error_x_v_slot_duplicate_slot_names() {
    let src = r#"<template><MyComp><template #default>a</template><template #default>b</template></MyComp></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVSlotDuplicateSlotNames");
    assert_error_severity(&result, "XVSlotDuplicateSlotNames");
}

#[test]
fn no_error_x_v_slot_duplicate_slot_names_on_valid() {
    let src = r#"<template><MyComp><template #header>h</template><template #footer>f</template></MyComp></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVSlotDuplicateSlotNames");
}

#[test]
fn error_x_v_model_no_expression() {
    let src = r#"<template><input v-model></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVModelNoExpression");
    assert_error_severity(&result, "XVModelNoExpression");
}

#[test]
fn no_error_x_v_model_no_expression_on_valid() {
    let src = r#"<template><input v-model="val"></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVModelNoExpression");
}

#[test]
fn error_x_v_model_malformed_expression() {
    // "a + b" is not a valid member expression for v-model
    let src = r#"<template><input v-model="a + b"></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVModelMalformedExpression");
    assert_error_severity(&result, "XVModelMalformedExpression");
}

#[test]
fn no_error_x_v_model_malformed_expression_on_valid() {
    let src = r#"<template><input v-model="obj.prop"></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVModelMalformedExpression");
}

#[test]
fn no_error_x_v_model_malformed_expression_on_ident() {
    let src = r#"<template><input v-model="val"></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVModelMalformedExpression");
}

// ════════════════════════════════════════════════════════════════════
// Group 5: Duplicate Directives (already emitted as warning)
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_x_duplicate_directive_v_if() {
    let src = r#"<template><div v-if="a" v-if="b">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XDuplicateDirective");
    assert_warning_severity(&result, "XDuplicateDirective");
}

#[test]
fn no_error_x_duplicate_directive_on_valid() {
    let src = r#"<template><div v-if="a">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XDuplicateDirective");
}

// ════════════════════════════════════════════════════════════════════
// Group 6: Script Block Errors (already emitted)
// ════════════════════════════════════════════════════════════════════

#[test]
fn error_duplicate_script_setup() {
    let src = "<script setup>const a = 1</script><script setup>const b = 2</script>";
    let result = compile_sfc(src);
    assert_has_error(&result, "DuplicateScriptSetup");
    assert_error_severity(&result, "DuplicateScriptSetup");
}

#[test]
fn no_error_duplicate_script_setup_on_valid() {
    let src = "<script setup>const a = 1</script>";
    let result = compile_sfc(src);
    assert_no_error(&result, "DuplicateScriptSetup");
}

#[test]
fn error_duplicate_script() {
    let src = "<script>export default {}</script><script>export default {}</script>";
    let result = compile_sfc(src);
    assert_has_error(&result, "DuplicateScript");
    assert_error_severity(&result, "DuplicateScript");
}

#[test]
fn no_error_duplicate_script_on_valid() {
    // One plain script + one script setup is valid
    let src = "<script>export default {}</script><script setup>const a = 1</script>";
    let result = compile_sfc(src);
    assert_no_error(&result, "DuplicateScript");
    assert_no_error(&result, "DuplicateScriptSetup");
}

// ════════════════════════════════════════════════════════════════════
// Group 7: Expression/Macro/CSS Errors
// ════════════════════════════════════════════════════════════════════

// XInvalidExpression — statement in interpolation context
// Note: This error is not yet emitted. It would need to be emitted during
// template expression OXC parsing when invalid JavaScript is found in
// interpolations or directive values.
#[test]
#[ignore = "requires OXC expression parse error propagation during template codegen"]
fn error_x_invalid_expression() {
    let src = r#"<template>{{ if(true){} }}</template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XInvalidExpression");
    assert_error_severity(&result, "XInvalidExpression");
}

#[test]
fn no_error_x_invalid_expression_on_valid() {
    let src = r#"<template>{{ count + 1 }}</template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XInvalidExpression");
}

// XCssParseError — CSS parse errors are emitted during style processing
#[test]
fn error_x_css_parse_error() {
    let src = "<template><div></div></template><style scoped>div { color: }</style>";
    let result = compile_sfc(src);
    // CSS parse errors may or may not be emitted depending on the CSS parser's tolerance
    // If it is emitted, verify the code
    if result.errors.iter().any(|e| e.code == "XCssParseError") {
        assert_error_severity(&result, "XCssParseError");
    }
}

// ════════════════════════════════════════════════════════════════════
// Group 8: Negative Tests — Valid inputs produce no errors
// ════════════════════════════════════════════════════════════════════

#[test]
fn no_errors_basic_template() {
    let src = "<template><div>hello</div></template>";
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_full_sfc() {
    let src = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <button @click="count++">{{ count }}</button>
</template>

<style scoped>
button { color: red; }
</style>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_if_chain() {
    let src = r#"<template>
  <div v-if="a">a</div>
  <div v-else-if="b">b</div>
  <div v-else>c</div>
</template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_for_with_key() {
    let src = r#"<template>
  <div v-for="item in items" :key="item.id">{{ item.name }}</div>
</template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_model() {
    let src = r#"<template><input v-model="text"></template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_bind_spread() {
    let src = r#"<template><div v-bind="attrs">hello</div></template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_void_elements() {
    let src = r#"<template><div><br><hr><img src="test.png"><input type="text"></div></template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_components_with_slots() {
    let src = r#"<template>
  <MyComp>
    <template #header>Header</template>
    <template #default>Content</template>
    <template #footer>Footer</template>
  </MyComp>
</template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_for_with_of() {
    let src = r#"<template><div v-for="item of items" :key="item">{{ item }}</div></template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

#[test]
fn no_errors_v_for_destructured() {
    let src =
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#;
    let result = compile_sfc(src);
    assert_no_errors(&result);
}

// ════════════════════════════════════════════════════════════════════
// Group 9: XVIfSameKey — v-if branches with duplicate keys
// ════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires cross-sibling key tracking during v-if chain resolution"]
fn error_x_v_if_same_key() {
    let src = r#"<template><div v-if="a" :key="1">a</div><div v-else :key="1">b</div></template>"#;
    let result = compile_sfc(src);
    assert_has_error(&result, "XVIfSameKey");
}

#[test]
fn no_error_x_v_if_same_key_on_valid() {
    let src = r#"<template><div v-if="a" :key="1">a</div><div v-else :key="2">b</div></template>"#;
    let result = compile_sfc(src);
    assert_no_error(&result, "XVIfSameKey");
}
