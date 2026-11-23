//! Slot bodies close the same operation rows as element and branch bodies.
use super::control_flow::{ConstructRefs, LoopParts};
use super::ops::{BlockKind, ClosedBlock, NameKind, Part, RunId, Stmt};
use super::scope::{MergedConstructKind, NamedSlot, VaporScope};
use super::slot_roots::{merged_root, SlotRoot};
use super::VaporCompiler;
use crate::ast::types::ChildrenFlags;
use crate::ast::types::ElementNodeConditionKind;
use crate::framework::vue::structure::{VueAttrKind, VueCompileNodeId};
use crate::framework::vue::target::VueTargetRefusal;
use crate::template::code_gen::shared::helpers::VaporHelper;

/// The flags runtime-vapor 3.6.0-rc.15 reads from a slot function whose
/// roots may render nothing: `shouldUseSlotFragment` checks `slot._ === 1`.
const NON_STABLE_SLOT: &str = "{ _: 1 /* NON_STABLE */ }";

/// A closed slot function body and whether it has no stable root.
#[derive(Clone, Copy)]
struct SlotBody {
    run: RunId,
    non_stable: bool,
}

impl<'c> VaporCompiler<'c> {
    pub(super) fn build_component_slots(
        &mut self,
        node: VueCompileNodeId,
        mut scope: VaporScope<'c>,
        raw: bool,
        destructured: bool,
        statements: &mut Vec<Part<'c>>,
    ) -> Result<Option<RunId>, VueTargetRefusal> {
        let mut named = std::mem::take(&mut scope.named_slots);
        // Vue's `genRawSlots` keeps static and dynamically named slots in
        // source order and appends conditional and looped slots after them;
        // the runtime resolves the dynamic list from its end.
        named.sort_by_key(|slot| matches!(slot, NamedSlot::Conditional(_) | NamedSlot::Loop(_)));
        for cache in std::mem::take(&mut scope.slot_caches) {
            statements.extend([Part::Text("let "), Part::Text(cache), Part::LineBreak]);
        }
        if named.is_empty() {
            if let Some(run) = self.forwarded_slot(&scope)? {
                return Ok(Some(run));
            }
        }
        let has_content = !scope.html.is_empty()
            || !scope.roots.is_empty()
            || !scope.child_nav.is_empty()
            || !scope.child_effects.is_empty()
            || !scope.child_statements.is_empty()
            || !scope.child_text_creations.is_empty();
        if named.is_empty() && !has_content {
            return Ok(None);
        }
        let (element, _) = self.element_of(node);
        let dynamic = element.children_flag.has(ChildrenFlags::HasInterpolation);
        // A `v-slot` on the component itself declares the default slot's props.
        let params = self
            .slot_parameters(node, destructured)
            .unwrap_or(Part::Text(""));
        let mut parts = Vec::new();
        if !named.is_empty() {
            parts.push(Part::Text("{ "));
            // Statically named slots before the first dynamic one stay in the
            // slots object; from there on, every slot is an element of its
            // dynamic list `$`, in source order, as Vue's `genDynamicSlots`
            // writes it.
            let first_dynamic = named
                .iter()
                .position(|slot| !matches!(slot, NamedSlot::Static(_)))
                .unwrap_or(named.len());
            for (index, slot) in named[..first_dynamic].iter().enumerate() {
                if index > 0 {
                    parts.push(Part::Text(", "));
                }
                if let NamedSlot::Static(entry) = slot {
                    parts.push(Part::Run(*entry));
                }
            }
            if first_dynamic < named.len() {
                if first_dynamic > 0 {
                    parts.push(Part::Text(", "));
                }
                parts.push(Part::Text("$: ["));
                let mut open_object = false;
                for (index, slot) in named[first_dynamic..].iter().enumerate() {
                    match slot {
                        NamedSlot::Static(entry) => {
                            parts.push(Part::Text(match (open_object, index) {
                                (true, _) => ", ",
                                (false, 0) => "{ ",
                                (false, _) => ", { ",
                            }));
                            open_object = true;
                            parts.push(Part::Run(*entry));
                            continue;
                        }
                        _ if open_object => {
                            parts.push(Part::Text(" }, "));
                            open_object = false;
                        }
                        _ if index > 0 => parts.push(Part::Text(", ")),
                        _ => {}
                    }
                    match slot {
                        NamedSlot::Static(_) => unreachable!("written above"),
                        NamedSlot::Dynamic(entry) | NamedSlot::Loop(entry) => {
                            parts.push(Part::Run(*entry))
                        }
                        NamedSlot::Conditional(arms) => {
                            parts.push(Part::Text("() => ("));
                            for (condition, object) in arms {
                                match condition {
                                    Some(condition) => parts.extend([
                                        Part::Text("("),
                                        Part::Run(*condition),
                                        Part::Text(") ? "),
                                        Part::Run(*object),
                                        Part::Text(" : "),
                                    ]),
                                    None => parts.push(Part::Run(*object)),
                                }
                            }
                            if arms
                                .last()
                                .is_some_and(|(condition, _)| condition.is_some())
                            {
                                parts.push(Part::Text("void 0"));
                            }
                            parts.push(Part::Text(")"));
                        }
                    }
                }
                if open_object {
                    parts.push(Part::Text(" }"));
                }
                parts.push(Part::Text("]"));
            }
            if has_content {
                let body = self.slot_body(scope, dynamic, "      ", node)?;
                let function = self.slot_function(params, body, "    }");
                parts.extend([Part::Text(", \"default\": "), Part::Run(function)]);
            }
            parts.push(Part::Text(" }"));
        } else if raw {
            let body = self.slot_body(scope, dynamic, "    ", node)?;
            let function = self.slot_function(params, body, "}");
            parts.push(Part::Run(function));
        } else {
            let body = self.slot_body(scope, dynamic, "      ", node)?;
            let function = self.slot_function(params, body, "    }");
            parts.extend([
                Part::Text("{ default: "),
                Part::Run(function),
                Part::Text(", _: 2 }"),
            ]);
        }
        Ok(Some(self.state.runs.commit(&mut parts)))
    }

