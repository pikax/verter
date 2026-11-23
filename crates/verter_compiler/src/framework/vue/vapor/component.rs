//! Component operations consume the snapshot's component classification.
use super::ops::{BlockKind, ClosedBlock, NameKind, Part, RunId};
use super::scope::VaporScope;
use super::slot_roots::flags_text;
use super::VaporCompiler;
use crate::framework::vue::semantic::VueComponentClass;
use crate::framework::vue::structure::{
    VueAttr, VueAttrKind, VueBuiltinDirective, VueCompileNodeId,
};
use crate::framework::vue::target::VueTargetRefusal;
use crate::template::binding::prefixed_expr::find_prop_oxc_exp;
use crate::template::code_gen::shared::helpers::{
    is_builtin_component, is_raw_children_builtin, VaporHelper,
};
use crate::template::oxc::types::Dynamism;
use crate::template::vue_syntax::{
    camelize, is_handler_reference, needs_quoted_key, to_pascal_case, trim_handler_body,
};

impl<'c> VaporCompiler<'c> {
    pub(super) fn build_component(
        &mut self,
        node: VueCompileNodeId,
        mut scope: VaporScope<'c>,
        destructured: bool,
    ) -> Result<(ClosedBlock, bool), VueTargetRefusal> {
        let inputs = self.inputs;
        let tag = inputs.structure.tag_name(node).expect("component tag");
        let template = self.inputs.structure.template_id(node);
        let class = self.inputs.semantic.component_class(template);
        let dynamic_attr = (class == VueComponentClass::Dynamic)
            .then(|| {
                inputs.structure.attrs(node).iter().find(|attr| {
                    attr.kind == VueAttrKind::Bind
                        && attr
                            .name
                            .is_some_and(|name| self.inputs.structure.text(name) == "is")
                })
            })
            .flatten();
        let dynamic = dynamic_attr.is_some();
        let node_ref = scope.ensure_node_ref(&mut self.state.counters);
        let reference = if let Some(attr) = dynamic_attr {
            self.attr_expression(node, attr)
        } else {
            let name = match class {
                VueComponentClass::Binding => {
                    let bound =
                        crate::compile::template_expr_overlay::component_tag_candidates(tag)
                            .into_iter()
                            .find(|candidate| self.resolver.get(candidate).is_some())
                            .expect("classified component binding");
                    let run = self.record_expression_with(
                        &bound,
                        0,
                        None,
                        super::expr::RecordOptions {
                            force_js: self.inputs.options.force_js,
                            resolution: super::expr::Resolution::Simple,
                            provenance: super::expr::ExprProvenance::Generated,
                        },
                    );
                    let reference = if bound != tag && tag.contains('.') {
                        let root = tag.split('.').next().expect("member root");
                        self.state.runs.commit(&mut vec![
                            Part::Run(run),
                            Part::Text(self.inputs.allocator.alloc_str(&tag[root.len()..])),
                        ])
                    } else {
                        run
                    };
                    return self.finish_component(
                        node,
                        scope,
                        node_ref,
                        reference,
                        false,
                        destructured,
                    );
                }
                VueComponentClass::BuiltIn => {
                    let pascal = to_pascal_case(tag);
                    match pascal.as_str() {
                        "Teleport" => {
                            self.state.use_helper(VaporHelper::VaporTeleport);
                            VaporHelper::VaporTeleport.name().to_owned()
                        }
                        "KeepAlive" => {
                            self.state.use_helper(VaporHelper::VaporKeepAlive);
                            VaporHelper::VaporKeepAlive.name().to_owned()
                        }
                        _ => {
                            let (flag, name) = is_builtin_component(tag)
                                .or_else(|| is_builtin_component(&pascal))
                                .expect("classified builtin");
                            self.state.builtins = self.state.builtins.add(flag);
                            name.to_owned()
                        }
                    }
                }
                _ => {
                    let base = format!("_component_{}", tag.replace(['-', '.'], "_"));
                    let quoted = self.quoted(tag);
                    self.state
                        .resolve_component(&base, quoted, self.inputs.allocator)
                        .to_owned()
                }
            };
            self.state.runs.commit(&mut vec![Part::Text(
                self.inputs.allocator.alloc_str(&name),
            )])
        };
        self.finish_component(node, scope, node_ref, reference, dynamic, destructured)
    }

    /// Whether this node's parent is a `<KeepAlive>`/`<Transition>`.
    fn parent_hosts_single_root_child(&self, node: VueCompileNodeId) -> bool {
        let topology = self.inputs.topology;
        topology
            .parent(self.inputs.structure.template_id(node))
            .is_some_and(|parent| {
                topology.kind(parent) == crate::template::topology::VueTemplateNodeKind::Component
                    && self
                        .inputs
                        .structure
                        .tag_name(self.inputs.structure.compile_id(parent))
                        .is_some_and(
                            crate::template::code_gen::shared::helpers::hosts_single_root_child,
                        )
            })
    }

