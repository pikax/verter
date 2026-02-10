use oxc_ast::ast::*;

use crate::{
    common::Span,
    syntax_kai::{
        binding_types::{BindingMetadata, BindingType},
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::*,
    },
};

/// Code Gen Script Plugin for the syntax_kai pipeline.
///
/// Processes `OxcScript` events to extract binding metadata from `<script setup>` blocks.
/// Emits `Event::ScriptBindings(BindingMetadata)` for downstream codegen plugins.
///
/// Binding classification follows Vue's official compiler:
/// - `const x = 'literal'` → `LiteralConst`
/// - `const x = ref(...)` / `computed(...)` → `SetupRef`
/// - `const x = reactive({})` → `SetupReactiveConst`
/// - `const x = useSomething()` → `SetupMaybeRef`
/// - `let x = ...` → `SetupLet`
/// - `import x from '...'` → `SetupConst`
/// - `defineProps<{msg: string}>()` → `Props` for msg
pub struct CodeGenScriptPlugin<'alloc> {
    _marker: std::marker::PhantomData<&'alloc ()>,
}

impl<'alloc> Default for CodeGenScriptPlugin<'alloc> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> CodeGenScriptPlugin<'alloc> {
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Extract binding metadata from an OxcScript's parsed program.
    fn extract_bindings(
        &self,
        script: &OxcScript<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> BindingMetadata {
        let mut metadata = BindingMetadata::default();

        // Only extract bindings from <script setup>
        if script.setup.is_none() {
            return metadata;
        }

        // OXC program spans are relative to the parsed slice.
        // We need to offset them to be relative to the full SFC source.
        let offset = script.content_start;

        for stmt in &script.program.body {
            self.classify_statement(stmt, &mut metadata, ctx, offset);
        }

        metadata
    }

    /// Classify a single top-level statement for binding types.
    fn classify_statement(
        &self,
        stmt: &Statement<'alloc>,
        metadata: &mut BindingMetadata,
        ctx: &SyntaxPluginContext<'alloc>,
        offset: u32,
    ) {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                self.classify_variable_declaration(decl, metadata, ctx, offset);
            }
            Statement::ImportDeclaration(import) => {
                self.classify_import(import, metadata, offset);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.classify_expression_statement(&expr_stmt.expression, metadata, ctx, offset);
            }
            _ => {}
        }
    }

    /// Classify variable declarations: const/let/var.
    fn classify_variable_declaration(
        &self,
        decl: &VariableDeclaration<'alloc>,
        metadata: &mut BindingMetadata,
        ctx: &SyntaxPluginContext<'alloc>,
        offset: u32,
    ) {
        let is_const = decl.kind == VariableDeclarationKind::Const;

        for declarator in &decl.declarations {
            let binding_type = if is_const {
                if let Some(init) = &declarator.init {
                    self.classify_const_init(init, ctx)
                } else {
                    BindingType::SetupConst
                }
            } else {
                BindingType::SetupLet
            };

            // Handle destructuring from defineProps
            if is_const {
                if let Some(init) = &declarator.init {
                    if self.is_define_props_call(init) {
                        self.extract_destructured_props(&declarator.id, metadata, offset);
                        continue;
                    }
                }
            }

            // Extract binding name(s) from pattern
            self.extract_pattern_bindings(&declarator.id, binding_type, metadata, offset);
        }
    }

    /// Classify the initializer of a `const` declaration.
    fn classify_const_init(
        &self,
        init: &Expression<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> BindingType {
        match init {
            Expression::StringLiteral(_)
            | Expression::NumericLiteral(_)
            | Expression::BooleanLiteral(_)
            | Expression::NullLiteral(_)
            | Expression::BigIntLiteral(_) => BindingType::LiteralConst,

            Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
                BindingType::LiteralConst
            }

            Expression::CallExpression(call) => self.classify_call_expression(call, ctx),

            _ => BindingType::SetupConst,
        }
    }

    /// Classify a call expression in a const initializer.
    fn classify_call_expression(
        &self,
        call: &CallExpression<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> BindingType {
        let callee_name = self.get_callee_name(&call.callee);

        match callee_name.as_deref() {
            Some("ref" | "computed" | "shallowRef" | "toRef" | "customRef" | "defineModel") => {
                BindingType::SetupRef
            }
            Some("reactive" | "shallowReactive") => BindingType::SetupReactiveConst,
            Some(name) if name.starts_with("use") => BindingType::SetupMaybeRef,
            Some("defineProps" | "withDefaults") => BindingType::SetupConst,
            _ => BindingType::SetupConst,
        }
    }

    /// Get the callee name from a call expression.
    fn get_callee_name(&self, callee: &Expression<'alloc>) -> Option<String> {
        match callee {
            Expression::Identifier(ident) => Some(ident.name.to_string()),
            _ => None,
        }
    }

    /// Check if an expression is a defineProps() call.
    fn is_define_props_call(&self, expr: &Expression<'alloc>) -> bool {
        match expr {
            Expression::CallExpression(call) => {
                matches!(
                    self.get_callee_name(&call.callee).as_deref(),
                    Some("defineProps" | "withDefaults")
                )
            }
            _ => false,
        }
    }

    /// Extract destructured props: `const { msg: m } = defineProps<...>()`
    fn extract_destructured_props(
        &self,
        pattern: &BindingPattern<'alloc>,
        metadata: &mut BindingMetadata,
        offset: u32,
    ) {
        match pattern {
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.extract_pattern_bindings(
                        &prop.value,
                        BindingType::PropsAliased,
                        metadata,
                        offset,
                    );
                }
                if let Some(rest) = &obj.rest {
                    self.extract_pattern_bindings(
                        &rest.argument,
                        BindingType::PropsAliased,
                        metadata,
                        offset,
                    );
                }
            }
            BindingPattern::BindingIdentifier(ident) => {
                let span = Span::new(ident.span.start + offset, ident.span.end + offset);
                metadata.entries.push((span, BindingType::Props));
            }
            _ => {}
        }
    }

    /// Extract binding names from a pattern and add to metadata.
    fn extract_pattern_bindings(
        &self,
        pattern: &BindingPattern<'alloc>,
        binding_type: BindingType,
        metadata: &mut BindingMetadata,
        offset: u32,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(ident) => {
                let span = Span::new(ident.span.start + offset, ident.span.end + offset);
                metadata.entries.push((span, binding_type));
            }
            BindingPattern::ObjectPattern(obj) => {
                for prop in &obj.properties {
                    self.extract_pattern_bindings(&prop.value, binding_type, metadata, offset);
                }
                if let Some(rest) = &obj.rest {
                    self.extract_pattern_bindings(&rest.argument, binding_type, metadata, offset);
                }
            }
            BindingPattern::ArrayPattern(arr) => {
                for elem in arr.elements.iter().flatten() {
                    self.extract_pattern_bindings(elem, binding_type, metadata, offset);
                }
                if let Some(rest) = &arr.rest {
                    self.extract_pattern_bindings(&rest.argument, binding_type, metadata, offset);
                }
            }
            BindingPattern::AssignmentPattern(assign) => {
                self.extract_pattern_bindings(&assign.left, binding_type, metadata, offset);
            }
        }
    }

    /// Classify standalone expression statements (e.g., defineProps<{msg: string}>()).
    fn classify_expression_statement(
        &self,
        expr: &Expression<'alloc>,
        metadata: &mut BindingMetadata,
        ctx: &SyntaxPluginContext<'alloc>,
        offset: u32,
    ) {
        if let Expression::CallExpression(call) = expr {
            let callee_name = self.get_callee_name(&call.callee);
            match callee_name.as_deref() {
                Some("defineProps") => {
                    if let Some(type_args) = &call.type_arguments {
                        self.extract_props_from_type_params(type_args, metadata, ctx, offset);
                    }
                }
                Some("withDefaults") => {
                    if let Some(first_arg) = call.arguments.first() {
                        if let Some(Expression::CallExpression(inner_call)) =
                            first_arg.as_expression()
                        {
                            if let Some(tp) = &inner_call.type_arguments {
                                self.extract_props_from_type_params(tp, metadata, ctx, offset);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Extract prop names from TypeScript type parameters of defineProps.
    fn extract_props_from_type_params(
        &self,
        type_params: &TSTypeParameterInstantiation<'alloc>,
        metadata: &mut BindingMetadata,
        _ctx: &SyntaxPluginContext<'alloc>,
        offset: u32,
    ) {
        if let Some(first_param) = type_params.params.first() {
            if let TSType::TSTypeLiteral(literal) = first_param {
                for member in &literal.members {
                    if let TSSignature::TSPropertySignature(prop) = member {
                        if let PropertyKey::StaticIdentifier(ident) = &prop.key {
                            let span =
                                Span::new(ident.span.start + offset, ident.span.end + offset);
                            metadata.entries.push((span, BindingType::Props));
                        }
                    }
                }
            }
        }
    }

    /// Classify import declarations.
    fn classify_import(
        &self,
        import: &ImportDeclaration<'alloc>,
        metadata: &mut BindingMetadata,
        offset: u32,
    ) {
        if let Some(specifiers) = &import.specifiers {
            for spec in specifiers {
                let span = match spec {
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        Span::new(s.local.span.start + offset, s.local.span.end + offset)
                    }
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        Span::new(s.local.span.start + offset, s.local.span.end + offset)
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        Span::new(s.local.span.start + offset, s.local.span.end + offset)
                    }
                };
                metadata.entries.push((span, BindingType::SetupConst));
            }
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for CodeGenScriptPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_script"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::OxcScript(ref script) => {
                let metadata = self.extract_bindings(script, ctx);
                if !metadata.is_empty() {
                    SyntaxResult::Keep(Event::ScriptBindings(metadata))
                } else {
                    SyntaxResult::Keep(event)
                }
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::binding_types::ReactivityLevel;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::plugins::oxc_parser::oxc_parser::OxcParserPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;
    use oxc_allocator::Allocator;

    /// Helper: run full pipeline (tokenize → syntax → element_compiler → oxc_parser → code_gen_script)
    /// on root_script_events. Returns extracted BindingMetadata.
    fn extract_bindings(input: &str, alloc: &Allocator) -> BindingMetadata {
        let mut tokenizer_events = Vec::new();
        tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &options,
        };

        let mut events_storage: Vec<Event<'_>> = Vec::new();
        let root_script_events: Vec<Event<'_>>;
        let ptr = &mut events_storage as *mut Vec<Event<'_>>;
        {
            let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }
            root_script_events = syntax.take_root_script_events();
        }

        // Run element_compiler on root_script_events
        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in root_script_events {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run oxc_parser
        let mut oxc = OxcParserPlugin::new(alloc);
        let mut parsed = Vec::new();
        for event in compiled {
            match oxc.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Run code_gen_script
        let mut cgs = CodeGenScriptPlugin::new();
        let mut final_events = Vec::new();
        for event in parsed {
            match cgs.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => final_events.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // Find ScriptBindings event
        final_events
            .into_iter()
            .find_map(|e| match e {
                Event::ScriptBindings(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Helper to find a binding type by name.
    fn find_binding(metadata: &BindingMetadata, name: &str, source: &str) -> Option<BindingType> {
        metadata.get(name.as_bytes(), source.as_bytes())
    }

    /// @ai-generated - const with literal string → LiteralConst
    #[test]
    fn test_extract_const_literal() {
        let input = r#"<script setup>const x = 'hello'</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "x", input);
        assert_eq!(bt, Some(BindingType::LiteralConst));
    }

    /// @ai-generated - const with ref() → SetupRef
    #[test]
    fn test_extract_ref() {
        let input = r#"<script setup>const count = ref(0)</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "count", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - const with computed() → SetupRef
    #[test]
    fn test_extract_computed() {
        let input = r#"<script setup>const double = computed(() => count * 2)</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "double", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - const with reactive() → SetupReactiveConst
    #[test]
    fn test_extract_reactive() {
        let input = r#"<script setup>const state = reactive({})</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "state", input);
        assert_eq!(bt, Some(BindingType::SetupReactiveConst));
    }

    /// @ai-generated - let declaration → SetupLet
    #[test]
    fn test_extract_let() {
        let input = r#"<script setup>let x = 0</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "x", input);
        assert_eq!(bt, Some(BindingType::SetupLet));
    }

    /// @ai-generated - import → SetupConst
    #[test]
    fn test_extract_import() {
        let input = r#"<script setup>import Foo from './Foo.vue'</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "Foo", input);
        assert_eq!(bt, Some(BindingType::SetupConst));
    }

    /// @ai-generated - defineModel() → SetupRef
    #[test]
    fn test_extract_define_model() {
        let input = r#"<script setup>const model = defineModel()</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "model", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - useSomething() → SetupMaybeRef
    #[test]
    fn test_extract_use_composable() {
        let input = r#"<script setup>const data = useFetch('/api')</script>"#;
        let alloc = Allocator::default();
        let metadata = extract_bindings(input, &alloc);
        let bt = find_binding(&metadata, "data", input);
        assert_eq!(bt, Some(BindingType::SetupMaybeRef));
    }

    /// @ai-generated - ReactivityLevel::Static for SetupConst and LiteralConst
    #[test]
    fn test_reactivity_level_static() {
        assert_eq!(
            BindingType::SetupConst.reactivity_level(),
            ReactivityLevel::Static
        );
        assert_eq!(
            BindingType::LiteralConst.reactivity_level(),
            ReactivityLevel::Static
        );
    }

    /// @ai-generated - ReactivityLevel::Dynamic for all other types
    #[test]
    fn test_reactivity_level_dynamic() {
        assert_eq!(
            BindingType::SetupRef.reactivity_level(),
            ReactivityLevel::Dynamic
        );
        assert_eq!(
            BindingType::SetupLet.reactivity_level(),
            ReactivityLevel::Dynamic
        );
    }
}