    /// A slot function's body. Content with no template of its own (only
    /// components, outlets, conditionals and loops) has no container: each
    /// construct is a root of the slot block and is returned as created, as
    /// Vue returns block roots. Any other content closes as a block.
    ///
    /// A body whose roots are all conditionals, loops, outlets or dynamic
    /// components has no stable root, so its function is marked NON_STABLE,
    /// as Vue's `markSlotRootOperations` decides; the runtime gives such a
    /// slot a fragment that can fall back when it renders nothing. Its roots
    /// are marked then, so each notifies the slot boundary when what it
    /// renders changes.
    fn slot_body(
        &mut self,
        scope: VaporScope<'c>,
        dynamic: bool,
        indent: &'c str,
        node: VueCompileNodeId,
    ) -> Result<SlotBody, VueTargetRefusal> {
        if scope.is_transparent_wrapper {
            // Several roots: each closed its own template as a root of the
            // slot block. A component, or a template with content other than
            // comments (an element or a text run), is a stable root.
            let roots: Vec<_> = scope
                .roots
                .iter()
                .map(|root| self.state.slot_root(root))
                .collect();
            let non_stable = !self.state.mark_slot_roots(&roots);
            let body = self.close_roots(scope, self.span_of(node))?;
            return Ok(SlotBody {
                run: body.run,
                non_stable,
            });
        }
        let constructs_only = scope.html.is_empty()
            && !scope.merged_construct_kinds.is_empty()
            && scope.child_nav.is_empty()
            && scope.child_effects.is_empty()
            && scope.own_effects.is_empty()
            && scope.child_text_creations.is_empty()
            && scope.text_node_ref.is_none();
        if !constructs_only {
            return Ok(SlotBody {
                run: self
                    .close_block(scope, dynamic, indent, false, true, self.span_of(node))?
                    .run,
                non_stable: false,
            });
        }
        let roots: Vec<_> = scope
            .merged_construct_kinds
            .iter()
            .map(|(kind, node_ref)| merged_root(*kind, *node_ref))
            .collect();
        let non_stable = !self.state.mark_slot_roots(&roots);
        let mut parts = Vec::new();
        for statement in &scope.child_statements {
            // Placement rows only position a construct inside a container.
            if let Stmt::Run(run) = statement {
                parts.extend([
                    Part::Text(indent),
                    Part::Indented(*run, indent),
                    Part::Text("\n"),
                ]);
            }
        }
        parts.extend([Part::Text(indent), Part::Text("return ")]);
        match scope.merged_construct_kinds.as_slice() {
            [(_, root)] => parts.push(Part::Name(NameKind::Node, *root)),
            roots => {
                parts.push(Part::Text("["));
                for (position, (_, root)) in roots.iter().enumerate() {
                    if position > 0 {
                        parts.push(Part::Text(", "));
                    }
                    parts.push(Part::Name(NameKind::Node, *root));
                }
                parts.push(Part::Text("]"));
            }
        }
        parts.push(Part::Text("\n"));
        self.recycle(scope);
        Ok(SlotBody {
            run: self.state.runs.commit(&mut parts),
            non_stable,
        })
    }

