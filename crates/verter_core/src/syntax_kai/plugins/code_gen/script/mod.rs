use std::{cell::RefCell, rc::Rc};

use crate::{
    code_transform::{self, CodeTransform, SourceMapOptions},
    cursor::ScriptLanguage,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        plugins::code_gen::{
            script::process::{process_script_event, ProcessScriptOptions},
            types::ScriptSetupImportDependencies,
        },
        types::{Event, OxcScript},
    },
};

pub mod macros;
pub mod process;
pub mod sections;

pub struct ScriptGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    component_name: &'alloc str,

    is_multi_script: bool,

    keep_ts_types: bool,
    is_production: bool,

    imports: ScriptSetupImportDependencies,
}

impl<'alloc> ScriptGeneratorPlugin<'alloc> {
    pub fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        component_name: &'alloc str,
        is_multi_script: bool,
        keep_ts_types: bool,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            component_name,
            is_multi_script,
            keep_ts_types,
            is_production,

            imports: ScriptSetupImportDependencies::default(),
        }
    }

    /// Get the transformed code (script block only).
    pub fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    /// Generate source map JSON string.
    pub fn generate_source_map(&self, options: SourceMapOptions) -> String {
        self.code_transform.borrow().generate_map_json(options)
    }

    fn process_script(&mut self, event: &OxcScript<'alloc>, ctx: &mut SyntaxPluginContext<'alloc>) {
        if self.is_multi_script {
            panic!(
                "Multiple <script> blocks are not supported in this version. Found at position {}.",
                event.start
            );
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
                inline_template: self.is_production,
            },
        );

        self.imports.add(processed.imports.0);
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for ScriptGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "ScriptGeneratorPlugin"
    }

    fn end(&mut self, _ctx: &SyntaxPluginContext<'alloc>) {
        // add imports to the top of the script
        if !self.imports.is_empty() {
            self.code_transform.borrow_mut().prepend(
                format!(
                    "import {{{}}} from 'vue';\n",
                    self.imports.to_import_string()
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
                self.process_script(&script, ctx);
            }
            _ => {}
        }
        SyntaxResult::Keep(event)
    }
}
