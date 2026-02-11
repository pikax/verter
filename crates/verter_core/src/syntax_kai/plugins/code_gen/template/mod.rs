use std::{cell::RefCell, rc::Rc};

use rustc_hash::FxHashMap;

use crate::{
    code_transform::{self, CodeTransform},
    syntax_kai::{
        binding_types::BindingType,
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        plugins::code_gen::{
            template::interpolation::handle_interpolation, types::TemplateImportDependencies,
        },
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, ElementScope, Event,
            OxcCompiledElementClosed, OxcCompiledElementStart, OxcInterpolation, Text,
        },
    },
    utils::vue::{PatchFlag, PatchFlags},
};

pub mod helper;
pub mod interpolation;

struct StateStack {
    id: u32,
    parent_id: u32,

    no_tracking: bool,

    has_once: bool,
    has_condition: bool,

    is_once: bool,

    children_count: u16,
    children_patch_flag: PatchFlag,

    cache_id: Option<u16>,
}
impl StateStack {
    pub fn new() -> Self {
        Self {
            id: 0,
            parent_id: 0,
            no_tracking: false,
            has_once: false,
            has_condition: false,

            is_once: false,

            children_count: 0,
            children_patch_flag: PatchFlag::empty(),
            cache_id: None,
        }
    }

    pub fn create_child(&mut self, element_id: u32) -> Self {
        self.children_count += 1;

        Self {
            id: element_id,
            parent_id: self.id,
            no_tracking: self.no_tracking,
            has_once: self.has_once || self.is_once,
            has_condition: self.has_condition,

            is_once: false,
            children_count: 0,
            children_patch_flag: PatchFlag::empty(),
            cache_id: None,
        }
    }
}

pub struct TemplateGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,

    bindings: FxHashMap<&'alloc str, BindingType>,

    is_production: bool,

    is_vapor: bool,

    imports: TemplateImportDependencies,

    stack: Vec<StateStack>,

    cache_id_counter: u16,
}

impl<'alloc> TemplateGeneratorPlugin<'alloc> {
    pub fn new(code_transform: Rc<RefCell<CodeTransform<'alloc>>>, is_production: bool) -> Self {
        Self {
            code_transform,
            is_production,

            is_vapor: false,

            imports: TemplateImportDependencies::default(),
            bindings: FxHashMap::default(),
            stack: Vec::with_capacity(50),
            cache_id_counter: 0,
        }
    }

    /// Get the transformed code (template block only).
    pub fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    pub fn generate_source_map(&self) -> String {
        self.code_transform
            .borrow()
            .generate_map_json(Default::default())
    }

    fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.stack.push(StateStack::new());

        let code_transform = &mut self.code_transform.borrow_mut();

        if ev.vapor.is_some() {
            self.is_vapor = true;
        }

        // clean template opening tag