    fn finish_component(
        &mut self,
        node: VueCompileNodeId,
        scope: VaporScope<'c>,
        node_ref: u32,
        reference: RunId,
        dynamic: bool,
        destructured: bool,
    ) -> Result<(ClosedBlock, bool), VueTargetRefusal> {
        // Hoisted event handlers precede the creation that reads them.
        let mut parts = Vec::new();
        let props = self.build_component_props(node, dynamic, false, &mut parts);
        let tag = self.inputs.structure.tag_name(node).expect("component tag");
        let slots = self.build_component_slots(
            node,
            scope,
            is_raw_children_builtin(tag),
            destructured,
            &mut parts,
        )?;
        parts.extend([
            Part::Text("const "),
            Part::Name(NameKind::Node, node_ref),
            Part::Text(if dynamic {
                " = _createDynamicComponent(() => ("
            } else {
                " = _createComponent("
            }),
            Part::Run(reference),
        ]);
        if dynamic {
            parts.push(Part::Text(")"));
        }
        parts.push(Part::Text(", "));
        parts.push(props.map_or(Part::Text("null"), Part::Run));
        parts.push(Part::Text(", "));
        parts.push(slots.map_or(Part::Text("null"), Part::Run));
        if dynamic {
            // Official `genDynamicComponentFlags`: the template's sole root is
            // SINGLE_ROOT; a component's sole slot child is SINGLE_ROOT under
            // a `<KeepAlive>`/`<Transition>` (which pass their single root
            // through) or SLOT_SINGLE_ROOT under any other component. A root
            // of a slot block with no stable root is SLOT_ROOT as well
            // (`markSlotRootOperations`).
            let sole_slot_root = self
                .element_stack
                .last()
                .is_some_and(|parent| parent.is_component_scope)
                && self.is_sole_meaningful_child(node);
            let root = if (self.template_single_root && self.depth == 0)
                || (sole_slot_root && self.parent_hosts_single_root_child(node))
            {
                Some((1, "SINGLE_ROOT"))
            } else if sole_slot_root {
                Some((32, "SLOT_SINGLE_ROOT"))
            } else {
                None
            };
            let is_production = self.inputs.options.is_production;
            let allocator = self.inputs.allocator;
            let argument = |slot_root: bool| {
                let (mut flags, mut names) =
                    root.map_or((0, Vec::new()), |(flags, name)| (flags, vec![name]));
                if slot_root {
                    flags |= 4;
                    names.push("SLOT_ROOT");
                }
                if flags == 0 {
                    return Vec::new();
                }
                let text = flags_text(flags, &names, is_production);
                vec![Part::Text(", "), Part::Text(allocator.alloc_str(&text))]
            };
            let (mut plain, mut slot_root) = (argument(false), argument(true));
            let flags = self
                .state
                .slot_root_flags(node_ref, &mut plain, &mut slot_root);
            parts.push(Part::Run(flags));
            self.state.use_helper(VaporHelper::CreateDynamicComponent);
        } else {
            parts.push(Part::Text(", true"));
            self.state.use_helper(VaporHelper::CreateComponent);
        }
        parts.push(Part::Text(")"));
        if let Some(statement) = self.template_ref_statement(node, node_ref) {
            parts.extend([Part::LineBreak, Part::Run(statement)]);
        }
        Ok((
            self.construct(
                node_ref,
                &mut parts,
                if dynamic {
                    BlockKind::Fragment
                } else {
                    BlockKind::Component
                },
            ),
            dynamic,
        ))
    }

    pub(super) fn attr_expression(&mut self, node: VueCompileNodeId, attr: &VueAttr) -> RunId {
        let inputs = self.inputs;
        let (_, oxc) = self.element_of(node);
        let source = inputs.source.text;
        if let Some(span) = attr.value_span {
            self.record_expression(
                &source[span.start as usize..span.end as usize],
                span.start,
                find_prop_oxc_exp(oxc, attr.prop_index as usize),
            )
        } else {
            let span = attr.arg_span.expect("shorthand argument");
            let value = camelize(&source[span.start as usize..span.end as usize]);
            self.record_expression_with(
                &value,
                span.start,
                None,
                super::expr::RecordOptions {
                    force_js: self.inputs.options.force_js,
                    resolution: super::expr::Resolution::Simple,
                    provenance: if matches!(value, std::borrow::Cow::Borrowed(_)) {
                        super::expr::ExprProvenance::Authored
                    } else {
                        super::expr::ExprProvenance::Generated
                    },
                },
            )
        }
    }

