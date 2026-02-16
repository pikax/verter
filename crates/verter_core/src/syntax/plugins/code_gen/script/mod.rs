use std::{cell::RefCell, rc::Rc};

use rustc_hash::FxHashMap;

use crate::{
    code_transform::{CodeTransform, SourceMapOptions},
    syntax::{
        binding_types::BindingType,
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        plugins::code_gen::{
            script::process::{process_script_event, ProcessScriptOptions},
            types::ScriptSetupImportDependencies,
        },
        types::{CssParsedStyleBlock, Event, OxcScript},
    },
};

pub mod macros;
pub mod process;
pub mod sections;

pub struct ScriptGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    component_name: &'alloc str,

    keep_ts_types: bool,
    is_production: bool,
    inline_template: bool,
    runtime_module_name: String,

    imports: ScriptSetupImportDependencies,

    /// Scope ID for CSS variable name generation (matches CSS side).
    scope_id: [u8; 8],
    /// Collected CSS v-bind expressions: (var_key, expression_text).
    /// var_key is the CSS variable key without `--` prefix (e.g., "a4f2eed6-color").
    css_v_binds: Vec<(String, String)>,
    /// Saved insertion position from OxcScript (tag_open_end).
    /// Used to inject useCssVars call inside setup().
    script_insert_pos: Option<u32>,

    /// Track whether we've already seen a `<script setup>` block.
    has_seen_script_setup: bool,
    /// Track whether we've already seen a plain `<script>` block.
    has_seen_script: bool,
    /// Binding metadata from OxcScript, used for correct accessor in _useCssVars.
    bindings: FxHashMap<String, BindingType>,
    /// Deferred closing text for inline template mode.
    /// When the template is inlined inside setup(), the script closing only emits "\n"
    /// (leaving setup open). This string closes setup() and the component definition
    /// AFTER the template content. Emitted in `end()`.
    deferred_inline_closing: Option<String>,
    /// Whether the template uses vapor mode (`<template vapor>`).
    /// Detected from `CompiledTemplateStart` event, emitted as `__vapor: true` in `end()`.
    is_vapor: bool,
    /// Whether any `<style scoped>` block was seen. Used to emit `__sfc__.__scopeId`.
    has_scoped: bool,
    /// Whether the script has a default export (from `<script setup>` or `export default` in
    /// regular `<script>`). When true, `end()` appends `export default __sfc__`.
    has_default_export: bool,
}

impl<'alloc> ScriptGeneratorPlugin<'alloc> {
    pub fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        component_name: &'alloc str,
        keep_ts_types: bool,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            component_name,
            keep_ts_types,
            is_production,
            inline_template: is_production,
            runtime_module_name: "vue".to_string(),