    /// `(params) => { body }`, wrapped in `_extend(fn, { _: 1 })` when
    /// the body has no stable root.
    fn slot_function(&mut self, params: Part<'c>, body: SlotBody, close: &'c str) -> RunId {
        let mut parts = Vec::new();
        if body.non_stable {
            parts.push(Part::Text("_extend("));
            self.state.use_helper(VaporHelper::Extend);
        }
        parts.extend([
            Part::Text("("),
            params,
            Part::Text(") => {\n"),
            Part::Run(body.run),
            Part::Text(close),
        ]);
        if body.non_stable {
            parts.extend([
                Part::Text(", "),
                Part::Text(NON_STABLE_SLOT),
                Part::Text(")"),
            ]);
        }
        self.state.runs.commit(&mut parts)
    }

    fn forwarded_slot(
        &mut self,
        scope: &VaporScope<'c>,
    ) -> Result<Option<RunId>, VueTargetRefusal> {
        if scope.is_transparent_wrapper
            || !scope.html.is_empty()
            || scope.merged_construct_kinds.len() != 1
            || !scope.child_nav.is_empty()
            || !scope.child_effects.is_empty()
            || !scope.child_text_creations.is_empty()
        {
            return Ok(None);
        }
        let (kind, node_ref) = scope.merged_construct_kinds[0];
        if !matches!(
            kind,
            MergedConstructKind::SlotOutlet | MergedConstructKind::DynamicComponent
        ) {
            return Ok(None);
        }
        // The forwarded construct is the slot block's only root, never a
        // stable one.
        self.state.mark_slot_roots(&[SlotRoot {
            node_ref,
            stable: false,
        }]);
        let mut parts = vec![Part::Text("_extend(() => {\n")];
        for statement in &scope.child_statements {
            match statement {
                // This scope is the forwarded closure, so it consumes the
                // container reservation instead of creating a DOM container.
                Stmt::Reserved => {}
                Stmt::Run(run) => {
                    parts.extend([Part::Text("    "), Part::Run(*run), Part::Text("\n")])
                }
                Stmt::InsertionState { .. } => {
                    return Err(VueTargetRefusal::Unsupported {
                        code: "forwarded-slot-container",
                        span: verter_span::Span::new(0, 0),
                    })
                }
            }
        }
        parts.extend([
            Part::Text("    return "),
            Part::Name(NameKind::Node, node_ref),
            Part::Text("\n  }, "),
            Part::Text(NON_STABLE_SLOT),
            Part::Text(")"),
        ]);
        self.state.use_helper(VaporHelper::Extend);
        Ok(Some(self.state.runs.commit(&mut parts)))
    }