    fn quoted_component_prop(&self, value: &str) -> &'c str {
        let mut quoted = String::from("\"");
        crate::template::vue_syntax::escape_js_string_into_with_controls(&mut quoted, value, false);
        quoted.push('"');
        self.inputs.allocator.alloc_str(&quoted)
    }

    pub(super) fn build_component_props(
        &mut self,
        node: VueCompileNodeId,
        skip_is: bool,
        outlet: bool,
        handlers: &mut Vec<Part<'c>>,
    ) -> Option<RunId> {
        let (_, oxc) = self.element_of(node);
        let source = self.inputs.source.text;
        let mut parts = vec![Part::Text("{ ")];
        let mut count = 0;
        for attr in self.inputs.structure.attrs(node) {
            if matches!(attr.prop_index, VueAttr::CACHED_PROP) {
                continue;
            }
            let name = &source[attr.name_span.start as usize..attr.name_span.end as usize];
            let arg = attr
                .arg_span
                .map(|span| &source[span.start as usize..span.end as usize]);
            let static_attr = attr.kind == VueAttrKind::Static;
            if outlet && (arg == Some("name") || (static_attr && name == "name")) {
                continue;
            }
            if !static_attr && arg == Some("key") {
                continue;
            }
            if skip_is && arg == Some("is") {
                continue;
            }
            if attr.kind == VueAttrKind::Builtin(VueBuiltinDirective::Model) {
                if !outlet && attr.dynamic_arg {
                    self.refusal.get_or_insert(VueTargetRefusal::Unsupported {
                        code: "dynamic-component-model-argument",
                        span: attr.name_span,
                    });
                    continue;
                }
                if !outlet && attr.value_span.is_some() {
                    if count > 0 {
                        parts.push(Part::Text(", "));
                    }
                    count += 1;
                    self.push_component_model(node, attr, arg, &mut parts, handlers);
                }
                continue;
            }
            if !static_attr && arg.is_none() {
                continue;
            }
            if !outlet && attr.value_span.is_none() {
                continue;
            }
            if outlet && !static_attr && attr.kind != VueAttrKind::Bind {
                continue;
            }
            if count > 0 {
                parts.push(Part::Text(", "));
            }
            count += 1;
            let key = if static_attr {
                name.to_owned()
            } else if attr.kind == VueAttrKind::On {
                format!(
                    "on{}",
                    to_pascal_case(&arg.expect("event arg").replace(':', "-"))
                )
            } else if outlet {
                camelize(arg.expect("prop arg")).into_owned()
            } else {
                arg.expect("prop arg").to_owned()
            };
            parts.push(Part::Text(if needs_quoted_key(&key) {
                self.quoted(&key)
            } else {
                self.inputs.allocator.alloc_str(&key)
            }));
            parts.push(Part::Text(": "));
            if static_attr {
                parts.push(Part::Text(attr.value_span.map_or("true", |span| {
                    if outlet {
                        self.quoted(&source[span.start as usize..span.end as usize])
                    } else {
                        self.quoted_component_prop(&source[span.start as usize..span.end as usize])
                    }
                })));
            } else {
                let parsed = find_prop_oxc_exp(oxc, attr.prop_index as usize);
                let raw = attr
                    .value_span
                    .map_or("", |span| &source[span.start as usize..span.end as usize]);
                if attr.kind == VueAttrKind::On {
                    // A component event prop is a getter that returns the
                    // handler. A handler reference is returned as read; any
                    // other value is written once as a handler local, as an
                    // element listener's handler is, and returned by name.
                    let start = attr.value_span.expect("event value").start;
                    let raw = trim_handler_body(raw);
                    if raw.is_empty() {
                        parts.push(Part::Text("() => {}"));
                    } else if is_handler_reference(raw, parsed) {
                        parts.push(Part::Text("() => ("));
                        parts.push(Part::Run(self.record_expression(raw, start, parsed)));
                        parts.push(Part::Text(")"));
                    } else {
                        let handler = self.record_handler(raw, start, parsed, &[], &[]);
                        let local =
                            self.hoist_handler(node, arg.expect("event arg"), handler, handlers);
                        parts.extend([Part::Text("() => "), Part::Text(local)]);
                    }
                } else {
                    let constant = if outlet {
                        is_direct_constant_expr(raw.trim())
                    } else {
                        parsed.is_some_and(|e| {
                            e.dynamism == Dynamism::Static
                                && e.bindings.as_ref().is_some_and(|bindings| {
                                    bindings.non_ignored_binding_names().is_empty()
                                })
                        })
                    };
                    if !constant {
                        parts.push(Part::Text("() => ("));
                    }
                    parts.push(Part::Run(self.attr_expression(node, attr)));
                    if !constant {
                        parts.push(Part::Text(")"));
                    }
                }
            }
        }
        parts.push(Part::Text(" }"));
        (count > 0).then(|| self.state.runs.commit(&mut parts))
    }
}

