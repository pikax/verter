//! Template code generation orchestrator.
//!
//! This module detects whether the template uses VDOM or Vapor mode and
//! delegates to the appropriate backend. The mode is determined by the
//! presence of a `vapor` attribute on `<template vapor>`.
//!
//! OxcScript events (carrying bindings) arrive before `CompiledTemplateStart`,
//! so the orchestrator buffers bindings and passes them to the backend when
//! the mode is known.

use std::{cell::RefCell, rc::Rc};

use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::Event,
    },
};

pub(crate) mod shared;
pub(crate) mod vapor;
pub(crate) mod vdom;

use vapor::VaporTemplateGenerator;
use vdom::VdomTemplateGenerator;

enum Backend<'alloc> {
    Uninitialized,
    Vdom(VdomTemplateGenerator<'alloc>),
    Vapor(VaporTemplateGenerator<'alloc>),
}

pub struct TemplateGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    is_production: bool,
    bindings: FxHashMap<&'alloc str, BindingType>,
    backend: Backend<'alloc>,
}

impl<'alloc> TemplateGeneratorPlugin<'alloc> {
    pub fn new(code_transform: Rc<RefCell<CodeTransform<'alloc>>>, is_production: bool) -> Self {
        Self {
            code_transform,
            is_production,
            bindings: FxHashMap::default(),
            backend: Backend::Uninitialized,
        }
    }

    /// Get the transformed code (template block only).
    pub fn get_code(&self) -> String {
        match &self.backend {
            Backend::Vdom(gen) => gen.get_code(),
            Backend::Vapor(gen) => gen.get_code(),
            Backend::Uninitialized => self.code_transform.borrow().to_string(),
        }
    }

    pub fn generate_source_map(&self) -> String {
        match &self.backend {
            Backend::Vdom(gen) => gen.generate_source_map(),
            Backend::Vapor(gen) => gen.generate_source_map(),
            Backend::Uninitialized => self
                .code_transform
                .borrow()
                .generate_map_json(Default::default()),
        }
    }

    /// Initialize the appropriate backend based on the vapor flag.
    fn init_backend(&mut self, is_vapor: bool) {
        let mut gen: Backend<'alloc> = if is_vapor {
            Backend::Vapor(VaporTemplateGenerator::new(
                Rc::clone(&self.code_transform),
                self.is_production,
            ))
        } else {
            Backend::Vdom(VdomTemplateGenerator::new(
                Rc::clone(&self.code_transform),
                self.is_production,
            ))
        };

        // Pass buffered bindings to the backend
        let bindings = std::mem::take(&mut self.bindings);
        match &mut gen {
            Backend::Vdom(g) => g.set_bindings(bindings),
            Backend::Vapor(g) => g.set_bindings(bindings),
            Backend::Uninitialized => unreachable!(),
        }

        self.backend = gen;
    }
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
            // Buffer bindings before template mode is known.
            Event::OxcScript(ev) => {
                ev.result.bindings.iter().for_each(|(name, binding)| {
                    self.bindings
                        .insert(&ctx.input[name.start as usize..name.end as usize], *binding);
                });
                SyntaxResult::keep(Event::OxcScript(ev))
            }

            // Detect mode and initialize the backend.
            Event::CompiledTemplateStart(ev) => {
                let is_vapor = ev.vapor.is_some();
                self.init_backend(is_vapor);

                match &mut self.backend {
                    Backend::Vdom(gen) => gen.handle_template_start(&ev, ctx),
                    Backend::Vapor(gen) => gen.handle_template_start(&ev, ctx),
                    Backend::Uninitialized => unreachable!(),
                }
                SyntaxResult::keep(Event::CompiledTemplateStart(ev))
            }

            Event::CompiledTemplateEnd(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) => gen.handle_template_closed(&ev, ctx),
                    Backend::Vapor(gen) => gen.handle_template_closed(&ev, ctx),
                    Backend::Uninitialized => {}
                }
                SyntaxResult::keep(Event::CompiledTemplateEnd(ev))
            }

            Event::OxcCompiledElementStart(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) => gen.handle_element_start(&ev, ctx),
                    Backend::Vapor(gen) => gen.handle_element_start(&ev, ctx),
                    Backend::Uninitialized => {}
                }
                SyntaxResult::keep(Event::OxcCompiledElementStart(ev))
            }
            Event::OxcCompiledElementClosed(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) => gen.handle_element_closed(&ev, ctx),
                    Backend::Vapor(gen) => gen.handle_element_closed(&ev, ctx),
                    Backend::Uninitialized => {}
                }
                SyntaxResult::keep(Event::OxcCompiledElementClosed(ev))
            }

            // Text, comment, interpolation events outside <template> are skipped.
            // These can arrive before CompiledTemplateStart or after CompiledTemplateEnd
            // when the stack is empty.
            Event::Comment(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) if gen.is_inside_template() => {
                        gen.handle_comment(&ev, ctx);
                    }
                    Backend::Vapor(gen) if gen.is_inside_template() => {
                        gen.handle_comment(&ev, ctx);
                    }
                    _ => {}
                }
                SyntaxResult::keep(Event::Comment(ev))
            }
            Event::Text(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) if gen.is_inside_template() => {
                        gen.handle_text(&ev, ctx);
                    }
                    Backend::Vapor(gen) if gen.is_inside_template() => {
                        gen.handle_text(&ev, ctx);
                    }
                    _ => {}
                }
                SyntaxResult::keep(Event::Text(ev))
            }
            Event::OxcInterpolation(ev) => {
                match &mut self.backend {
                    Backend::Vdom(gen) if gen.is_inside_template() => {
                        gen.handle_interpolation(&ev, ctx);
                    }
                    Backend::Vapor(gen) if gen.is_inside_template() => {
                        gen.handle_interpolation(&ev, ctx);
                    }
                    _ => {}
                }
                SyntaxResult::keep(Event::OxcInterpolation(ev))
            }

            _ => SyntaxResult::keep(event),
        }
    }
}

#[cfg(test)]
mod tests;