    /// Close a slot template into its component's slots. A statically named
    /// slot is an entry of the slots object; a dynamically named, looped or
    /// conditional one is an element of the dynamic slot list, as Vue's
    /// `genDynamicSlot` writes it. The slot function is closed inside the
    /// template's own loop frame, and the loop's source, name and key getters
    /// outside it.
    pub(super) fn close_named_slot(
        &mut self,
        node: VueCompileNodeId,
        scope: VaporScope<'c>,
        destructured: bool,
        refs: ConstructRefs,
    ) -> Result<(), VueTargetRefusal> {
        let (element, oxc) = self.element_of(node);
        let slot = element.v_slot.as_ref().expect("named slot");
        let source = self.inputs.source.text;
        let params = self
            .slot_parameters(node, destructured)
            .expect("named slot");
        let body = self.slot_body(
            scope,
            element.children_flag.has(ChildrenFlags::HasInterpolation),
            "      ",
            node,
        )?;
        let dynamic_entry = refs.for_loop.is_some()
            || element.v_condition.is_some()
            || slot
                .arg_start
                .is_some_and(|start| source.as_bytes()[start as usize] == b'[');
        let function = self.slot_function(
            params,
            SlotBody {
                non_stable: body.non_stable && !dynamic_entry,
                ..body
            },
            "    }",
        );
        self.leave_for_scope(refs);
        let dynamic_name = oxc
            .and_then(|oxc| oxc.v_slot.as_ref())
            .and_then(|slot| slot.dynamic_name.as_ref());
        let name = match (slot.arg_start.zip(slot.arg_end), dynamic_name) {
            (Some((start, end)), Some(parsed)) if end >= start + 2 => {
                let text = &source[start as usize + 1..end as usize - 1];
                Err(if refs.for_loop.is_some() {
                    self.record_with_authored_aliases(node, text, start + 1, Some(parsed))
                } else {
                    self.record_expression(text, start + 1, Some(parsed))
                })
            }
            (Some((start, end)), _) => Ok(&source[start as usize..end as usize]),
            (None, _) => Ok("default"),
        };
        let condition = element.v_condition.as_ref();
        let entry = if refs.for_loop.is_some() {
            if condition.is_some() {
                return Err(VueTargetRefusal::Unsupported {
                    code: "conditional-looped-slot",
                    span: self.span_of(node),
                });
            }
            let LoopParts {
                source: items,
                callback,
                aliases,
                key,
            } = self.loop_parts(node);
            let name = match name {
                Ok(name) => self
                    .state
                    .runs
                    .commit(&mut vec![Part::Text(self.quoted(name))]),
                Err(run) => run,
            };
            let mut parts = vec![
                Part::Text("_createForSlots(() => ("),
                Part::Run(items),
                Part::Text("), ("),
                Part::Text(callback),
                Part::Text(") => "),
                Part::Run(function),
                Part::Text(", ("),
                Part::Text(aliases),
                Part::Text(") => ("),
                Part::Run(name),
                Part::Text(")"),
            ];
            if let Some(key) = key {
                parts.extend([
                    Part::Text(", ("),
                    Part::Text(aliases),
                    Part::Text(") => ("),
                    Part::Run(key),
                    Part::Text(")"),
                ]);
            }
            parts.push(Part::Text(")"));
            self.state.use_helper(VaporHelper::CreateForSlots);
            NamedSlot::Loop(self.state.runs.commit(&mut parts))
        } else if condition.is_none() && name.is_ok() {
            let name = name.expect("static slot name");
            NamedSlot::Static(self.state.runs.commit(&mut vec![
                Part::Text(self.quoted(name)),
                Part::Text(": "),
                Part::Run(function),
            ]))
        } else {
            // The function is kept across evaluations of the name or
            // condition, so an unchanged slot is not recreated.
            let cache = self.state.slot_cache_name(self.inputs.allocator);
            let name = match name {
                Ok(name) => Part::Text(self.quoted(name)),
                Err(run) => Part::Run(run),
            };
            let object = self.state.runs.commit(&mut vec![
                Part::Text("{ name: "),
                name,
                Part::Text(", fn: "),
                Part::Text(cache),
                Part::Text(" || ("),
                Part::Text(cache),
                Part::Text(" = "),
                Part::Run(function),
                Part::Text(") }"),
            ]);
            if let Some(parent) = self.element_stack.last_mut() {
                parent.slot_caches.push(cache);
            }
            match condition {
                None => NamedSlot::Dynamic(self.state.runs.commit(&mut vec![
                    Part::Text("() => ("),
                    Part::Run(object),
                    Part::Text(")"),
                ])),
                Some(condition) => {
                    let test = match (
                        &condition.kind,
                        condition.prop.value_start.zip(condition.prop.value_end),
                    ) {
                        (ElementNodeConditionKind::Else, _) => None,
                        (_, Some((start, end))) => Some(self.record_expression(
                            &source[start as usize..end as usize],
                            start,
                            oxc.and_then(|oxc| oxc.condition.as_ref()),
                        )),
                        (_, None) => Some(self.state.runs.commit(&mut vec![Part::Text("true")])),
                    };
                    let continues = !matches!(condition.kind, ElementNodeConditionKind::If);
                    let parent = self.element_stack.last_mut();
                    match parent.and_then(|parent| parent.named_slots.last_mut()) {
                        Some(NamedSlot::Conditional(arms))
                            if continues && arms.last().is_some_and(|(test, _)| test.is_some()) =>
                        {
                            arms.push((test, object));
                            return Ok(());
                        }
                        _ => NamedSlot::Conditional(vec![(test, object)]),
                    }
                }
            }
        };
        if let Some(parent) = self.element_stack.last_mut() {
            parent.named_slots.push(entry);
        }
        Ok(())
    }