            imports: ScriptSetupImportDependencies::default(),
            scope_id: [b'0'; 8],
            css_v_binds: Vec::new(),
            script_insert_pos: None,
            has_seen_script_setup: false,
            has_seen_script: false,
            bindings: FxHashMap::default(),
            deferred_inline_closing: None,
            is_vapor: false,
            has_scoped: false,
            has_default_export: false,
        }
    }

    /// Set the scope ID for CSS variable name generation.
    pub fn with_scope_id(mut self, scope_id: [u8; 8]) -> Self {
        self.scope_id = scope_id;
        self
    }

    /// Set inline template mode (decoupled from is_production).
    pub fn with_inline_template(mut self, inline: bool) -> Self {
        self.inline_template = inline;
        self
    }

    /// Set vapor mode (template uses `<template vapor>`).
    pub fn with_vapor(mut self, vapor: bool) -> Self {
        self.is_vapor = vapor;
        self
    }

    /// Set the runtime module name for helper imports.
    pub fn with_runtime_module_name(mut self, name: String) -> Self {
        self.runtime_module_name = name;
        self
    }

    /// Get the transformed code (script block only).
    pub fn get_code(&self) -> String {
        self.code_transform.borrow().build_string()
    }

    /// Generate source map JSON string.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.borrow().generate_map_json(options)
    }

    /// Whether this plugin used the `const __sfc__ = ...` + `export default __sfc__` pattern.
    /// When true, the builder should skip its own scoped-style wrapping.
    pub fn has_sfc_wrapper(&self) -> bool {
        self.has_default_export
    }

    fn process_script(&mut self, event: &OxcScript<'alloc>, ctx: &mut SyntaxPluginContext<'alloc>) {
        use crate::syntax::plugin::CompilerErrorCode;

        let is_setup = event.setup.is_some();

        // Detect duplicate script blocks
        if is_setup {
            if self.has_seen_script_setup {
                ctx.error_at(
                    "ScriptGeneratorPlugin",
                    CompilerErrorCode::DuplicateScriptSetup,
                    crate::common::Span {
                        start: event.start,
                        end: event.tag_open_end,
                    },
                );
                return;
            }
            self.has_seen_script_setup = true;
        } else {
            if self.has_seen_script {
                ctx.error_at(
                    "ScriptGeneratorPlugin",
                    CompilerErrorCode::DuplicateScript,
                    crate::common::Span {
                        start: event.start,
                        end: event.tag_open_end,
                    },
                );
                return;
            }
            self.has_seen_script = true;
        }

        // Save the insertion position for useCssVars injection in end()
        if is_setup {
            self.script_insert_pos = Some(event.tag_open_end);
            // <script setup> always produces a default export (via const __sfc__ in process.rs)
            self.has_default_export = true;
        } else {
            // For regular <script>, check if it has `export default` via the AST.
            // Replace it with `const __sfc__ = ` using the known span, so the plugin
            // controls the export (emitted in end()).
            use crate::utils::oxc::vue::ScriptItem;
            for item in &event.result.items {
                if let ScriptItem::DefaultExport(de) = item {
                    self.has_default_export = true;
                    // Use AST span to precisely overwrite "export default " with "const __sfc__ = "
                    // The span covers the full statement; we only need to replace the
                    // `export default` keyword prefix (the declaration follows).
                    let keyword_end = de.span.start + "export default ".len() as u32;
                    self.code_transform.borrow_mut().overwrite(
                        de.span.start,
                        keyword_end,
                        "const __sfc__ = ",
                    );
                    break;
                }
            }
        }

        // Process the script content with macros and transformations.
        let processed = process_script_event(
            event,
            &mut self.code_transform.borrow_mut(),
            ProcessScriptOptions {
                source: ctx.input,
                component_name: self.component_name,
                keep_ts_types: self.keep_ts_types,
                is_production: self.is_production,
                inline_template: self.inline_template,
                is_vapor: self.is_vapor,
            },
        );

        // Emit macro diagnostics (e.g., "Unresolvable type reference")
        for diag in processed.diagnostics {
            ctx.error_at_with_message(
                "ScriptGeneratorPlugin",
                CompilerErrorCode::XInvalidExpression,
                diag.message,
                diag.span,
            );
        }

        self.imports.add(processed.imports.0);

        // Store deferred closing for inline template mode
        if processed.deferred_closing.is_some() {
            self.deferred_inline_closing = processed.deferred_closing;
        }
    }

    fn collect_css_v_binds(
        &mut self,
        parsed: &CssParsedStyleBlock,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        if parsed.v_binds.is_empty() {
            return;
        }

        let scope_id_str = std::str::from_utf8(&self.scope_id).unwrap_or("00000000");

        for vbind in &parsed.v_binds {
            let expr_text =
                &ctx.input[vbind.expression.start as usize..vbind.expression.end as usize];

            // Generate the CSS variable name (e.g., "--a4f2eed6-color")
            let var_name = crate::css::prepass::generate_var_name(scope_id_str, expr_text);
            // Strip leading "--" to get the JS key (e.g., "a4f2eed6-color")
            let var_key = var_name[2..].to_string();

            self.css_v_binds.push((var_key, expr_text.to_string()));
        }
    }

    fn inject_use_css_vars(&mut self) {
        if self.css_v_binds.is_empty() {
            return;
        }

        let Some(insert_pos) = self.script_insert_pos else {
            return;
        };

        self.imports
            .add(ScriptSetupImportDependencies::USE_CSS_VARS);

        let mut needs_unref = false;

        let mut buf = String::with_capacity(64 + self.css_v_binds.len() * 48);
        buf.push_str("\n_useCssVars(_ctx => ({\n");
        for (i, (key, expr)) in self.css_v_binds.iter().enumerate() {
            buf.push_str("  \"");
            buf.push_str(key);
            buf.push_str("\": (");
            // Inside setup(), bindings are in scope directly. Use binding metadata
            // to determine the correct accessor, matching the official Vue compiler.
            if let Some(bt) = self.bindings.get(expr.as_str()) {
                match bt {
                    BindingType::SetupRef => {
                        // Definitively a ref: access .value directly
                        buf.push_str(expr);
                        buf.push_str(".value");
                    }
                    BindingType::SetupMaybeRef | BindingType::SetupLet => {
                        // Might be a ref: wrap with _unref()
                        needs_unref = true;
                        buf.push_str("_unref(");
                        buf.push_str(expr);
                        buf.push(')');
                    }
                    BindingType::Props | BindingType::PropsAliased => {
                        buf.push_str("__props.");
                        buf.push_str(expr);
                    }
                    _ => {
                        // SetupConst, SetupReactiveConst, LiteralConst: direct access
                        buf.push_str(expr);
                    }
                }
            } else {
                // Unknown binding: use _ctx. prefix as fallback
                buf.push_str("_ctx.");
                buf.push_str(expr);
            }
            buf.push(')');
            if i < self.css_v_binds.len() - 1 {
                buf.push(',');
            }
            buf.push('\n');
        }
        buf.push_str("}))\n");

        if needs_unref {
            self.imports.add(ScriptSetupImportDependencies::UNREF);
        }

        self.code_transform
            .borrow_mut()
            .prepend_left(insert_pos, &buf);
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for ScriptGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "ScriptGeneratorPlugin"
    }

    fn end(&mut self, _ctx: &SyntaxPluginContext<'alloc>) {
        // Inject useCssVars call inside setup() if v-bind expressions were found
        self.inject_use_css_vars();

        // Emit deferred inline closing at the very end of the output.
        // When inline_template is true, the script closing only emits "\n" (leaving
        // setup open for the template). The template close emits "}" for the arrow
        // function. This deferred closing adds "\n}}" or "\n}})" to close setup()
        // and the component definition.
        if let Some(ref closing) = self.deferred_inline_closing {
            self.code_transform.borrow_mut().append(closing);
        }

        // Template-only components (no <script> block at all): emit a minimal
        // component definition so bundlers get `export default __sfc__`.
        if !self.has_default_export && !self.has_seen_script && !self.has_seen_script_setup {
            self.code_transform
                .borrow_mut()
                .append("\nconst __sfc__ = {};");
            self.has_default_export = true;
        }

        // Emit __sfc__.__scopeId and export default __sfc__ at the end.
        // This is done here (not in codegen.rs) because the plugin has AST-level
        // knowledge of where `export default` was placed, avoiding fragile string matching.
        if self.has_default_export {
            let mut ct = self.code_transform.borrow_mut();
            if self.has_scoped {
                let hex = std::str::from_utf8(&self.scope_id).unwrap_or("00000000");
                ct.append(&format!("\n__sfc__.__scopeId = \"data-v-{}\";", hex));
            }
            ct.append("\nexport default __sfc__;\n");
        }

        // Add imports to the top of the script
        if !self.imports.is_empty() {
            self.code_transform.borrow_mut().prepend(
                format!(
                    "import {{{}}} from '{}';\n",
                    self.imports.to_import_string(),
                    self.runtime_module_name,
                )
                .as_str(),
            );
        }
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match &event {
            Event::OxcScript(script) => {
                // Store binding metadata for _useCssVars accessor generation
                for (span, binding) in &script.result.bindings {
                    let name = &ctx.input[span.start as usize..span.end as usize];
                    self.bindings.insert(name.to_string(), *binding);
                }
                self.process_script(script, ctx);
            }
            Event::CssParsedStyle(parsed) => {
                self.has_scoped |= parsed.scoped;
                self.collect_css_v_binds(parsed, ctx);
            }
            _ => {}
        }
        SyntaxResult::Keep(event)
    }
}

#[cfg(test)]
mod tests {
    use crate::builder::codegen::{compile, CodegenOptions};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // =========================================================================
    // Test Infrastructure
    // =========================================================================

    fn gen(input: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        compile(input, &options, &allocator).code
    }

    fn gen_prod(input: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new()
            .with_filename("test.vue")
            .with_production(true);
        compile(input, &options, &allocator).code
    }