        if self.is_production {
            code_transform.replace(
                ev.tag_open.start,
                ev.tag_open.end,
                "return (_ctx,_cache) => {",
            );
        } else {
            code_transform.replace(
                ev.tag_open.start,
                ev.tag_open.end,
                "function render(_ctx, _cache, $props, $setup, $data, $options) {",
            );
        }
    }

    fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let code_transform = &mut self.code_transform.borrow_mut();

        let extra_return = if let Some(state) = self.stack.pop() {
            if state.children_count == 0 {
                "return null"
            } else {
                ""
            }
        } else {
            "return null"
        };

        if let Some(close) = &ev.tag_close {
            // clean template closing tag
            code_transform.replace(
                close.start,
                close.end,
                format!("{}}}", extra_return).as_str(),
            );
        } else {
            // If template closing tag is missing, append a closing brace at the end of the template.
            code_transform.append_right(ev.end, format!("{}}}", extra_return).as_str());
        }
    }

    fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let mut state = self
            .stack
            .last_mut()
            .expect("Element start should be within template")
            .create_child(ev.event.element_id);

        // Example: transform <div> to <div data-foo="bar">
        // self.code_transform.borrow_mut().append_left(ev.tag_name_end, " data-foo=\"bar\"");
        let mut code_transform = self.code_transform.borrow_mut();

        code_transform.append_left(ev.event.event_open_tag.start, "-->");

        // let mut has_scope = false;

        for scope in &ev.scopes {
            match scope {
                ElementScope::If(_) => state.has_condition = true,
                ElementScope::Once(prop) => {
                    state.has_once = true;
                    code_transform.remove(prop.start, prop.end);

                    // Drop the RefMut so we can mutably borrow self
                    drop(code_transform);

                    let content = self
                        .start_with_cache_prepend("", "_setBlockTracking(-1, true),")
                        .as_str()
                        .to_string();

                    self.imports
                        .add(TemplateImportDependencies::SET_BLOCK_TRACKING);

                    // Re-borrow after the &mut self calls are done
                    code_transform = self.code_transform.borrow_mut();
                    code_transform.append_left(prop.element_id, content.as_str());
                }

                _ => {}
            }
        }

        self.stack.push(state);
    }

    fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let mut code_transform = self.code_transform.borrow_mut();
        let mut state = self
            .stack
            .last_mut()
            .expect("Element start should be within template")
            .create_child(ev.event.element_id);
        // Example: transform <div> to <div data-foo="bar">
        // self.code_transform.borrow_mut().append_left(ev.tag_name_end, " data-foo=\"bar\"");

        if let Some(close) = &ev.event.event_close_tag {
            self.code_transform
                .borrow_mut()
                .append_left(close.end, "<--");
        }

        if state.is_once {
            // restore tracking

            let end = if let Some(close) = &ev.event.event_close_tag {
                close.end
            } else {
                ev.event.element_id
            };

            code_transform.append_left(end, "");
        }

        self.stack.pop();
    }

    fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        // Handle comments if needed
    }

    fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        // Handle text nodes if needed
    }

    fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // Handle interpolation if needed
        handle_interpolation(
            &mut self.code_transform.borrow_mut(),
            ev,
            &self.bindings,
            self.is_production,
        );

        self.imports
            .add(TemplateImportDependencies::TO_DISPLAY_STRING);
    }

    fn start_with_cache(&mut self, content: &str) -> String {
        self.start_with_cache_prepend(content, "")
    }
    fn end_with_cache(&mut self, content: &str) -> String {
        let state = self
            .stack
            .last_mut()
            .expect("Cache should be within template");
        if let Some(cache_id) = state.cache_id {
            format!(
                "_cache[{}] || (_cache[{}] = {})",
                cache_id, cache_id, content
            )
        } else {
            content.to_string()
        }
    }

    fn start_with_cache_prepend(&mut self, content: &str, prepend: &str) -> String {
        let cache_id = self.cache_id_counter;
        self.cache_id_counter += 1;

        let state = self
            .stack
            .last_mut()
            .expect("Cache should be within template");
        state.cache_id = Some(cache_id);

        format!(
            "_cache[{}] || {}(_cache[{}] = {}",
            cache_id, prepend, cache_id, content
        )
    }

    // TODO investigate
    // fn end_with_cache_append(&mut self, content: &str, append: &str) -> String {
    //     let state = self
    //         .stack
    //         .last_mut()
    //         .expect("Cache should be within template");
    //     if let Some(cache_id) = state.cache_id {
    //         format!(
    //             "_cache[{}] || (_cache[{}] = {}{})",
    //             cache_id, cache_id, content, append
    //         )
    //     } else {
    //         format!("{}{}", content, append)
    //     }
    // }
}

impl<'alloc> SyntaxPlugin<'alloc> for TemplateGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "TemplateGeneratorPlugin"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::OxcScript(ev) => {
                ev.result.bindings.iter().for_each(|(name, binding)| {
                    self.bindings
                        .insert(&ctx.input[name.start as usize..name.end as usize], *binding);
                });
                // Skip processing script content in template plugin
                SyntaxResult::keep(Event::OxcScript(ev))
            }

            Event::CompiledTemplateStart(ev) => {
                self.handle_template_start(&ev, ctx);
                SyntaxResult::keep(Event::CompiledTemplateStart(ev))
            }
            Event::CompiledTemplateEnd(ev) => {
                self.handle_template_closed(&ev, ctx);
                SyntaxResult::keep(Event::CompiledTemplateEnd(ev))
            }

            Event::OxcCompiledElementStart(ev) => {
                self.handle_element_start(&ev, ctx);
                SyntaxResult::keep(Event::OxcCompiledElementStart(ev))
            }
            Event::OxcCompiledElementClosed(ev) => {
                self.handle_element_closed(&ev, ctx);
                SyntaxResult::keep(Event::OxcCompiledElementClosed(ev))
            }

            Event::Comment(ev) => {
                self.handle_comment(&ev, ctx);
                SyntaxResult::keep(Event::Comment(ev))
            }
            Event::Text(ev) => {
                self.handle_text(&ev, ctx);
                SyntaxResult::keep(Event::Text(ev))
            }
            Event::OxcInterpolation(ev) => {
                self.handle_interpolation(&ev, ctx);
                SyntaxResult::keep(Event::OxcInterpolation(ev))
            }
            // Event::
            _ => SyntaxResult::keep(event),
        }
    }
}
