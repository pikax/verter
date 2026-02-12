//! Vapor prop processing: classifies static vs dynamic props and generates
//! effects/statements for each prop kind.

use crate::syntax_kai::{
    plugin::SyntaxPluginContext,
    plugins::code_gen::{
        template::shared::helper::apply_dynamic_arg_prefix,
        types::{TemplateCodeGenError, TemplateCodeGenResult, VaporImportDependencies},
    },
    types::{OxcCompiledElementStart, PropKind},
};

use super::super::shared::helper::{classify_modifier, ModifierKind};
use super::helpers::is_member_expression;
use super::types::{VaporEffect, VaporTextPart};
use super::VaporTemplateGenerator;

impl<'alloc> VaporTemplateGenerator<'alloc> {
    /// Process all props on the current top-of-stack element.
    pub(super) fn process_props(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let node_ref = self
            .stack
            .last()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "process_props called with empty stack",
            ))?
            .node_ref;

        for oxc_prop in &ev.props {
            let prop = &oxc_prop.event;
            match prop.kind {
                PropKind::ClassValue | PropKind::StyleValue => {
                    // Static props are already in the HTML.
                }

                PropKind::Value => {
                    // Static props are already in the HTML.
                    // Detect ref="..." — remove from HTML and emit _setTemplateRef.
                    let attr_name = &ctx.input[prop.start as usize..prop.name_end as usize];
                    if attr_name == "ref" {
                        if let Some(ref val) = prop.value {
                            let ref_name = &ctx.input[val.start as usize..val.end as usize];
                            self.has_template_ref = true;
                            self.imports
                                .add(VaporImportDependencies::CREATE_TEMPLATE_REF_SETTER);
                            let stmt = format!("_setTemplateRef(n{}, \"{}\")", node_ref, ref_name);
                            self.stack
                                .last_mut()
                                .ok_or(TemplateCodeGenError::StackUnderflow(
                                    "process_props: stack empty after ref check",
                                ))?
                                .statements
                                .push(stmt);
                        }
                    }
                }

                PropKind::ClassBind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::SET_CLASS);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for ClassBind",
                            ))?
                            .effects
                            .push(VaporEffect::SetClass {
                                node_ref,
                                expr: prefixed,
                            });
                    }
                }

                PropKind::StyleBind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::SET_STYLE);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for StyleBind",
                            ))?
                            .effects
                            .push(VaporEffect::SetStyle {
                                node_ref,
                                expr: prefixed,
                            });
                    }
                }

                PropKind::Bind => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);

                        if prop.has_dynamic_arg {
                            // :[attrName]="value" → _setDynamicProps(n{X}, [{ [expr]: value }])
                            let arg_span = prop.arg.ok_or(TemplateCodeGenError::MissingArg(
                                "dynamic Bind prop must have arg span",
                            ))?;
                            let arg_raw =
                                &ctx.input[arg_span.start as usize..arg_span.end as usize];
                            let arg_prefixed = apply_dynamic_arg_prefix(
                                arg_raw,
                                arg_span.start,
                                &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                                &self.bindings,
                                self.is_production,
                            );
                            self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                            let dynamic_expr = format!("{{ [{}]: {} }}", arg_prefixed, prefixed);
                            self.stack
                                .last_mut()
                                .ok_or(TemplateCodeGenError::StackUnderflow(
                                    "process_props: stack empty for dynamic Bind",
                                ))?
                                .effects
                                .push(VaporEffect::SetDynamicProps {
                                    node_ref,
                                    expr: dynamic_expr,
                                });
                        } else {
                            let attr_name = if let Some(ref arg) = prop.arg {
                                ctx.input[arg.start as usize..arg.end as usize].to_string()
                            } else {
                                String::new()
                            };

                            self.imports.add(VaporImportDependencies::SET_PROP);
                            self.stack
                                .last_mut()
                                .ok_or(TemplateCodeGenError::StackUnderflow(
                                    "process_props: stack empty for Bind",
                                ))?
                                .effects
                                .push(VaporEffect::SetProp {
                                    node_ref,
                                    attr: attr_name,
                                    expr: prefixed,
                                });
                        }
                    }
                }

                PropKind::BindSpread => {
                    // v-bind="obj" → _setDynamicProps(n{X}, [expr])
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for BindSpread",
                            ))?
                            .effects
                            .push(VaporEffect::SetDynamicProps {
                                node_ref,
                                expr: prefixed,
                            });
                    }
                }

                PropKind::On => {
                    self.process_event(oxc_prop, ctx, node_ref)?;
                }

                PropKind::OnSpread => {
                    // v-on="obj" → _toHandlers(expr, true) wrapped in _setDynamicProps
                    // Matches Vue's compiler behavior: spread event handlers via _toHandlers.
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::SET_DYNAMIC_PROPS);
                        self.imports.add(VaporImportDependencies::TO_HANDLERS);
                        let dynamic_expr = format!("_toHandlers({})", prefixed);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for OnSpread",
                            ))?
                            .effects
                            .push(VaporEffect::SetDynamicProps {
                                node_ref,
                                expr: dynamic_expr,
                            });
                    }
                }

                PropKind::Html => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::SET_HTML);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for Html",
                            ))?
                            .effects
                            .push(VaporEffect::SetHtml {
                                node_ref,
                                expr: prefixed,
                            });
                    }
                }

                PropKind::Text => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::TO_DISPLAY_STRING);
                        let display_expr = format!("_toDisplayString({})", prefixed);

                        let state =
                            self.stack
                                .last_mut()
                                .ok_or(TemplateCodeGenError::StackUnderflow(
                                    "process_props: stack empty for Text",
                                ))?;
                        state.has_dynamic_children = true;
                        state.text_parts.push(VaporTextPart::Dynamic(display_expr));
                        if state.text_node_ref.is_none() {
                            let text_ref = self.counters.text_node;
                            self.counters.text_node += 1;
                            state.text_node_ref = Some(text_ref);
                        }
                    }
                }

                PropKind::Show => {
                    if let Some(ref exp) = oxc_prop.exp {
                        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
                        self.imports.add(VaporImportDependencies::APPLY_V_SHOW);
                        let stmt = format!("_applyVShow(n{}, () => ({}))", node_ref, prefixed);
                        self.stack
                            .last_mut()
                            .ok_or(TemplateCodeGenError::StackUnderflow(
                                "process_props: stack empty for Show",
                            ))?
                            .statements
                            .push(stmt);
                    }
                }

                PropKind::Model => {
                    self.process_model(oxc_prop, ev, ctx, node_ref)?;
                }

                PropKind::Directive => {
                    self.process_directive(oxc_prop, ctx, node_ref)?;
                }

                _ => {
                    // Structural directives (If/ElseIf/Else/For/Slot/Once)
                    // not yet handled in vapor mode.
                }
            }
        }

        Ok(())
    }

    pub(super) fn process_event(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) -> TemplateCodeGenResult {
        let prop = &oxc_prop.event;

        let is_dynamic = prop.has_dynamic_arg;

        let event_name = if let Some(ref arg) = prop.arg {
            ctx.input[arg.start as usize..arg.end as usize].to_string()
        } else {
            return Ok(());
        };

        let handler_expr = if let Some(ref exp) = oxc_prop.exp {
            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
            let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);

            let trimmed = prefixed.trim();
            if is_member_expression(trimmed) {
                format!("e => {}(e)", trimmed)
            } else if trimmed.contains("$event") {
                format!("$event => ({})", trimmed)
            } else {
                format!("() => ({})", trimmed)
            }
        } else {
            return Ok(());
        };

        self.imports.add(VaporImportDependencies::CREATE_INVOKER);

        let modifier_names: Vec<String> = if let Some(ref mods) = prop.modifiers {
            mods.iter()
                .map(|m| ctx.input[m.start as usize..m.end as usize].to_string())
                .collect()
        } else {
            Vec::new()
        };

        // Classify modifiers into three categories using shared classifier.
        let mut runtime_mods: Vec<&String> = Vec::new();
        let mut key_mods: Vec<&String> = Vec::new();
        let mut listener_opts: Vec<&String> = Vec::new();

        for m in &modifier_names {
            match classify_modifier(m) {
                ModifierKind::ListenerOption => listener_opts.push(m),
                ModifierKind::KeyFilter => key_mods.push(m),
                ModifierKind::Runtime => runtime_mods.push(m),
            }
        }

        // Build handler: runtime modifiers first, then key modifiers.
        let mut wrapped = handler_expr;
        if !runtime_mods.is_empty() {
            self.imports.add(VaporImportDependencies::WITH_MODIFIERS);
            let mods_str = runtime_mods
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            wrapped = format!("_withModifiers({}, [{}])", wrapped, mods_str);
        }
        if !key_mods.is_empty() {
            self.imports.add(VaporImportDependencies::WITH_KEYS);
            let keys_str = key_mods
                .iter()
                .map(|m| format!("\"{}\"", m))
                .collect::<Vec<_>>()
                .join(", ");
            wrapped = format!("_withKeys({}, [{}])", wrapped, keys_str);
        }
        let invoker_expr = format!("_createInvoker({})", wrapped);

        if is_dynamic {
            // Dynamic event: @[eventName]="handler"
            // → _on(n{X}, expr, handler, { effect: true }) inside _renderEffect
            let arg_span = prop.arg.ok_or(TemplateCodeGenError::MissingArg(
                "dynamic On prop must have arg span",
            ))?;
            let arg_raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
            let arg_prefixed = apply_dynamic_arg_prefix(
                arg_raw,
                arg_span.start,
                &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                &self.bindings,
                self.is_production,
            );
            self.imports.add(VaporImportDependencies::ON);
            self.stack
                .last_mut()
                .ok_or(TemplateCodeGenError::StackUnderflow(
                    "process_event: stack empty for dynamic event",
                ))?
                .effects
                .push(VaporEffect::OnDynamic {
                    node_ref,
                    event_expr: arg_prefixed,
                    handler: invoker_expr,
                });
        } else {
            let non_delegatable = Self::has_non_delegatable_modifier(&prop.modifiers, ctx);

            if !non_delegatable && Self::is_delegatable(&event_name) {
                if self.delegated_events_set.insert(event_name.clone()) {
                    self.delegated_events.push(event_name.clone());
                }
                let stmt = format!("n{}.$evt{} = {}", node_ref, event_name, invoker_expr);
                self.stack
                    .last_mut()
                    .ok_or(TemplateCodeGenError::StackUnderflow(
                        "process_event: stack empty for delegated event",
                    ))?
                    .statements
                    .push(stmt);
            } else {
                self.imports.add(VaporImportDependencies::ON);
                let opts = if !listener_opts.is_empty() {
                    let opts_str = listener_opts
                        .iter()
                        .map(|o| format!("{}: true", o))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(", {{ {} }}", opts_str)
                } else {
                    String::new()
                };
                let stmt = format!(
                    "_on(n{}, \"{}\", {}{})",
                    node_ref, event_name, invoker_expr, opts
                );
                self.stack
                    .last_mut()
                    .ok_or(TemplateCodeGenError::StackUnderflow(
                        "process_event: stack empty for non-delegated event",
                    ))?
                    .statements
                    .push(stmt);
            }
        }

        Ok(())
    }

    pub(super) fn process_model(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) -> TemplateCodeGenResult {
        let Some(ref exp) = oxc_prop.exp else {
            return Ok(());
        };

        let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
        let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);

        let tag_name = &self
            .stack
            .last()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "process_model: stack empty",
            ))?
            .tag_name
            .clone();

        // Determine which apply*Model helper to use.
        let helper = match tag_name.as_str() {
            "select" => {
                self.imports
                    .add(VaporImportDependencies::APPLY_SELECT_MODEL);
                "_applySelectModel"
            }
            "textarea" => {
                self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                "_applyTextModel"
            }
            "input" => {
                let input_type = Self::find_static_attr_value("type", ev, ctx);
                match input_type.as_deref() {
                    Some("checkbox") => {
                        self.imports
                            .add(VaporImportDependencies::APPLY_CHECKBOX_MODEL);
                        "_applyCheckboxModel"
                    }
                    Some("radio") => {
                        self.imports.add(VaporImportDependencies::APPLY_RADIO_MODEL);
                        "_applyRadioModel"
                    }
                    _ => {
                        self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                        "_applyTextModel"
                    }
                }
            }
            _ => {
                self.imports.add(VaporImportDependencies::APPLY_TEXT_MODEL);
                "_applyTextModel"
            }
        };

        // Build modifiers object if any.
        let prop = &oxc_prop.event;
        let mods_str = if let Some(ref mods) = prop.modifiers {
            let entries: Vec<String> = mods
                .iter()
                .map(|m| {
                    let name = &ctx.input[m.start as usize..m.end as usize];
                    format!("{}: true", name)
                })
                .collect();
            if entries.is_empty() {
                String::new()
            } else {
                format!(", {{ {} }}", entries.join(","))
            }
        } else {
            String::new()
        };

        let stmt = format!(
            "{}(n{}, () => ({}), _value => ({} = _value){})",
            helper, node_ref, prefixed, prefixed, mods_str
        );
        self.stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "process_model: stack empty after building statement",
            ))?
            .statements
            .push(stmt);

        Ok(())
    }

    pub(super) fn process_directive(
        &mut self,
        oxc_prop: &crate::syntax_kai::types::OxcProp<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        node_ref: u32,
    ) -> TemplateCodeGenResult {
        let prop = &oxc_prop.event;

        // Extract directive name: "v-my-directive" → "my-directive"
        let dir_raw_name = &ctx.input[prop.start as usize..prop.name_end as usize];
        let dir_name = dir_raw_name.strip_prefix("v-").unwrap_or(dir_raw_name);
        let dir_var = format!("_directive_{}", dir_name.replace('-', "_"));

        // Register for _resolveDirective declaration (deduped).
        if self.resolutions.directives_set.insert(dir_name.to_string()) {
            self.resolutions.directives.push(dir_name.to_string());
            self.imports.add(VaporImportDependencies::RESOLVE_DIRECTIVE);
            self.resolutions.directive_decls.push(format!(
                "  const {} = _resolveDirective(\"{}\")",
                dir_var, dir_name
            ));
        }
        self.imports
            .add(VaporImportDependencies::WITH_VAPOR_DIRECTIVES);

        // Build value expression.
        let value = if let Some(ref exp) = oxc_prop.exp {
            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
            let prefixed = self.prefix_expr(expr_text, exp.start, &exp.bindings);
            format!("() => {}", prefixed)
        } else {
            String::new()
        };

        // Build arg.
        let arg = prop
            .arg
            .map(|arg_span| {
                let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
                if prop.has_dynamic_arg {
                    apply_dynamic_arg_prefix(
                        raw,
                        arg_span.start,
                        &oxc_prop.arg.as_ref().and_then(|a| a.bindings.clone()),
                        &self.bindings,
                        self.is_production,
                    )
                } else {
                    format!("\"{}\"", raw)
                }
            })
            .unwrap_or_default();

        // Build modifiers object.
        let mods_str = if let Some(ref mods) = prop.modifiers {
            let entries: Vec<String> = mods
                .iter()
                .map(|m| {
                    let name = &ctx.input[m.start as usize..m.end as usize];
                    format!("{}: true", name)
                })
                .collect();
            if entries.is_empty() {
                String::new()
            } else {
                format!(", {{ {} }}", entries.join(", "))
            }
        } else {
            String::new()
        };

        // Build directive entry: [directive, value, arg, mods]
        let mut entry = dir_var;
        if !value.is_empty() || !arg.is_empty() || !mods_str.is_empty() {
            entry = format!(
                "{}, {}",
                entry,
                if value.is_empty() { "void 0" } else { &value }
            );
        }
        if !arg.is_empty() || !mods_str.is_empty() {
            entry = format!(
                "{}, {}",
                entry,
                if arg.is_empty() { "void 0" } else { &arg }
            );
        }
        if !mods_str.is_empty() {
            // mods_str already starts with ", { ... }"
            entry = format!("{}{}", entry, mods_str);
        }

        let stmt = format!("_withVaporDirectives(n{}, [[{}]])", node_ref, entry);
        self.stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "process_directive: stack empty after building statement",
            ))?
            .statements
            .push(stmt);

        Ok(())
    }

    /// Find the value of a static attribute on the element.
    pub(super) fn find_static_attr_value(
        attr_name: &str,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> Option<String> {
        for prop in &ev.event.props {
            if prop.kind == PropKind::Value {
                let name = &ctx.input[prop.start as usize..prop.name_end as usize];
                if name == attr_name {
                    if let Some(ref val) = prop.value {
                        return Some(ctx.input[val.start as usize..val.end as usize].to_string());
                    }
                }
            }
        }
        None
    }
}