    fn gen_with_filename(input: &str, filename: &str) -> String {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename(filename);
        compile(input, &options, &allocator).code
    }

    fn gen_result(input: &str) -> crate::builder::codegen::CodegenResult {
        let allocator = Allocator::new();
        let options = CodegenOptions::new().with_filename("test.vue");
        compile(input, &options, &allocator)
    }

    // =========================================================================
    // Multi-script: <script setup> + <script> is valid Vue
    // =========================================================================

    #[test]
    fn test_multi_script_does_not_panic() {
        // Vue supports <script setup> + <script> in the same SFC.
        // This must not panic — it should either compile or emit diagnostics.
        let input = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default { name: 'MyComponent' }
</script>
<template><div>{{ count }}</div></template>"#;

        let result = gen_result(input);
        // Should not panic, and should produce some output or errors
        assert!(
            !result.code.is_empty() || !result.errors.is_empty(),
            "Multi-script SFC should compile or report errors, not panic"
        );
    }

    #[test]
    fn test_duplicate_script_setup_reports_error() {
        // Two <script setup> blocks is invalid — should report an error, not panic.
        let input = r#"<script setup>
const a = 1
</script>
<script setup>
const b = 2
</script>
<template><div>test</div></template>"#;

        let result = gen_result(input);
        assert!(
            !result.errors.is_empty(),
            "Duplicate <script setup> should produce error diagnostics, got none"
        );
    }

