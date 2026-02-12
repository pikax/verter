//! Vapor template code generation (placeholder).
//!
//! Vapor mode uses direct DOM manipulation with reactivity wrappers instead of
//! the Virtual DOM approach. When `<template vapor>` is present, this backend
//! will be used instead of the VDOM backend.

use std::{cell::RefCell, rc::Rc};

use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, OxcCompiledElementClosed,
            OxcCompiledElementStart, OxcInterpolation, Text,
        },
    },
};

pub(crate) struct VaporTemplateGenerator<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
    bindings: FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
}

impl<'alloc> VaporTemplateGenerator<'alloc> {
    pub(crate) fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            bindings: FxHashMap::default(),
            is_production,
        }
    }

    pub(crate) fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.bindings = bindings;
    }

    pub(crate) fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    pub(crate) fn generate_source_map(&self) -> String {
        self.code_transform
            .borrow()
            .generate_map_json(Default::default())
    }

    pub(crate) fn is_inside_template(&self) -> bool {
        false // TODO: implement stack tracking
    }

    pub(crate) fn handle_template_start(
        &mut self,
        _ev: &CompiledRootTemplateStart,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // TODO: vapor template initialization
    }

    pub(crate) fn handle_template_closed(
        &mut self,
        _ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // TODO: vapor template finalization
    }

    pub(crate) fn handle_element_start(
        &mut self,
        _ev: &OxcCompiledElementStart<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // TODO: vapor element open
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        _ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // TODO: vapor element close
    }

    pub(crate) fn handle_comment(&mut self, _ev: &Comment, _ctx: &SyntaxPluginContext<'alloc>) {
        // TODO: vapor comment handling
    }

    pub(crate) fn handle_text(&mut self, _ev: &Text, _ctx: &SyntaxPluginContext<'alloc>) {
        // TODO: vapor text handling
    }

    pub(crate) fn handle_interpolation(
        &mut self,
        _ev: &OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // TODO: vapor interpolation handling
    }
}
