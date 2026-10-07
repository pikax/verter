use super::*;

#[test]
pub(super) fn style_block_extracted() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    assert!(result.styles[0].scoped);
    assert!(!result.scope_id.is_empty());
}

#[test]
fn scoped_css_no_double_data_v_prefix() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div class="app">{{ msg }}</div>
</template>

<style scoped>
.app { color: red; }
</style>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    let css = result.styles[0].code();
    assert!(
        !css.contains("data-v-data-v-"),
        "CSS should not contain double data-v- prefix: {}",
        css
    );
    assert!(
        css.contains("[data-v-"),
        "CSS should contain scoped attribute selector: {}",
        css
    );
}

// ==================== v-for block scoping ====================

// @ai-generated - Native elements with v-for need their own block scope
#[test]
fn vfor_native_element_uses_block_scope() {
    let result = compile_sfc(
        r#"<template><div><div v-for="item in items" :key="item.id">{{ item.name }}</div></div></template>
<script setup>const items = ref([])</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_openBlock()") && tpl.code.contains("_createElementBlock("),
        "v-for native element should use (_openBlock(), _createElementBlock()), got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Scoped style scope_id in script must include data-v- prefix
/// and must match the scope_id used in CSS selectors.
#[test]
fn scoped_style_scope_id_matches_between_script_and_css() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>
<style scoped>.app { color: red; }</style>"#,
    );

    let script = result.script.as_ref().expect("script block");
    let style = result.styles.first().expect("should have a style block");

    // Script must emit __scopeId with data-v- prefix
    let scope_marker = "__scopeId = \"";
    let scope_pos = script.code.find(scope_marker).unwrap_or_else(|| {
        panic!(
            "Script should contain __scopeId assignment, got:\n{}",
            script.code
        )
    });
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("should have closing quote")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];

    assert!(
        script_scope_id.starts_with("data-v-"),
        "Script __scopeId must start with 'data-v-', got: '{}'\nFull script:\n{}",
        script_scope_id,
        script.code
    );

    // CSS must use the same scope_id in its selectors
    let css_marker = "[data-v-";
    let css_pos = style.code().find(css_marker).unwrap_or_else(|| {
        panic!(
            "CSS should contain [data-v-...] selector, got:\n{}",
            style.code()
        )
    });
    let css_id_start = css_pos + 1; // skip '['
    let css_id_end = style.code()[css_id_start..]
        .find(']')
        .expect("should have closing ]")
        + css_id_start;
    let css_scope_id = &style.code()[css_id_start..css_id_end];

    assert_eq!(
        script_scope_id,
        css_scope_id,
        "Script __scopeId and CSS selector scope_id must match.\nScript: {}\nCSS: {}",
        script.code,
        style.code()
    );
}

// @ai-generated - Tests that CSS child combinator is preserved in scoped styles
#[test]
fn scoped_css_child_combinator_preserved() {
    let result = compile_sfc(
        r#"<script setup>
const x = 1
</script>
<template><div class="parent"><span class="child">{{ x }}</span></div></template>
<style scoped>
.parent > .child { color: red; }
</style>"#,
    );
    let css = result.styles[0].code();
    // Must NOT have dangling > after the scope attr
    assert!(
        !css.contains("]>"),
        "Scope attr must not be followed by dangling > combinator.\nCSS: {}",
        css
    );
}

// ==================== Static style → object ====================