impl<'c> VaporCompiler<'c> {
    /// Declare a component listener once, before the creation that reads it,
    /// under a render-unique local. The local must not shadow a name the
    /// component's props already see: an enclosing slot or loop parameter,
    /// the component's own loop aliases, or a script binding.
    fn hoist_handler(
        &mut self,
        node: VueCompileNodeId,
        event: &str,
        handler: RunId,
        handlers: &mut Vec<Part<'c>>,
    ) -> &'c str {
        let (_, oxc) = self.element_of(node);
        let expressions = self.inputs.semantic.expressions();
        let scope = oxc.map_or_else(
            || {
                self.inputs
                    .semantic
                    .scope_of(self.inputs.structure.template_id(node))
            },
            |element| element.props_scope,
        );
        let resolver = &self.resolver;
        let local = self.state.handler_name(event, |name| {
            expressions.scopes.declares(scope, name) || resolver.get(name).is_some()
        });
        let local = self.inputs.allocator.alloc_str(&local);
        handlers.extend([
            Part::Text("const "),
            Part::Text(local),
            Part::Text(" = "),
            Part::Run(handler),
            Part::LineBreak,
        ]);
        local
    }

    /// A component `v-model`: the model prop as a getter, its `onUpdate:`
    /// listener writing the bound expression, and its modifiers, as Vue's
    /// component `v-model` transform passes them.
    fn push_component_model(
        &mut self,
        node: VueCompileNodeId,
        attr: &VueAttr,
        arg: Option<&'c str>,
        parts: &mut Vec<Part<'c>>,
        handlers: &mut Vec<Part<'c>>,
    ) {
        let (element, oxc) = self.element_of(node);
        let source = self.inputs.source.text;
        let span = attr.value_span.expect("model value");
        let value = self.record_expression(
            &source[span.start as usize..span.end as usize],
            span.start,
            find_prop_oxc_exp(oxc, attr.prop_index as usize),
        );
        let prop = arg.unwrap_or("modelValue");
        let handler = self.state.runs.commit(&mut vec![
            Part::Text("_value => ("),
            Part::Run(value),
            Part::Text(" = _value)"),
        ]);
        let local = self.hoist_handler(node, &format!("update:{prop}"), handler, handlers);
        let key = |name: String| -> &'c str {
            if needs_quoted_key(&name) {
                self.quoted(&name)
            } else {
                self.inputs.allocator.alloc_str(&name)
            }
        };
        parts.extend([
            Part::Text(key(prop.to_owned())),
            Part::Text(": () => ("),
            Part::Run(value),
            Part::Text("), "),
            Part::Text(key(format!("onUpdate:{prop}"))),
            Part::Text(": () => "),
            Part::Text(local),
        ]);
        let modifiers: Vec<_> = element.props[attr.prop_index as usize]
            .modifiers
            .iter()
            .map(|modifier| &source[modifier.start as usize..modifier.end as usize])
            .collect();
        if modifiers.is_empty() {
            return;
        }
        parts.extend([
            Part::Text(", "),
            Part::Text(key(format!("{}Modifiers", arg.unwrap_or("model")))),
            Part::Text(": { "),
        ]);
        for (position, modifier) in modifiers.into_iter().enumerate() {
            if position > 0 {
                parts.push(Part::Text(", "));
            }
            parts.extend([Part::Text(key(modifier.to_owned())), Part::Text(": true")]);
        }
        parts.push(Part::Text(" }"));
    }
}

fn is_direct_constant_expr(expr: &str) -> bool {
    matches!(expr, "true" | "false" | "null" | "undefined")
        || is_string_literal_text(expr)
        || is_numeric_literal_text(expr)
}
fn is_string_literal_text(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    bytes.len() >= 2
        && (bytes[0] == b'"' || bytes[0] == b'\'')
        && bytes[bytes.len() - 1] == bytes[0]
        && !bytes[1..bytes.len() - 1].contains(&bytes[0])
}
fn is_numeric_literal_text(expr: &str) -> bool {
    !expr.is_empty()
        && expr.bytes().any(|b| b.is_ascii_digit())
        && expr
            .bytes()
            .enumerate()
            .all(|(i, b)| b.is_ascii_digit() || b == b'.' || (i == 0 && b == b'-'))
}
