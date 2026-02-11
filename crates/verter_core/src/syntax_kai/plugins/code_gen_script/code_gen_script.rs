use crate::syntax_kai::{
    plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
    types::*,
};

/// Code Gen Script Plugin for the syntax_kai pipeline.
///
/// Passes through `OxcScript` events. Binding metadata is now populated
/// during `parse_script()` and lives in `OxcScript.result.bindings`.
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
}

impl<'alloc> SyntaxPlugin<'alloc> for CodeGenScriptPlugin<'alloc> {
    fn name(&self) -> &str {
        "code_gen_script"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        _ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        SyntaxResult::Keep(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Span;
    use crate::syntax_kai::binding_types::{get_binding_type, BindingType, ReactivityLevel};
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::plugins::oxc_parser::oxc_parser::OxcParserPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;
    use oxc_allocator::Allocator;

    /// Helper: run full pipeline (tokenize → syntax → element_compiler → oxc_parser → code_gen_script)
    /// and return the binding entries from the OxcScript result.
    fn extract_bindings(input: &str, alloc: &Allocator) -> Vec<(Span, BindingType)> {
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

        // Find OxcScript event and return its bindings
        parsed
            .into_iter()
            .find_map(|e| match e {
                Event::OxcScript(script) => Some(script.result.bindings.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Helper to find a binding type by name.
    fn find_binding(
        entries: &[(Span, BindingType)],
        name: &str,
        source: &str,
    ) -> Option<BindingType> {
        get_binding_type(entries, name.as_bytes(), source.as_bytes())
    }

    /// @ai-generated - const with literal string → LiteralConst
    #[test]
    fn test_extract_const_literal() {
        let input = r#"<script setup>const x = 'hello'</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "x", input);
        assert_eq!(bt, Some(BindingType::LiteralConst));
    }

    /// @ai-generated - const with ref() → SetupRef
    #[test]
    fn test_extract_ref() {
        let input = r#"<script setup>const count = ref(0)</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "count", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - const with computed() → SetupRef
    #[test]
    fn test_extract_computed() {
        let input = r#"<script setup>const double = computed(() => count * 2)</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "double", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - const with reactive() → SetupReactiveConst
    #[test]
    fn test_extract_reactive() {
        let input = r#"<script setup>const state = reactive({})</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "state", input);
        assert_eq!(bt, Some(BindingType::SetupReactiveConst));
    }

    /// @ai-generated - let declaration → SetupLet
    #[test]
    fn test_extract_let() {
        let input = r#"<script setup>let x = 0</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "x", input);
        assert_eq!(bt, Some(BindingType::SetupLet));
    }

    /// @ai-generated - import → SetupConst
    #[test]
    fn test_extract_import() {
        let input = r#"<script setup>import Foo from './Foo.vue'</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "Foo", input);
        assert_eq!(bt, Some(BindingType::SetupConst));
    }

    /// @ai-generated - defineModel() → SetupRef
    #[test]
    fn test_extract_define_model() {
        let input = r#"<script setup>const model = defineModel()</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "model", input);
        assert_eq!(bt, Some(BindingType::SetupRef));
    }

    /// @ai-generated - useSomething() → SetupMaybeRef
    #[test]
    fn test_extract_use_composable() {
        let input = r#"<script setup>const data = useFetch('/api')</script>"#;
        let alloc = Allocator::default();
        let entries = extract_bindings(input, &alloc);
        let bt = find_binding(&entries, "data", input);
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