#[test]
pub(super) fn static_style_compiled_to_object() {
    // Vue compiles static style="margin-top: 15px" into an object { "margin-top": "15px" }
    // so that SSR can serialize it compactly as margin-top:15px;
    let code = compile_and_validate_template(
        r#"<template><div style="margin-top: 15px">text</div></template>"#,
    );
    // Should produce an object, not a string
    assert!(
        code.contains(r#"{ "margin-top": "15px" }"#),
        "Static style should be compiled to a JS object, not a string\n{}",
        code
    );
    assert!(
        !code.contains(r#"style: "margin-top"#),
        "Static style should NOT be emitted as a string\n{}",
        code
    );
}

#[test]
pub(super) fn static_style_multiple_properties() {
    let code = compile_and_validate_template(
        r#"<template><div style="color: red; font-size: 14px">text</div></template>"#,
    );
    assert!(
        code.contains(r#""color": "red""#),
        "Should parse color property (all style-object keys are quoted, \
         even valid identifiers)\n{}",
        code
    );
    assert!(
        code.contains(r#""font-size": "14px""#),
        "Should parse font-size property (quoted because of hyphen)\n{}",
        code
    );
}

// ==================== JS string escaping in codegen ====================

#[test]
fn style_with_newlines_produces_valid_js() {
    // Regression: ant-design-vue horizontal.vue has style with literal newlines.
    // Verter must escape newlines in style property names to produce valid JS.
    let code = compile_and_validate_template(
        "<template><div style=\"\n  {\n    padding: '20px'\n  }\n\"></div></template>",
    );
    // The output must not contain raw newlines inside string literals
    assert!(
        !code.contains("\"{\n"),
        "style object key must have newlines escaped\n{}",
        code
    );
}

#[test]
fn style_normal_css_produces_valid_js() {
    let code = compile_and_validate_template(
        r#"<template><div style="margin-top: 15px; color: red"></div></template>"#,
    );
    assert!(
        code.contains("\"margin-top\""),
        "hyphenated CSS prop should be quoted\n{}",
        code
    );
    assert!(
        code.contains("\"15px\""),
        "value should be quoted\n{}",
        code
    );
}

#[test]
fn style_with_multiline_value_produces_valid_js() {
    // Multi-line style attribute values are common in formatted templates
    let code = compile_and_validate_template(
        "<template><div style=\"\n  margin-top: 15px;\n  color: red;\n\"></div></template>",
    );
    assert!(
        code.contains("\"margin-top\""),
        "hyphenated prop should be parsed from multiline style\n{}",
        code
    );
}

/// The SAME template-only Vapor case, but ALSO with a scoped style. Real
/// `@vitejs/plugin-vue`'s `__scopeId` is a COMPLETELY DIFFERENT mechanism
/// from `__vapor` — it flows through `attachedProps` + the bundler-level
/// `_export_sfc(_sfc_main, [['__scopeId', …]])` helper, not
/// `compileScript`'s inline `runtimeOptions` string `__vapor` uses
/// (confirmed directly against the real vendored `@vitejs/plugin-vue@6.0.7`
/// source) — so `__scopeId`'s existing emission shape is left untouched
/// here (no seed fixture exercises scoped styles, so there is no confirmed
/// target shape to match here regardless). This test only pins that
/// `__vapor` stays correctly inlined when a scopeId assignment is ALSO
/// present, leaving `__scopeId`'s own emission unaffected.
#[test]
fn template_only_vapor_with_scoped_style_still_inlines_vapor_flag() {
    let result = compile_sfc_vapor(
        "<template><div class=\"app\">x</div></template>\n<style scoped>.app { color: red; }</style>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect(
        "template-only vapor component with scoped style should emit a synthetic script block",
    );
    assert!(
        script.code.contains("__vapor: true"),
        "__vapor must be an inline object-literal property even alongside \
         a scopeId assignment, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("__sfc__.__vapor ="),
        "__vapor must not be a separate trailing assignment, got:\n{}",
        script.code
    );
}

/// Template-only component WITHOUT scoped styles should NOT emit a
/// synthetic script block (no __scopeId needed).
#[test]
pub(super) fn template_only_no_scoped_style_no_script_block() {
    let result = compile_sfc("<template><div>hello</div></template>");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert!(
        result.script.is_none(),
        "template-only without scoped style should not have a script block",
    );
}

/// Template-only with scoped style: CSS should contain scoped selectors.
#[test]
pub(super) fn template_only_scoped_style_css_is_scoped() {
    let result = compile_sfc(
        "<template><div class=\"app\">hello</div></template>\n<style scoped>\n.app { color: red; }\n</style>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    assert_eq!(result.styles.len(), 1);
    assert!(
        result.styles[0].code().contains("[data-v-"),
        "scoped CSS should contain [data-v-] selector, got:\n{}",
        result.styles[0].code()
    );
}

/// @ai-generated - Template-only component with scoped grid CSS: scope IDs must
/// match between script and CSS, and CSS selectors must all be scoped.
#[test]
pub(super) fn template_only_scoped_style_grid_layout_scope_id_consistency() {
    let source = r#"<template>
  <div class="dashboard">
    <header class="header">
      <h1>Title</h1>
    </header>
    <aside class="sidebar">
      <ul class="menu">
        <li class="menu-item active"><span>Overview</span></li>
        <li class="menu-item"><span>Settings</span></li>
      </ul>
    </aside>
    <main class="content">
      <section class="stats-grid">
        <div class="stat-card">
          <h3>Total Users</h3>
          <p class="stat-value">12,345</p>
          <span class="stat-change positive">+12.5%</span>
        </div>
      </section>
      <section class="recent-activity">
        <table class="activity-table">
          <thead><tr><th>User</th><th>Action</th></tr></thead>
          <tbody>
            <tr>
              <td>John</td>
              <td><span class="badge success">Done</span></td>
            </tr>
          </tbody>
        </table>
      </section>
    </main>
    <footer class="footer">
      <p>&copy; 2026</p>
    </footer>
  </div>
</template>

<style scoped>
.dashboard {
  display: grid;
  grid-template-areas:
    "header header"
    "sidebar content"
    "footer footer";
  grid-template-columns: 250px 1fr;
  grid-template-rows: auto 1fr auto;
  min-height: 100vh;
}
.header {
  grid-area: header;
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
  background: #fff;
  border-bottom: 1px solid #ddd;
}
.sidebar { grid-area: sidebar; background: #f8f9fa; padding: 1rem; }
.content { grid-area: content; padding: 2rem; background: #f5f5f5; }
.footer {
  grid-area: footer;
  display: flex;
  justify-content: space-between;
  padding: 1rem 2rem;
  background: #fff;
  border-top: 1px solid #ddd;
}
.stats-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 1rem; }
.stat-card {
  background: white;
  padding: 1.5rem;
  border-radius: 8px;
  box-shadow: 0 2px 4px rgba(0,0,0,0.1);
}
.activity-table { width: 100%; border-collapse: collapse; }
.activity-table th { text-align: left; padding: 0.75rem; border-bottom: 2px solid #ddd; }
.activity-table td { padding: 0.75rem; border-bottom: 1px solid #eee; }
.badge { padding: 0.25rem 0.5rem; border-radius: 4px; font-size: 0.875rem; }
.badge.success { background: #d4edda; color: #155724; }
</style>"#;

    let result = compile_sfc(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );

    // 1. Script must have __scopeId
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script block");
    assert!(
        script.code.contains("__scopeId"),
        "script should contain __scopeId assignment, got:\n{}",
        script.code
    );

    // 2. Template must have a render function
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    assert!(
        template.code.contains("function render"),
        "template should contain render function, got:\n{}",
        template.code
    );

    // 3. Extract scope ID from script
    let scope_marker = "__scopeId = \"";
    let scope_pos = script
        .code
        .find(scope_marker)
        .expect("scope marker in script");
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("closing quote for scope ID")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];
    assert!(
        script_scope_id.starts_with("data-v-"),
        "scope ID must start with data-v-, got: '{}'",
        script_scope_id
    );

    // 4. CSS must have matching scope selectors
    assert_eq!(result.styles.len(), 1);
    let css = result.styles[0].code();
    let css_scope_attr = format!("[{}]", script_scope_id);
    assert!(
        css.contains(&css_scope_attr),
        "CSS must contain scope selector '{}', got:\n{}",
        css_scope_attr,
        css
    );

    // 5. ALL CSS selectors that should be scoped must have the scope attribute.
    //    Check every non-at-rule selector in the CSS output.
    let expected_scoped_selectors = [
        ".dashboard",
        ".header",
        ".sidebar",
        ".content",
        ".footer",
        ".stats-grid",
        ".stat-card",
        // descendant selectors — only last part gets scope
        "th", // from ".activity-table th"
        "td", // from ".activity-table td"
        ".badge",
    ];
    for sel in expected_scoped_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // 6. Compound selector `.badge.success` should have scope on the compound
    let compound_scoped = format!(".badge.success{}", css_scope_attr);
    assert!(
        css.contains(&compound_scoped),
        "CSS should contain scoped compound selector '{}', got:\n{}",
        compound_scoped,
        css
    );

    // 7. Validate render function is valid JS
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", template.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Render function should be valid JS:\n{}\nErrors: {:?}",
        template.code,
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );

    // 8. Verify template imports contain all referenced helpers.
    // Extract helpers used in the render function (identifiers starting with _)
    let re_helpers: Vec<&str> = template
        .code
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|tok| tok.starts_with('_') && tok.len() > 1)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .filter(|h| {
            // Only check Vue runtime helpers (e.g. _createElementVNode, _openBlock)
            // Skip _ctx, _cache, locally declared _hoisted_N, _component_X
            !["_ctx", "_cache"].contains(h)
                && !h.starts_with("_hoisted_")
                && !h.starts_with("_component_")
        })
        .collect();
    for helper in &re_helpers {
        // Each _helper should map to a Vue export (strip leading _)
        let import_name = helper.strip_prefix('_').unwrap_or(helper);
        assert!(
            template
                .imports
                .iter()
                .any(|imp| &**imp == *helper || imp.ends_with(import_name)),
            "Template uses '{}' but it's not in imports {:?}",
            helper,
            template.imports
        );
    }

    // Debug: print imports only (CSS/script verified above)
    eprintln!("=== TEMPLATE IMPORTS ===\n{:?}", template.imports);
}

/// @ai-generated - Full template-heavy.vue integration test: compile the exact
/// fixture content and verify every CSS selector is properly scoped.
#[test]
pub(super) fn template_heavy_vue_full_css_scoping() {
    let source = include_str!("../../../../packages/benchmark/src/fixtures/template-heavy.vue");
    let result = compile_sfc(source);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );

    // Must have exactly one scoped style block
    assert_eq!(result.styles.len(), 1, "expected 1 style block");
    let css = result.styles[0].code();

    // Extract scope ID from script
    let script = result
        .script
        .as_ref()
        .expect("should have synthetic script block");
    let scope_marker = "__scopeId = \"";
    let scope_pos = script
        .code
        .find(scope_marker)
        .expect("scope marker in script");
    let scope_value_start = scope_pos + scope_marker.len();
    let scope_value_end = script.code[scope_value_start..]
        .find('"')
        .expect("closing quote for scope ID")
        + scope_value_start;
    let script_scope_id = &script.code[scope_value_start..scope_value_end];
    let css_scope_attr = format!("[{}]", script_scope_id);

    // Print full CSS for inspection
    eprintln!("=== FULL SCOPED CSS OUTPUT ===\n{}", css);
    eprintln!("=== SCOPE ATTR: {} ===", css_scope_attr);

    // Every original class selector must be scoped in the output.
    // For descendant selectors (e.g., .activity-table th), only the last
    // compound selector gets the scope attribute.
    let expected_scoped_selectors = [
        // Layout
        ".dashboard",
        ".header",
        ".sidebar",
        ".content",
        ".footer",
        // Stats
        ".stats-grid",
        ".stat-card",
        // Charts
        ".charts",
        ".chart-container",
        ".chart-placeholder",
        ".bar",
        // Activity
        ".recent-activity",
        ".activity-table",
        // Descendant selectors — last compound gets scope
        "th", // from ".activity-table th"
        "td", // from ".activity-table td"
        // Badges (simple)
        ".badge",
        // Widgets
        ".widgets",
        ".widget",
        ".indicator",
    ];
    for sel in expected_scoped_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // Compound selectors — the scope attribute goes at the END of the compound
    let expected_compound_selectors = [
        ".badge.success",
        ".badge.danger",
        ".badge.warning",
        ".badge.info",
        ".indicator.online",
        ".indicator.warning",
    ];
    for sel in expected_compound_selectors {
        let scoped_sel = format!("{}{}", sel, css_scope_attr);
        assert!(
            css.contains(&scoped_sel),
            "CSS should contain scoped compound selector '{}', got:\n{}",
            scoped_sel,
            css
        );
    }

    // Key CSS properties must be preserved (not dropped or collapsed)
    let preserved_properties = [
        "grid-template-areas",
        "grid-template-columns",
        "grid-template-rows",
        "min-height",
        "grid-area",
        "border-collapse",
        "box-shadow",
        "border-radius",
    ];
    for prop in preserved_properties {
        assert!(
            css.contains(prop),
            "CSS must preserve property '{}', got:\n{}",
            prop,
            css
        );
    }

    // Scope attribute count: every rule must have at least one scoped selector.
    // Count number of `{` that are NOT inside @-rules, and count scope attributes.
    let scope_count = css.matches(&css_scope_attr).count();
    assert!(
        scope_count >= 20, // template-heavy.vue has ~25 selectors
        "Expected at least 20 scoped selectors, found {} in:\n{}",
        scope_count,
        css
    );

    // Validate template render function is valid JS
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    eprintln!("=== TEMPLATE CODE ===\n{}", template.code);

    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", template.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Template render function should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        template.code
    );

    // Validate script is valid JS
    eprintln!("=== SCRIPT CODE ===\n{}", script.code);
    let alloc2 = Allocator::new();
    let parsed2 = verter_parser::oxc_parse::Parser::new(&alloc2, &script.code, source_type).parse();
    assert!(
        parsed2.diagnostics.is_empty(),
        "Script should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed2
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        script.code
    );

    // Simulate the playground's mergeRenderIntoComponent:
    // script + "\n" + template (with import prepended by host)
    let assembled = format!("{}\n{}", script.code, {
        if template.imports.is_empty() {
            template.code.clone()
        } else {
            let specifiers: Vec<String> = template
                .imports
                .iter()
                .map(|name| {
                    if let Some(stripped) = name.strip_prefix('_') {
                        format!("{stripped} as {name}")
                    } else {
                        name.to_string()
                    }
                })
                .collect();
            format!(
                "import {{ {} }} from \"vue\"\n{}",
                specifiers.join(", "),
                template.code
            )
        }
    });
    eprintln!("=== ASSEMBLED CODE (script + template) ===\n{}", assembled);

    // Verify the assembled code is valid JS
    let alloc3 = Allocator::new();
    let parsed3 = verter_parser::oxc_parse::Parser::new(&alloc3, &assembled, source_type).parse();
    assert!(
        parsed3.diagnostics.is_empty(),
        "Assembled code should be valid JS:\nErrors: {:?}\nCode:\n{}",
        parsed3
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        assembled
    );
}

#[test]
fn tsx_template_ref_vfor_scope_component_ref() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const compRef = useTemplateRef('compRef')
const components = [() => {}]
</script>
<template>
  <div v-for="Comp in components">
    <Comp ref="compRef" />
  </div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: should reconstruct the v-for iterable element type
    assert!(
        tsx.code.contains("(typeof components)[number]"),
        "v-for component should use iterable element type: {}",
        tsx.code
    );
}

#[test]
fn tsx_block_scope_no_double_braces() {
    // Double braces `}}` / `{{` in push_str should not appear —
    // they are NOT format-escaping, they emit literal characters.
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Should NOT contain `}}` followed by " // close" (old double-brace bug)
    assert!(
        !tsx.code.contains("}} // close"),
        "Double braces `}}` must not appear in block scope / templateBindingFN close.\nTSX:\n{}",
        tsx.code
    );

    // Should contain single-brace closes
    assert!(
        tsx.code.contains("} // close block scope"),
        "Block scope should close with single brace.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("} // close templateBindingFN"),
        "TemplateBindingFN should close with single brace.\nTSX:\n{}",
        tsx.code
    );
}

/// @ai-generated — TSX source map: SFC with style block after template (exposes bleeding bug)
///
/// This is the key regression test for Bug 1. The template CodeTransform operates
/// on the full SFC input, so its source map contains tokens for ALL regions including
/// the style block after `</template>`. If combine_tsx_source_maps doesn't filter
/// post-template tokens, style block tokens bleed into the combined map with incorrect
/// generated line positions.
#[test]
fn tsx_sourcemap_style_after_template_no_bleeding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div class="app">{{ msg }}</div>
</template>

<style scoped>
.app {
  color: red;
  font-size: 16px;
}
</style>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Basic bounds check
    verify_sourcemap_tokens_in_bounds(source, tsx);

    // TSX output should NOT contain any style content
    assert!(
        !tsx.code.contains("color: red"),
        "Style content should not appear in TSX output:\n{}",
        tsx.code
    );

    // Critical check: no source map token should reference a source position within
    // the style block (lines 8-13 in the Vue SFC). If style tokens bleed through
    // combine_tsx_source_maps, they'll appear as tokens with src_line in the style range.
    let style_start_line = source[..source.find("<style").unwrap()]
        .matches('\n')
        .count() as u32;
    let style_end_line = source[..source.find("</style>").unwrap() + "</style>".len()]
        .matches('\n')
        .count() as u32;

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");

    let mut style_tokens = Vec::new();
    for token in sm.get_tokens() {
        if token.get_source_id().is_none() {
            continue;
        }
        let src_line = token.get_src_line();
        if src_line >= style_start_line && src_line <= style_end_line {
            style_tokens.push((
                token.get_src_line(),
                token.get_src_col(),
                token.get_dst_line(),
                token.get_dst_col(),
            ));
        }
    }

    assert!(
        style_tokens.is_empty(),
        "Source map contains {} tokens referencing style block (Vue lines {}-{}): {:?}\n\
         These tokens bleed from the template CodeTransform's source map.\nTSX code:\n{}",
        style_tokens.len(),
        style_start_line,
        style_end_line,
        style_tokens,
        tsx.code
    );
}

/// @ai-generated — TSX source map: SFC with multiple style blocks
#[test]
fn tsx_sourcemap_multiple_style_blocks() {
    let source = r#"<script setup>
const x = 1
</script>

<template>
  <div>{{ x }}</div>
</template>

<style>
.a { color: red; }
</style>

<style scoped>
.b { color: blue; }
</style>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);

    // No token should reference source positions in the style blocks
    let first_style_line = source[..source.find("<style").unwrap()]
        .matches('\n')
        .count() as u32;
    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    for token in sm.get_tokens() {
        if token.get_source_id().is_some() {
            assert!(
                token.get_src_line() < first_style_line,
                "Source map token references style block (src line {}, first style line {}).\n\
                 Style tokens must not bleed into the combined TSX source map.",
                token.get_src_line(),
                first_style_line,
            );
        }
    }
}