    fn assert_valid_js(code: &str, context: &str) {
        let allocator = Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&allocator, code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\n\
             Context: {}\n\
             Parse Errors: {:?}\n\
             Generated Code:\n{}",
            context,
            parser_result.errors,
            code
        );
    }

    fn gen_and_validate(input: &str) -> String {
        let code = gen(input);
        assert_valid_js(&code, input);
        code
    }

    fn gen_prod_and_validate(input: &str) -> String {
        let code = gen_prod(input);
        // Production output should be valid JS at module level (export default ...)
        assert_valid_js(&code, input);
        code
    }

    // =========================================================================
    // Basic Script Wrapping
    // =========================================================================

    /// @ai-generated — Dev mode emits setup with full signature and __returned__
    #[test]
    fn test_script_setup_basic_dev() {
        let code = gen_and_validate(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>",
        );
        assert!(
            code.contains("const __sfc__ = /*@__PURE__*/"),
            "Should have const __sfc__ = /*@__PURE__*/, got:\n{}",
            code
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should export __sfc__ at the end, got:\n{}",
            code
        );
        assert!(
            code.contains("__name: 'test'"),
            "Should have component name from filename, got:\n{}",
            code
        );
        assert!(
            code.contains("setup(__props,{expose:__expose})"),
            "Dev mode should have full setup signature, got:\n{}",
            code
        );
        assert!(
            code.contains("__expose()"),
            "Dev mode should auto-call __expose(), got:\n{}",
            code
        );
        assert!(
            code.contains("__returned__"),
            "Dev mode should use __returned__, got:\n{}",
            code
        );
        assert!(
            code.contains("__isScriptSetup"),
            "Dev mode should set __isScriptSetup, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode emits production template but script remains dev
    /// NOTE: compile currently hardcodes is_production=false for ScriptGeneratorPlugin.
    /// The template correctly uses production mode. This test verifies current behavior.
    #[test]
    fn test_script_setup_prod_template() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>",
        );
        // Template uses production mode arrow function
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Template should use production arrow function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Component name extracted from filename
    #[test]
    fn test_component_name_from_filename() {
        let code = gen_with_filename(
            "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>",
            "MyComponent.vue",
        );
        assert_valid_js(&code, "component name");
        assert!(
            code.contains("__name: 'MyComponent'"),
            "Should extract component name from filename, got:\n{}",
            code
        );
    }

    /// @ai-generated — Regular <script> (no setup) strips tags and keeps content
    #[test]
    fn test_script_no_setup() {
        let code = gen_and_validate(
            "<script>\nexport default { name: 'Foo' }\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("<script>"),
            "Should strip <script> tag, got:\n{}",
            code
        );
        assert!(
            !code.contains("</script>"),
            "Should strip </script> tag, got:\n{}",
            code
        );
        assert!(
            code.contains("const __sfc__ = { name: 'Foo' }"),
            "Should replace export default with const __sfc__, got:\n{}",
            code
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should export __sfc__ at the end, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineProps
    // =========================================================================

    /// @ai-generated — defineProps with object arg moves props to component def
    #[test]
    fn test_define_props_object_arg() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>{{ props.title }}</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with inline type literal resolves to runtime props
    #[test]
    fn test_define_props_typed_inline() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("title:"),
            "Should have title prop, got:\n{}",
            code
        );
        assert!(
            code.contains("String"),
            "Should resolve string to String, got:\n{}",
            code
        );
        assert!(
            !code.contains("defineProps<"),
            "Should NOT leave defineProps as-is, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with optional typed prop
    #[test]
    fn test_define_props_typed_optional() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ count?: number }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("required: false"),
            "Optional prop should be required: false, got:\n{}",
            code
        );
        assert!(
            code.contains("Number"),
            "Should resolve number to Number, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with string literal union type
    #[test]
    fn test_define_props_string_literal_union() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ view?: 'list' | 'board' | 'calendar' }>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("String"),
            "String literal union should resolve to String, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps inline type: no stale delimiters in output
    /// Verifies that < and > from the type parameter are not duplicated in the output.
    #[test]
    fn test_define_props_inline_no_stale_delimiters() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>x</div></template>",
        );
        // The output should not contain leftover < or > from the type parameter.
        // Look for patterns like ">{" or "}<" or "><" that would indicate duplicated delimiters.
        // It's fine for < and > to appear in the template render function, but the props section
        // should not have stale angle brackets.
        assert!(
            !code.contains("defineProps<"),
            "Should not leave defineProps< in output, got:\n{}",
            code
        );
        assert!(
            !code.contains(">()"),
            "Should not leave >() in output, got:\n{}",
            code
        );
        // The props section should be clean
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with SFC-local interface reference
    #[test]
    fn test_define_props_interface_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ninterface Props { title: string; count?: number }\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("title:"),
            "Should resolve interface prop 'title', got:\n{}",
            code
        );
        assert!(
            code.contains("count:"),
            "Should resolve interface prop 'count', got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with SFC-local type alias reference
    #[test]
    fn test_define_props_type_alias_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ntype MyProps = { message: string }\ndefineProps<MyProps>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("message:"),
            "Should resolve type alias prop 'message', got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with unresolvable imported type defaults to empty props
    #[test]
    fn test_define_props_unresolvable_type() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("defineProps<"),
            "Should NOT leave defineProps as-is, got:\n{}",
            code
        );
        assert!(
            code.contains("__props"),
            "Should replace with __props, got:\n{}",
            code
        );
        // Unresolvable types should still emit a props section with empty object
        // so that the component definition includes `props: {}`
        assert!(
            code.contains("props:"),
            "Should emit props section for unresolvable types, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with empty type literal should emit props:{}
    #[test]
    fn test_define_props_empty_type_literal() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ndefineProps<{}>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("__props"),
            "Should replace with __props, got:\n{}",
            code
        );
        // Empty type literal should still emit a props section
        assert!(
            code.contains("props:"),
            "Should emit props section for empty type literal, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineProps with unresolvable type should emit diagnostic error
    #[test]
    fn test_define_props_unresolvable_type_diagnostic() {
        let result = gen_result(
            "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            result.errors.iter().any(|e| e.message.contains("Unresolvable type reference")),
            "Should emit 'Unresolvable type reference' diagnostic for imported types, got errors: {:?}",
            result.errors
        );
    }

    /// @ai-generated — defineProps replaces call site with __props
    #[test]
    fn test_define_props_replaced_with_props() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("__props"),
            "defineProps should be replaced with __props reference, got:\n{}",
            code
        );
    }

    // =========================================================================
    // withDefaults
    // =========================================================================

    /// @ai-generated — withDefaults with inline typed defineProps
    #[test]
    fn test_with_defaults_typed_inline() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nconst props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("default:"),
            "Should have default value, got:\n{}",
            code
        );
        assert!(
            !code.contains("withDefaults("),
            "Should NOT leave withDefaults as-is, got:\n{}",
            code
        );
    }

    /// @ai-generated — withDefaults with SFC-local interface reference
    #[test]
    fn test_with_defaults_interface_ref() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\ninterface Props { foo?: string; bar?: number }\nconst props = withDefaults(defineProps<Props>(), { foo: 'hello' })\n</script>\n<template><div>x</div></template>",
        );
        assert!(code.contains("props:"), "Should have props, got:\n{}", code);
        assert!(
            code.contains("foo:"),
            "Should have foo prop, got:\n{}",
            code
        );
        assert!(
            code.contains("bar:"),
            "Should have bar prop, got:\n{}",
            code
        );
        assert!(
            code.contains("default:"),
            "Should have default for foo, got:\n{}",
            code
        );
    }

    // =========================================================================
    // export interface / export type with defineProps
    // =========================================================================

    /// @ai-generated — export interface should resolve like plain interface in defineProps
    #[test]
    fn test_define_props_export_interface_resolves() {
        // Verify generated code is valid JS and props are resolved
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nexport interface Props {\n  foo: string\n  bar?: number\n}\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.foo }}</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("foo:"),
            "Should have foo prop, got:\n{}",
            code
        );
        assert!(
            code.contains("bar:"),
            "Should have bar prop, got:\n{}",
            code
        );

        // Verify no "Unresolvable" error
        let result = gen_result(
            "<script setup lang=\"ts\">\nexport interface Props {\n  foo: string\n  bar?: number\n}\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.foo }}</div></template>",
        );
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.message.contains("Unresolvable type reference")),
            "Should NOT emit 'Unresolvable type reference' for locally exported interface, got errors: {:?}",
            result.errors
        );
    }

    /// @ai-generated — export type alias should resolve like plain type alias in defineProps
    #[test]
    fn test_define_props_export_type_alias_resolves() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nexport type Props = {\n  bar: number\n}\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("bar:"),
            "Should have bar prop, got:\n{}",
            code
        );

        let result = gen_result(
            "<script setup lang=\"ts\">\nexport type Props = {\n  bar: number\n}\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.message.contains("Unresolvable type reference")),
            "Should NOT emit 'Unresolvable type reference' for locally exported type alias, got errors: {:?}",
            result.errors
        );
    }

    /// @ai-generated — export interface with withDefaults should produce props with defaults
    #[test]
    fn test_with_defaults_export_interface() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nexport interface Props {\n  size?: number\n  color?: string\n}\nconst props = withDefaults(defineProps<Props>(), {\n  size: 16,\n  color: 'red',\n})\n</script>\n<template><div>{{ props.size }}</div></template>",
        );
        assert!(
            code.contains("props:"),
            "Should have props section, got:\n{}",
            code
        );
        assert!(
            code.contains("size:"),
            "Should have size prop, got:\n{}",
            code
        );
        assert!(
            code.contains("color:"),
            "Should have color prop, got:\n{}",
            code
        );
        assert!(
            code.contains("default:"),
            "Should have default values, got:\n{}",
            code
        );
        assert!(
            !code.contains("withDefaults("),
            "Should NOT leave withDefaults as-is, got:\n{}",
            code
        );

        let result = gen_result(
            "<script setup lang=\"ts\">\nexport interface Props {\n  size?: number\n  color?: string\n}\nconst props = withDefaults(defineProps<Props>(), {\n  size: 16,\n  color: 'red',\n})\n</script>\n<template><div>{{ props.size }}</div></template>",
        );
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.message.contains("Unresolvable type reference")),
            "Should NOT emit 'Unresolvable type reference' for exported interface in withDefaults, got errors: {:?}",
            result.errors
        );
    }

    // =========================================================================
    // defineEmits
    // =========================================================================

    /// @ai-generated — defineEmits with array arg
    #[test]
    fn test_define_emits_array() {
        let code = gen_and_validate(
            "<script setup>\nconst emit = defineEmits(['click', 'update'])\n</script>\n<template><div @click=\"emit('click')\">x</div></template>",
        );
        assert!(
            code.contains("emits:"),
            "Should have emits section, got:\n{}",
            code
        );
        assert!(
            code.contains("emit:__emit"),
            "Should have emit in setup signature, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineEmits without declarator doesn't add emit to signature
    #[test]
    fn test_define_emits_no_declarator() {
        let code = gen_and_validate(
            "<script setup>\ndefineEmits(['click'])\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("emits:"),
            "Should have emits section, got:\n{}",
            code
        );
        assert!(
            !code.contains("emit:__emit"),
            "No declarator means no emit in signature, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineModel
    // =========================================================================

    /// @ai-generated — defineModel produces _useModel call and mergeModels
    #[test]
    fn test_define_model_basic() {
        let code = gen_and_validate(
            "<script setup>\nconst model = defineModel()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_useModel"),
            "defineModel should produce _useModel call, got:\n{}",
            code
        );
        assert!(
            code.contains("modelValue"),
            "Default model name should be modelValue, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineModel with named model
    #[test]
    fn test_define_model_named() {
        let code = gen_and_validate(
            "<script setup>\nconst count = defineModel('count')\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("\"count\""),
            "Named model should use provided name, got:\n{}",
            code
        );
    }

    /// @ai-generated — defineModel with defineProps triggers mergeModels
    #[test]
    fn test_define_model_with_props_merge() {
        let code = gen_and_validate(
            "<script setup>\nconst props = defineProps({ title: String })\nconst model = defineModel()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_mergeModels"),
            "defineModel + defineProps should use _mergeModels, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineExpose
    // =========================================================================

    /// @ai-generated — defineExpose replaces with __expose and suppresses auto-expose
    #[test]
    fn test_define_expose() {
        let code = gen_and_validate(
            "<script setup>\nconst publicFn = () => {}\ndefineExpose({ publicFn })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("__expose("),
            "defineExpose should be replaced with __expose, got:\n{}",
            code
        );
        // When defineExpose is used, auto __expose() should NOT be called
        // (only the user's explicit expose should run)
        let expose_count = code.matches("__expose(").count();
        assert_eq!(
            expose_count, 1,
            "Should have exactly one __expose call (no auto-expose), got {} in:\n{}",
            expose_count, code
        );
    }

    // =========================================================================
    // defineOptions
    // =========================================================================

    /// @ai-generated — defineOptions moves object to component definition
    #[test]
    fn test_define_options() {
        let code = gen_and_validate(
            "<script setup>\ndefineOptions({ inheritAttrs: false })\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("inheritAttrs: false"),
            "defineOptions object should be in output, got:\n{}",
            code
        );
    }

    // =========================================================================
    // defineSlots
    // =========================================================================

    /// @ai-generated — defineSlots replaced with _useSlots()
    #[test]
    fn test_define_slots() {
        let code = gen_and_validate(
            "<script setup>\nconst slots = defineSlots()\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_useSlots"),
            "defineSlots should be replaced with _useSlots, got:\n{}",
            code
        );
    }

    // =========================================================================
    // TypeScript vs JavaScript Wrapping (inline)
    // NOTE: compile does not call plugin.end(), so script imports
    // (defineComponent, useSlots, mergeModels) are not emitted as import
    // statements. These tests verify the inline helper usage.
    // =========================================================================

    /// @ai-generated — TypeScript setup uses _defineComponent wrapper
    #[test]
    fn test_ts_uses_define_component() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("_defineComponent("),
            "TS script setup should use _defineComponent, got:\n{}",
            code
        );
    }

    /// @ai-generated — JavaScript setup does NOT use _defineComponent
    #[test]
    fn test_js_no_define_component() {
        let code = gen_and_validate(
            "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            !code.contains("_defineComponent("),
            "JS script setup should NOT use _defineComponent, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Script Items
    // =========================================================================

    /// @ai-generated — Regular import is moved before component definition
    #[test]
    fn test_import_hoisted() {
        let code = gen_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div>{{ count }}</div></template>",
        );
        // Import should appear before export default
        let import_pos = code.find("import { ref }");
        let export_pos = code.find("export default");
        assert!(
            import_pos.is_some() && export_pos.is_some(),
            "Should have both import and export, got:\n{}",
            code
        );
        assert!(
            import_pos.unwrap() < export_pos.unwrap(),
            "Import should appear before export default, got:\n{}",
            code
        );
    }

    /// @ai-generated — Type-only imports are stripped from output
    #[test]
    fn test_type_import_stripped() {
        let code = gen_and_validate(
            "<script setup lang=\"ts\">\nimport type { Ref } from 'vue'\nconst x = 1\n</script>\n<template><div>x</div></template>",
        );
        // Type-only imports should NOT appear in the runtime output
        assert!(
            !code.contains("import type"),
            "Type-only import should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — Declarations appear in return statement
    #[test]
    fn test_declarations_in_return() {
        let code = gen_and_validate(
            "<script setup>\nconst count = 0\nfunction increment() {}\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("count") && code.contains("increment"),
            "Both declarations should be in output, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    /// @ai-generated — Empty script setup produces valid output
    #[test]
    fn test_script_setup_empty() {
        let code = gen_and_validate("<script setup>\n</script>\n<template><div>x</div></template>");
        assert!(
            code.contains("setup("),
            "Empty setup should still produce setup function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Script setup with only a comment
    #[test]
    fn test_script_setup_comment_only() {
        let code = gen_and_validate(
            "<script setup>\n// just a comment\n</script>\n<template><div>x</div></template>",
        );
        assert!(
            code.contains("setup("),
            "Comment-only setup should still produce setup function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode includes return with bindings
    #[test]
    fn test_prod_return_bindings() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        assert!(
            code.contains("msg"),
            "Production mode should include bindings, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Import bindings in __returned__
    // =========================================================================

    /// @ai-generated — Imported components must appear in __returned__ so $setup["Comp"] resolves
    #[test]
    fn test_imported_component_in_returned() {
        let code = gen_and_validate(
            r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><MyComp/></template>"#,
        );
        // The __returned__ object must include MyComp so the template can access $setup["MyComp"]
        assert!(
            code.contains("__returned__") && code.contains("MyComp"),
            "Imported component should appear in __returned__, got:\n{}",
            code
        );
        // Specifically check it's in the __returned__ object
        let returned_pos = code
            .find("__returned__={")
            .expect("Should have __returned__");
        let returned_end = code[returned_pos..].find('}').unwrap() + returned_pos;
        let returned_section = &code[returned_pos..returned_end];
        assert!(
            returned_section.contains("MyComp"),
            "MyComp should be inside __returned__={{...}}, got section: {}",
            returned_section
        );
    }

    /// @ai-generated — Named imports (helpers, constants) must appear in __returned__
    #[test]
    fn test_named_import_in_returned() {
        let code = gen_and_validate(
            r#"<script setup>
import { SOME_CONST } from './constants'
const x = SOME_CONST
</script>
<template><div>{{ x }}</div></template>"#,
        );
        let returned_pos = code
            .find("__returned__={")
            .expect("Should have __returned__");
        let returned_end = code[returned_pos..].find('}').unwrap() + returned_pos;
        let returned_section = &code[returned_pos..returned_end];
        assert!(
            returned_section.contains("SOME_CONST"),
            "Named import should be inside __returned__, got section: {}",
            returned_section
        );
    }

    /// @ai-generated — Type-only imports must NOT appear in __returned__
    #[test]
    fn test_type_import_not_in_returned() {
        let code = gen_and_validate(
            r#"<script setup lang="ts">
import type { Ref } from 'vue'
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
        );
        // Type-only import should not appear anywhere in the output
        assert!(
            !code.contains("Ref"),
            "Type-only import should NOT appear in output, got:\n{}",
            code
        );
    }

    /// @ai-generated — Props should NOT appear in __returned__ (accessed via $props)
    #[test]
    fn test_props_not_in_returned() {
        let code = gen_and_validate(
            r#"<script setup lang="ts">
defineProps<{ store: any }>()
const localVar = 'hello'
</script>
<template><div>{{ localVar }}</div></template>"#,
        );
        let returned_pos = code
            .find("__returned__={")
            .expect("Should have __returned__");
        let returned_end = code[returned_pos..].find('}').unwrap() + returned_pos;
        let returned_section = &code[returned_pos..returned_end];
        assert!(
            returned_section.contains("localVar"),
            "Local var should be in __returned__, got section: {}",
            returned_section
        );
        // store prop should NOT be in __returned__ (it's accessed via $props.store)
        assert!(
            !returned_section.contains("store"),
            "Props should NOT be in __returned__, got section: {}",
            returned_section
        );
    }

    /// @ai-generated — Multiple imports and declarations all appear in __returned__
    #[test]
    fn test_mixed_imports_and_declarations_in_returned() {
        let code = gen_and_validate(
            r#"<script setup>
import Header from './Header.vue'
import { ref } from 'vue'
const count = ref(0)
function increment() {}
</script>
<template><Header/><div @click="increment">{{ count }}</div></template>"#,
        );
        let returned_pos = code
            .find("__returned__={")
            .expect("Should have __returned__");
        let returned_end = code[returned_pos..].find('}').unwrap() + returned_pos;
        let returned_section = &code[returned_pos..returned_end];
        assert!(
            returned_section.contains("Header"),
            "Imported component Header should be in __returned__, got: {}",
            returned_section
        );
        assert!(
            returned_section.contains("ref"),
            "Imported ref should be in __returned__, got: {}",
            returned_section
        );
        assert!(
            returned_section.contains("count"),
            "Declaration count should be in __returned__, got: {}",
            returned_section
        );
        assert!(
            returned_section.contains("increment"),
            "Declaration increment should be in __returned__, got: {}",
            returned_section
        );
    }

    /// @ai-generated — Per-specifier type imports must NOT appear in __returned__
    /// `import { CONST, type MyType } from '...'` — only CONST should be in __returned__
    #[test]
    fn test_per_specifier_type_import_not_in_returned() {
        // Use gen() not gen_and_validate(): output contains TS syntax (`type MyType`)
        // which is valid TS but invalid JS — esbuild strips it in the real pipeline.
        let code = gen(r#"<script setup lang="ts">
import { CONST_VAL, type MyType } from './types'
const x = CONST_VAL
</script>
<template><div>{{ x }}</div></template>"#);
        let returned_pos = code
            .find("__returned__={")
            .expect("Should have __returned__");
        let returned_end = code[returned_pos..].find('}').unwrap() + returned_pos;
        let returned_section = &code[returned_pos..returned_end];
        assert!(
            returned_section.contains("CONST_VAL"),
            "Value import CONST_VAL should be in __returned__, got section: {}",
            returned_section
        );
        assert!(
            !returned_section.contains("MyType"),
            "Per-specifier type import MyType should NOT be in __returned__, got section: {}",
            returned_section
        );
    }

    // =========================================================================
    // Production Inline Template
    // =========================================================================

    /// @ai-generated — Production inline template must produce valid module-level JS.
    /// The inline render function `return (_ctx,_cache) => {` must be inside setup(),
    /// not at module level after the component definition closes.
    #[test]
    fn test_prod_inline_template_valid_js() {
        let code = gen_prod(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        // The production output should be valid JS at module level (no wrapping needed)
        assert_valid_js(&code, "production inline template");
    }

    /// @ai-generated — Template-before-script in production must still use inline mode
    /// AND produce valid JS. The template content should be moved inside setup().
    #[test]
    fn test_prod_template_before_script_valid_js() {
        let code = gen_prod(
            "<template><div class=\"text-sm\">{{ msg }}</div></template>\n<script setup lang=\"ts\">\nconst msg = ref('Hello')\n</script>",
        );
        assert_valid_js(&code, "production template-before-script");
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Template-before-script should still use inline mode, got:\n{code}"
        );
    }

    /// @ai-generated — Template-before-script: dev mode must also produce valid JS.
    /// Dev mode uses function render() form (not inline), so this mainly tests
    /// that the function render is valid when template precedes script.
    #[test]
    fn test_dev_template_before_script_valid_js() {
        let code = gen(
            "<template><div class=\"text-sm\">{{ msg }}</div></template>\n<script setup lang=\"ts\">\nconst msg = ref('Hello')\n</script>",
        );
        assert_valid_js(&code, "dev template-before-script");
    }

    /// @ai-generated — Production inline template: render function is inside setup()
    #[test]
    fn test_prod_inline_template_inside_setup() {
        let code = gen_prod(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        // Should have the inline arrow function
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Should have inline template arrow function, got:\n{code}"
        );
        // Inline template replaces __returned__ — setup returns the render function directly
        assert!(
            !code.contains("__returned__"),
            "Inline template should not have __returned__, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline template: no __expose() auto-call
    #[test]
    fn test_prod_inline_no_auto_expose() {
        let code = gen_prod(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        assert!(
            !code.contains("__expose()"),
            "Production mode should not auto-call __expose(), got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: ref() bindings get .value suffix in template
    #[test]
    fn test_prod_inline_ref_gets_value_suffix() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div>{{ count }}</div></template>",
        );
        // In inline mode, ref bindings must be accessed as `count.value`
        assert!(
            code.contains("count.value"),
            "Inline ref should use count.value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: computed() bindings get .value suffix in template
    #[test]
    fn test_prod_inline_computed_gets_value_suffix() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref, computed } from 'vue'\nconst count = ref(0)\nconst doubled = computed(() => count.value * 2)\n</script>\n<template><div>{{ doubled }}</div></template>",
        );
        // computed() is classified as SetupRef and needs .value in inline mode
        assert!(
            code.contains("doubled.value"),
            "Inline computed should use doubled.value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: non-ref const does NOT get .value suffix
    #[test]
    fn test_prod_inline_const_no_value_suffix() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        // SetupConst (literal const) should NOT have .value
        assert!(
            !code.contains("msg.value"),
            "Literal const should NOT use .value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: ref in v-for iterable gets .value
    #[test]
    fn test_prod_inline_ref_in_vfor_gets_value() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst items = ref(['a','b','c'])\n</script>\n<template><div v-for=\"item in items\" :key=\"item\">{{ item }}</div></template>",
        );
        // The iterable `items` is a ref and should use .value in inline mode
        assert!(
            code.contains("items.value"),
            "Inline ref in v-for iterable should use items.value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: ref in v-if condition gets .value
    #[test]
    fn test_prod_inline_ref_in_vif_gets_value() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst show = ref(true)\n</script>\n<template><div v-if=\"show\">visible</div></template>",
        );
        assert!(
            code.contains("show.value"),
            "Inline ref in v-if should use show.value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: ref in event handler gets .value
    #[test]
    fn test_prod_inline_ref_in_event_handler_gets_value() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><button @click=\"count++\">click</button></template>",
        );
        assert!(
            code.contains("count.value"),
            "Inline ref in event handler should use count.value, got:\n{code}"
        );
    }

    /// @ai-generated — Production inline: template ref uses ref_key + ref variable (not string)
    /// Vue inline mode converts `ref="el"` to `ref_key: "el", ref: el` when el is a setup-ref.
    #[test]
    fn test_prod_inline_template_ref_uses_ref_key() {
        let code = gen_prod_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst el = ref()\n</script>\n<template><div ref=\"el\">hello</div></template>",
        );
        assert!(
            code.contains("ref_key: \"el\""),
            "Inline template ref should use ref_key, got:\n{code}"
        );
        assert!(
            code.contains(", ref: el"),
            "Inline template ref should reference the variable, got:\n{code}"
        );
    }

    /// @ai-generated — Dev mode: template ref stays as string (no ref_key needed)
    #[test]
    fn test_dev_template_ref_stays_string() {
        let code = gen_and_validate(
            "<script setup>\nimport { ref } from 'vue'\nconst el = ref()\n</script>\n<template><div ref=\"el\">hello</div></template>",
        );
        assert!(
            code.contains("ref: \"el\""),
            "Dev mode template ref should be a string, got:\n{code}"
        );
        assert!(
            !code.contains("ref_key"),
            "Dev mode should not have ref_key, got:\n{code}"
        );
    }

    // @ai-generated — Options API: production mode should NOT use inline render
    #[test]
    fn test_prod_options_api_no_inline_render() {
        let code = gen_prod_and_validate(
            "<script>\nimport { defineComponent } from 'vue'\nexport default defineComponent({ data() { return { count: 0 } } })\n</script>\n<template><div>{{ count }}</div></template>",
        );
        // Options API components should always use function render(), not inline arrow
        assert!(
            code.contains("function render("),
            "Options API in production should use function render(), got:\n{code}"
        );
        assert!(
            !code.contains("return (_ctx,_cache) => {"),
            "Options API in production should NOT use inline render arrow, got:\n{code}"
        );
    }

    // @ai-generated — Script setup: production mode SHOULD use inline render
    #[test]
    fn test_prod_script_setup_uses_inline_render() {
        let code = gen_prod_and_validate(
            "<script setup>\nconst count = 0\n</script>\n<template><div>{{ count }}</div></template>",
        );
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Script setup in production should use inline render arrow, got:\n{code}"
        );
        assert!(
            !code.contains("function render("),
            "Script setup in production should NOT use function render(), got:\n{code}"
        );
    }

    // @ai-generated — CSS v-bind with ref should use .value in _useCssVars callback
    #[test]
    fn test_css_v_bind_ref_uses_value_accessor() {
        let code = gen(r#"<script setup>
import { ref } from 'vue'
const themeColor = ref('red')
</script>
<template><div>{{ themeColor }}</div></template>
<style scoped>
.text { color: v-bind(themeColor); }
</style>"#);
        assert!(
            code.contains("_useCssVars"),
            "Should inject _useCssVars call, got:\n{code}"
        );
        assert!(
            code.contains("themeColor.value"),
            "Should use themeColor.value (not _ctx.themeColor) since it's a ref, got:\n{code}"
        );
        assert!(
            !code.contains("_ctx.themeColor"),
            "Should NOT use _ctx.themeColor for setup ref bindings, got:\n{code}"
        );
    }

    // =========================================================================
    // Scoped Styles: __sfc__ wrapping (AST-based export default)
    // =========================================================================

    /// @ai-generated — Script setup with scoped style should use const __sfc__ pattern
    #[test]
    fn test_scoped_style_uses_sfc_variable() {
        let code = gen_and_validate(
            "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>\n<style scoped>\n.red { color: red }\n</style>",
        );
        assert!(
            code.contains("const __sfc__ = /*@__PURE__*/"),
            "Should use const __sfc__ for scoped styles, got:\n{code}"
        );
        assert!(
            code.contains("__sfc__.__scopeId"),
            "Should set __scopeId on __sfc__, got:\n{code}"
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should export __sfc__ at the end, got:\n{code}"
        );
        // Must have exactly ONE export default statement
        let export_count = code.matches("export default ").count();
        assert_eq!(
            export_count, 1,
            "Should have exactly one export default, got {export_count} in:\n{code}"
        );
    }

    /// @ai-generated — Regression: script body with "export default" in a comment must
    /// NOT cause duplicate export default statements in scoped output
    #[test]
    fn test_scoped_with_export_default_in_comment() {
        let code = gen_and_validate(
            r#"<script setup>
const msg = 'hi'
// Transform: export default X -> something
</script>
<template><div>{{ msg }}</div></template>
<style scoped>
.red { color: red }
</style>"#,
        );
        // The comment text contains "export default" but the output is valid JS
        // (gen_and_validate already ensures that). Verify the actual export is
        // the compiler-generated `export default __sfc__` at the end.
        assert!(
            code.contains("export default __sfc__"),
            "Should have export default __sfc__, got:\n{code}"
        );
        // Count only export default statements that start a line (not inside comments)
        let export_stmt_count = code
            .lines()
            .filter(|line| line.trim_start().starts_with("export default "))
            .count();
        assert_eq!(
            export_stmt_count, 1,
            "Should have exactly one export default statement, got {export_stmt_count} in:\n{code}"
        );
    }

    /// @ai-generated — Regular <script> with scoped style: export default from AST
    #[test]
    fn test_regular_script_scoped_style() {
        let code = gen_and_validate(
            "<script>\nexport default { name: 'Foo' }\n</script>\n<template><div>x</div></template>\n<style scoped>\n.red { color: red }\n</style>",
        );
        assert!(
            code.contains("const __sfc__ ="),
            "Scoped regular script should use const __sfc__, got:\n{code}"
        );
        assert!(
            code.contains("__sfc__.__scopeId"),
            "Should set __scopeId on __sfc__, got:\n{code}"
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should export __sfc__ at the end, got:\n{code}"
        );
        let export_count = code.matches("export default ").count();
        assert_eq!(
            export_count, 1,
            "Should have exactly one export default, got {export_count} in:\n{code}"
        );
    }

    /// @ai-generated — Non-scoped setup should still use __sfc__ + export default pattern
    #[test]
    fn test_non_scoped_setup_uses_sfc_variable() {
        let code = gen_and_validate(
            "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        );
        assert!(
            code.contains("const __sfc__ = /*@__PURE__*/"),
            "Should use const __sfc__ pattern, got:\n{code}"
        );
        assert!(
            code.contains("export default __sfc__"),
            "Should export __sfc__ at the end, got:\n{code}"
        );
        assert!(
            !code.contains("__sfc__.__scopeId"),
            "Non-scoped should not set __scopeId, got:\n{code}"
        );
    }

    // =========================================================================
    // TypeScript Declaration Stripping
    // =========================================================================

    /// @ai-generated — TypeScript interface declarations must be stripped from JS output
    #[test]
    fn test_ts_interface_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
import { computed } from 'vue';

interface Props {
  codes?: string[];
}

const props = withDefaults(defineProps<Props>(), {
  codes: () => [],
});

const hasAccess = computed(() => props.codes.length > 0);
</script>
<template><div v-if="hasAccess"><slot /></div></template>"#,
        );
        assert!(
            !code.contains("interface"),
            "TypeScript interface should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — TypeScript type alias declarations must be stripped from JS output
    #[test]
    fn test_ts_type_alias_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
type Status = 'active' | 'inactive';

const status: Status = 'active';
</script>
<template><div>{{ status }}</div></template>"#,
        );
        assert!(
            !code.contains("type Status"),
            "TypeScript type alias should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — TypeScript enum declarations must be stripped from JS output
    #[test]
    fn test_ts_enum_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
enum Direction {
  Up,
  Down,
  Left,
  Right,
}

const dir = Direction.Up;
</script>
<template><div>{{ dir }}</div></template>"#,
        );
        assert!(
            !code.contains("enum Direction"),
            "TypeScript enum should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — Multiple TypeScript declarations should all be stripped
    #[test]
    fn test_ts_multiple_declarations_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
interface User {
  name: string;
  age: number;
}

type Role = 'admin' | 'user';

const user = { name: 'test', age: 25 };
</script>
<template><div>{{ user.name }}</div></template>"#,
        );
        assert!(
            !code.contains("interface User"),
            "TypeScript interface should be stripped, got:\n{}",
            code
        );
        assert!(
            !code.contains("type Role"),
            "TypeScript type alias should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — declare const/function should be stripped from JS output
    #[test]
    fn test_ts_declare_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
declare const __brand: unique symbol;
declare function assertNever(x: never): never;

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#,
        );
        assert!(
            !code.contains("declare"),
            "TypeScript declare should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — export interface should be stripped from JS output
    #[test]
    fn test_ts_export_interface_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
export interface MyProps {
  value: string;
}

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#,
        );
        assert!(
            !code.contains("interface"),
            "Exported TypeScript interface should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — export type alias should be stripped from JS output
    #[test]
    fn test_ts_export_type_alias_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
export type MyType = string | number;

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#,
        );
        assert!(
            !code.contains("type MyType"),
            "Exported TypeScript type alias should be stripped, got:\n{}",
            code
        );
    }

    /// @ai-generated — namespace declaration should be stripped from JS output
    #[test]
    fn test_ts_namespace_stripped() {
        let code = gen_and_validate(
            r#"<script lang="ts" setup>
namespace MyNS {
  export interface Foo {
    bar: string;
  }
}

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#,
        );
        assert!(
            !code.contains("namespace"),
            "TypeScript namespace should be stripped, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Template-only Components (no <script> block)
    // =========================================================================

    /// @ai-generated — Template-only component must have `export default` so bundlers can import it
    #[test]
    fn test_template_only_component_has_default_export() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            code.contains("export default"),
            "Template-only component should have export default, got:\n{}",
            code
        );
    }

    /// @ai-generated — Template-only component must have `const __sfc__` scaffolding
    #[test]
    fn test_template_only_component_has_sfc_scaffolding() {
        let code = gen_and_validate(
            r#"<template>
  <div class="footer">
    <span>Footer text</span>
  </div>
</template>"#,
        );
        assert!(
            code.contains("const __sfc__"),
            "Template-only component should have const __sfc__, got:\n{}",
            code
        );
        assert!(
            code.contains("export default __sfc__"),
            "Template-only component should have export default __sfc__, got:\n{}",
            code
        );
    }

    /// @ai-generated — Template-only component with scoped style must include __scopeId
    #[test]
    fn test_template_only_component_scoped_style() {
        let code = gen_and_validate(
            r#"<template><div class="red">hello</div></template>
<style scoped>
.red { color: red }
</style>"#,
        );
        assert!(
            code.contains("export default"),
            "Template-only component with scoped style should have export default, got:\n{}",
            code
        );
        assert!(
            code.contains("__sfc__.__scopeId"),
            "Template-only component with scoped style should have __scopeId, got:\n{}",
            code
        );
    }
}