    /// The parameter list of the slot function an element's `v-slot`
    /// declares: a destructured pattern reads through `_slotProps<depth>`,
    /// any other pattern is written as authored. `None` without a `v-slot`.
    fn slot_parameters(&mut self, node: VueCompileNodeId, destructured: bool) -> Option<Part<'c>> {
        let (element, oxc) = self.element_of(node);
        let slot = element.v_slot.as_ref()?;
        if destructured {
            return Some(Part::Text(
                self.inputs
                    .allocator
                    .alloc_str(&format!("_slotProps{}", self.for_scope_depth)),
            ));
        }
        let source = self.inputs.source.text;
        let params = slot
            .value_start
            .zip(slot.value_end)
            .map_or("", |(start, end)| &source[start as usize..end as usize]);
        let parameters = oxc.and_then(|oxc| oxc.v_slot.as_ref()).and_then(|slot| {
            slot.parsed
                .params()
                .map(|params| (params, slot.parsed.offset()))
        });
        Some(Part::Run(self.record_syntax(
            params,
            slot.value_start.unwrap_or(0),
            super::expr::Syntax::Parameters(parameters),
            super::expr::RecordOptions {
                force_js: self.inputs.options.force_js,
                resolution: super::expr::Resolution::Verbatim,
                provenance: super::expr::ExprProvenance::Authored,
            },
        )))
    }

    pub(super) fn build_slot_outlet(
        &mut self,
        node: VueCompileNodeId,
        scope: VaporScope<'c>,
        node_ref: u32,
    ) -> Result<ClosedBlock, VueTargetRefusal> {
        let source = self.inputs.source.text;
        let name_attr = self.inputs.structure.attrs(node).iter().find(|attr| {
            attr.kind == VueAttrKind::Static
                && attr
                    .name
                    .is_some_and(|name| self.inputs.structure.text(name) == "name")
                && attr.value_span.is_some()
        });
        let name = name_attr
            .and_then(|attr| attr.value_span)
            .map_or("default", |span| {
                &source[span.start as usize..span.end as usize]
            });
        // The fallback's children are its block roots, as Vue returns them.
        let fallback = if scope.is_transparent_wrapper && !scope.roots.is_empty() {
            // Inside a slot block a fallback stands in for the owning slot's
            // content, so Vue's `genSlotOutlet` marks its roots as a slot
            // block's.
            if self
                .element_stack
                .iter()
                .any(|open| open.is_component_scope)
            {
                let roots: Vec<_> = scope
                    .roots
                    .iter()
                    .map(|root| self.state.slot_root(root))
                    .collect();
                self.state.mark_slot_roots(&roots);
            }
            let body = self.close_roots(scope, self.span_of(node))?;
            Some(self.state.runs.commit(&mut vec![
                Part::Text("() => {\n"),
                Part::Run(body.run),
                Part::Text("  }"),
            ]))
        } else {
            None
        };
        // An outlet forwards no listeners, so no handler is hoisted.
        let props = self.build_component_props(node, false, true, &mut Vec::new());
        let quote = name_attr.map_or(Part::Text("\""), |attr| {
            Part::Anchored("\"", attr.name_span.start)
        });
        let name_text = Part::Text(self.inputs.allocator.alloc_str(name));
        let is_production = self.inputs.options.is_production;
        // A root of a slot block with no stable root is FORWARDED
        // (`markSlotRootOperations`); trailing default arguments are omitted
        // only without flags.
        let arguments = |forwarded: bool| {
            let mut parts = Vec::new();
            if fallback.is_some() || props.is_some() || name != "default" || forwarded {
                parts.extend([quote, name_text, Part::Text("\"")]);
            }
            if fallback.is_some() || props.is_some() || forwarded {
                parts.push(Part::Text(", "));
                parts.push(props.map_or(Part::Text("null"), Part::Run));
            }
            if let Some(fallback) = fallback {
                parts.extend([Part::Text(", "), Part::Run(fallback)]);
            } else if forwarded {
                parts.push(Part::Text(", null"));
            }
            if forwarded {
                parts.push(Part::Text(if is_production {
                    ", 4"
                } else {
                    ", 4 /* FORWARDED */"
                }));
            }
            parts
        };
        let (mut plain, mut forwarded) = (arguments(false), arguments(true));
        let arguments = self
            .state
            .slot_root_flags(node_ref, &mut plain, &mut forwarded);
        let mut parts = vec![
            Part::Text("const "),
            Part::Name(NameKind::Node, node_ref),
            Part::Text(" = _createSlot("),
            Part::Run(arguments),
            Part::Text(")"),
        ];
        self.state.use_helper(VaporHelper::CreateSlot);
        Ok(self.construct(node_ref, &mut parts, BlockKind::Fragment))
    }
}
