//! Template code generation orchestrator.
//!
//! This module detects whether the template uses VDOM or Vapor mode and
//! delegates to the appropriate backend via the [`TemplateBackend`] trait.
//! The mode is determined by the presence of a `vapor` attribute on
//! `<template vapor>`.
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
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, Event,
            OxcCompiledElementClosed, OxcCompiledElementStart, OxcInterpolation, Text,
        },
    },
};

pub(crate) mod shared;
pub(crate) mod vapor;
pub(crate) mod vdom;

use vapor::VaporTemplateGenerator;
use vdom::VdomTemplateGenerator;

/// Shared interface for VDOM and Vapor template code generation backends.
///
/// Both backends implement the same set of event handlers. This trait
/// eliminates duplicated match arms in the orchestrator's `process_event`.
trait TemplateBackend<'alloc> {
    fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>);
    fn is_inside_template(&self) -> bool;

    fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        ctx: &SyntaxPluginContext<'alloc>,
    );
    fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    );
    fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    );
    fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        ctx: &SyntaxPluginContext<'alloc>,
    );
    fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>);
    fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>);
    fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    );
}

impl<'alloc> TemplateBackend<'alloc> for VdomTemplateGenerator<'alloc> {
    fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.set_bindings(bindings);
    }
    fn is_inside_template(&self) -> bool {
        self.is_inside_template()
    }
    fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_template_start(ev, ctx);
    }
    fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_template_closed(ev, ctx);
    }
    fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_element_start(ev, ctx);
    }
    fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_element_closed(ev, ctx);
    }
    fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        self.handle_comment(ev, ctx);
    }
    fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        self.handle_text(ev, ctx);
    }
    fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_interpolation(ev, ctx);
    }
}

impl<'alloc> TemplateBackend<'alloc> for VaporTemplateGenerator<'alloc> {
    fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.set_bindings(bindings);
    }
    fn is_inside_template(&self) -> bool {
        self.is_inside_template()
    }
    fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_template_start(ev, ctx);
    }
    fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_template_closed(ev, ctx);
    }
    fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_element_start(ev, ctx);
    }
    fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_element_closed(ev, ctx);
    }
    fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        self.handle_comment(ev, ctx);
    }
    fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        self.handle_text(ev, ctx);
    }
    fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.handle_interpolation(ev, ctx);
    }
}

enum Backend<'alloc> {
    Uninitialized,
    Vdom(Box<VdomTemplateGenerator<'alloc>>),
    Vapor(Box<VaporTemplateGenerator<'alloc>>),
}

impl<'alloc> Backend<'alloc> {
    /// Get a reference to the active backend, or `None` if uninitialized.
    fn as_backend(&mut self) -> Option<&mut dyn TemplateBackend<'alloc>> {
        match self {
            Backend::Vdom(gen) => Some(gen.as_mut()),
            Backend::Vapor(gen) => Some(gen.as_mut()),
            Backend::Uninitialized => None,
        }
    }
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
        self.backend = if is_vapor {
            Backend::Vapor(Box::new(VaporTemplateGenerator::new(
                Rc::clone(&self.code_transform),
                self.is_production,
            )))
        } else {
            Backend::Vdom(Box::new(VdomTemplateGenerator::new(
                Rc::clone(&self.code_transform),
                self.is_production,
            )))
        };

        // Pass buffered bindings to the backend.
        let bindings = std::mem::take(&mut self.bindings);
        self.backend
            .as_backend()
            .expect("backend just initialized")
            .set_bindings(bindings);
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
                self.backend
                    .as_backend()
                    .expect("backend just initialized")
                    .handle_template_start(&ev, ctx);
                SyntaxResult::keep(Event::CompiledTemplateStart(ev))
            }

            Event::CompiledTemplateEnd(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    gen.handle_template_closed(&ev, ctx);
                }
                SyntaxResult::keep(Event::CompiledTemplateEnd(ev))
            }

            Event::OxcCompiledElementStart(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    gen.handle_element_start(&ev, ctx);
                }
                SyntaxResult::keep(Event::OxcCompiledElementStart(ev))
            }
            Event::OxcCompiledElementClosed(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    gen.handle_element_closed(&ev, ctx);
                }
                SyntaxResult::keep(Event::OxcCompiledElementClosed(ev))
            }

            // Text, comment, interpolation events outside <template> are skipped.
            // These can arrive before CompiledTemplateStart or after CompiledTemplateEnd
            // when the stack is empty.
            Event::Comment(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    if gen.is_inside_template() {
                        gen.handle_comment(&ev, ctx);
                    }
                }
                SyntaxResult::keep(Event::Comment(ev))
            }
            Event::Text(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    if gen.is_inside_template() {
                        gen.handle_text(&ev, ctx);
                    }
                }
                SyntaxResult::keep(Event::Text(ev))
            }
            Event::OxcInterpolation(ev) => {
                if let Some(gen) = self.backend.as_backend() {
                    if gen.is_inside_template() {
                        gen.handle_interpolation(&ev, ctx);
                    }
                }
                SyntaxResult::keep(Event::OxcInterpolation(ev))
            }

            _ => SyntaxResult::keep(event),
        }
    }
}

#[cfg(test)]
mod tests;