/// @ai-generated — Scoped style injects data-v attribute on cached static elements
#[test]
fn static_hoist_scoped_style() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><p class="foo">text</p></div></template>
<style scoped>.foo { color: red; }</style>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping with scoped style\n--- code ---\n{}",
        code
    );
}

#[test]
fn tsx_root_element_skips_class_and_style() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div class="foo" style="color: red" id="bar">content</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: id is captured
    assert!(
        tsx.code.contains(r#""id": "bar""#),
        "should have id in props, got:\n{}",
        tsx.code
    );
    // Negative: class and style are excluded
    let comp_fn = tsx
        .code
        .split("function ___VERTER___Comp")
        .nth(1)
        .unwrap_or("");
    assert!(
        !comp_fn.contains(r#""class""#),
        "class should be excluded from Comp props, got:\n{}",
        comp_fn
    );
    assert!(
        !comp_fn.contains(r#""style""#),
        "style should be excluded from Comp props, got:\n{}",
        comp_fn
    );
}

#[test]
fn tsx_global_component_fallbacks_before_block_scope() {
    // Global components (not imported) should be declared BEFORE the block scope
    // so template JSX inside the block scope can reference them without TDZ errors.
    let result = compile_tsx(
        r#"<script setup lang="ts"></script>
<template>
  <RouterLink to="/home">Home</RouterLink>
  <RouterView />
</template>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    // RouterLink and RouterView fallback consts must appear before block scope
    let block_scope_pos = tsx
        .code
        .find("/* verter-destructured-start */")
        .or_else(|| tsx.code.find("\n{\n"))
        .unwrap_or_else(|| panic!("should have block scope: {}", tsx.code));
    let router_link_pos = tsx
        .code
        .find("const RouterLink =")
        .unwrap_or_else(|| panic!("should have RouterLink fallback: {}", tsx.code));
    let router_view_pos = tsx
        .code
        .find("const RouterView =")
        .unwrap_or_else(|| panic!("should have RouterView fallback: {}", tsx.code));
    assert!(
        router_link_pos < block_scope_pos,
        "RouterLink fallback must be before block scope (TDZ): {}",
        tsx.code
    );
    assert!(
        router_view_pos < block_scope_pos,
        "RouterView fallback must be before block scope (TDZ): {}",
        tsx.code
    );
    // Must NOT be after the block scope close
    let close_block = tsx
        .code
        .find("} // close block scope")
        .expect("should have close block scope");
    assert!(
        router_link_pos < close_block,
        "RouterLink fallback must not be after block scope: {}",
        tsx.code
    );
}

#[test]
fn bind_shorthand_style_uses_normalize_style() {
    let code = compile_and_validate_template(
        r#"<template><div :style>content</div></template>
<script setup>const style = { color: 'red' };</script>"#,
    );
    assert!(
        code.contains("_normalizeStyle("),
        ":style shorthand should use _normalizeStyle, got:\n{}",
        code
    );
}

/// Regression: Popover.vue with `attrs="{ class: string, style: string }"`
/// and `generic="T extends object"` on `<script setup>` must not produce
/// duplicate class/style attributes in TSX output (ts(17001)).
#[test]
fn ide_no_duplicate_class_style_with_script_attrs_and_generic() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Popover.vue".to_string()),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { ref } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
const floatingStyles = ref({})
const showArrow = ref(false)
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
</template>"#;
    let result = compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tsx = result.tsx.as_ref().expect("TSX output");
    let code = &tsx.code;

    eprintln!("=== FULL IDE OUTPUT ===\n{}\n=== END ===", code);

    // Count class= and style= occurrences in the template portion
    // (skip type construct declarations which may mention class/style)
    let template_start = code.find("<>").unwrap_or(0);
    let template_portion = &code[template_start..];

    let class_count = template_portion.matches("class=").count();
    assert!(
        class_count <= 1,
        "should have at most 1 class= attribute in template JSX, got {class_count}:\n{template_portion}"
    );

    let style_count = template_portion.matches("style=").count();
    assert!(
        style_count <= 1,
        "should have at most 1 style= attribute in template JSX, got {style_count}:\n{template_portion}"
    );

    // Verify no duplicate attribute names (TSX should parse cleanly)
    let alloc2 = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc2, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX parse errors (may indicate duplicate attrs): {:?}\n--- code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        code
    );
}

#[test]
fn combined_target_keeps_ide_completion_overlay_separate_for_scoped_prefix() {
    reset_parse_template_expressions_calls();

    let combined = compile_with_target(
        TS_OVERLAY_SCOPED_COMPLETION_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    assert!(
        combined.errors.is_empty(),
        "compile errors: {:?}",
        combined.errors
    );
    assert_eq!(
        parse_template_expressions_call_count(),
        2,
        "combined TS target must parse once for runtime(false) and once for TSX(true)"
    );

    let runtime_only = compile_with_target(
        TS_OVERLAY_SCOPED_COMPLETION_SFC,
        CompileTarget::BUNDLER,
        false,
    );
    let tsx_only = compile_with_target(TS_OVERLAY_SCOPED_COMPLETION_SFC, CompileTarget::IDE, false);

    let combined_runtime = &combined.template.as_ref().expect("combined runtime").code;
    let combined_tsx = &combined.tsx.as_ref().expect("combined tsx").code;

    assert_eq!(
        combined_runtime,
        &runtime_only.template.as_ref().expect("runtime-only").code,
        "combined runtime must stay byte-identical to runtime-only false parse"
    );
    assert_eq!(
        combined_tsx,
        &tsx_only.tsx.as_ref().expect("tsx-only").code,
        "combined TSX must stay byte-identical to IDE-only true parse"
    );

    assert!(
        combined_runtime.contains("_ctx.it"),
        "runtime false parse must keep partial scoped-prefix identifier as a real instance reference"
    );
    assert!(
        combined_tsx.contains("{ it }") || combined_tsx.contains("{it}"),
        "TSX true parse must keep partial scoped-prefix identifier bare for completion"
    );
    assert!(
        !combined_tsx.contains("___VERTER___instance.it"),
        "TSX true parse must not use the runtime false binding facts"
    );
}

#[test]
fn define_props_destructure_default_function_scope_local_stays_valid() {
    // A local declared INSIDE a default factory function body is function-scope,
    // not a setup-local — valid, exactly as for the runtime-object default path.
    let result = compile_sfc(
        "<script setup>\nconst { x = () => { const a = 1; return a } } = defineProps({ x: Number })\n</script>\n<template><div>{{ x }}</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineProps destructure default factory with only a function-scope local must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn missing_sfc_entry_block_for_empty_style_or_custom_block_only_carriers() {
    for src in [
        "<style/>",
        "<style> \n\t </style>",
        "<i18n/>",
        "<i18n> \n\t </i18n>",
    ] {
        let result = compile_sfc(src);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == "MissingSfcEntryBlock"),
            "{src:?} must diagnose MissingSfcEntryBlock (oracle: parse.ts \
             tracks the block-less carrier regardless of the pruned block's \
             own emptiness), got: {:?}",
            result.errors
        );
    }
}
