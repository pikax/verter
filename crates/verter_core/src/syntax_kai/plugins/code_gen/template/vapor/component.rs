//! Vapor component handling: setup, resolution, and component call building.

use crate::syntax_kai::{
    plugin::SyntaxPluginContext,
    plugins::code_gen::types::{
        TemplateCodeGenError, TemplateCodeGenResult, VaporImportDependencies,
    },
    types::{OxcCompiledElementStart, PropKind},
};

use super::types::{VaporEffect, VaporElementKind, VaporElementState};
use super::VaporTemplateGenerator;

impl<'alloc> VaporTemplateGenerator<'alloc> {
    /// Set up component resolution and detect built-in components.
    pub(super) fn setup_component(
        &mut self,
        tag_name: &str,
        state: &mut VaporElementState,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let lower = tag_name.to_lowercase();

        match lower.as_str() {
            "teleport" => {
                self.imports.add(VaporImportDependencies::VAPOR_TELEPORT);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                if let VaporElementKind::Component { component_var, .. } = &mut state.kind {
                    *component_var = "_VaporTeleport".to_string();
                }
            }
            "transition" => {
                self.imports.add(VaporImportDependencies::VAPOR_TRANSITION);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                if let VaporElementKind::Component { component_var, .. } = &mut state.kind {
                    *component_var = "_VaporTransition".to_string();
                }
            }
            "transition-group" | "transitiongroup" => {
                self.imports
                    .add(VaporImportDependencies::VAPOR_TRANSITION_GROUP);
                self.imports.add(VaporImportDependencies::CREATE_COMPONENT);
                if let VaporElementKind::Component { component_var, .. } = &mut state.kind {
                    *component_var = "_VaporTransitionGroup".to_string();
                }
            }
            "keep-alive" | "keepalive" => {
                // KeepAlive uses _resolveComponent + _createComponentWithFallback.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                self.imports.add(VaporImportDependencies::WITH_VAPOR_CTX);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if self.resolutions.components_set.insert(tag_name.to_string()) {
                    self.resolutions.components.push(tag_name.to_string());
                    self.resolutions.component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                if let VaporElementKind::Component {
                    component_var,
                    needs_vapor_ctx,
                    ..
                } = &mut state.kind
                {
                    *component_var = comp_var;
                    *needs_vapor_ctx = true;
                }
            }
            "suspense" => {
                // Suspense uses _resolveComponent + _createComponentWithFallback.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                self.imports.add(VaporImportDependencies::WITH_VAPOR_CTX);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if self.resolutions.components_set.insert(tag_name.to_string()) {
                    self.resolutions.components.push(tag_name.to_string());
                    self.resolutions.component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                if let VaporElementKind::Component {
                    component_var,
                    needs_vapor_ctx,
                    ..
                } = &mut state.kind
                {
                    *component_var = comp_var;
                    *needs_vapor_ctx = true;
                }
            }
            _ if state.is_dynamic_component() => {
                // <component :is="expr"> → _createDynamicComponent
                self.imports
                    .add(VaporImportDependencies::CREATE_DYNAMIC_COMPONENT);
                // Extract :is expression.
                for oxc_prop in &ev.props {
                    let prop = &oxc_prop.event;
                    if prop.kind == PropKind::Bind {
                        if let Some(ref arg) = prop.arg {
                            let attr_name = &ctx.input[arg.start as usize..arg.end as usize];
                            if attr_name == "is" {
                                if let Some(ref exp) = oxc_prop.exp {
                                    let expr_text =
                                        &ctx.input[exp.start as usize..exp.end as usize];
                                    let prefixed =
                                        self.prefix_expr(expr_text, exp.start, &exp.bindings);
                                    if let VaporElementKind::DynamicComponent {
                                        dynamic_is_expr,
                                        ..
                                    } = &mut state.kind
                                    {
                                        *dynamic_is_expr = Some(prefixed);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                // Regular user component.
                self.imports.add(VaporImportDependencies::RESOLVE_COMPONENT);
                self.imports
                    .add(VaporImportDependencies::CREATE_COMPONENT_WITH_FALLBACK);
                let comp_var = format!("_component_{}", tag_name.replace('-', "_"));
                if self.resolutions.components_set.insert(tag_name.to_string()) {
                    self.resolutions.components.push(tag_name.to_string());
                    self.resolutions.component_decls.push(format!(
                        "const {} = _resolveComponent(\"{}\")",
                        comp_var, tag_name
                    ));
                }
                if let VaporElementKind::Component { component_var, .. } = &mut state.kind {
                    *component_var = comp_var;
                }
            }
        }
    }

    /// Build a component call expression (without `const n{X} = ` prefix).
    pub(super) fn build_component_call(
        &mut self,
        state: &mut VaporElementState,
        indent: &str,
    ) -> TemplateCodeGenResult<String> {
        if state.is_slot_outlet() {
            return Ok(self.build_slot_outlet_call(state));
        }

        if state.is_dynamic_component() {
            return Ok(self.build_dynamic_component_call(state, indent));
        }

        let comp_var = if let VaporElementKind::Component {
            ref component_var, ..
        } = state.kind
        {
            component_var.clone()
        } else {
            panic!("build_component_call: kind must be Component")
        };

        // Determine if this uses _createComponent or _createComponentWithFallback.
        let is_builtin_create = comp_var.starts_with("_Vapor");
        let create_fn = if is_builtin_create {
            "_createComponent"
        } else {
            "_createComponentWithFallback"
        };

        // Build props object.
        let props_str = self.build_component_props(state);

        // Build slots object.
        let slots_str = self.build_component_slots(state, indent)?;

        Ok(format!(
            "{}({}, {}, {}, true)",
            create_fn, comp_var, props_str, slots_str
        ))
    }

    /// Build props object for a component call.
    pub(super) fn build_component_props(&self, state: &VaporElementState) -> String {
        // Collect effects as reactive props using structured VaporEffect data.
        // Each effect like SetProp { attr: "title", expr: "_ctx.msg" }
        // becomes `title: () => (_ctx.msg)` in the props object.
        if state.effects.is_empty() {
            return "null".to_string();
        }

        let mut entries = Vec::new();
        for effect in &state.effects {
            if let Some(prop_entry) = effect.to_component_prop() {
                entries.push(prop_entry);
            }
        }

        if entries.is_empty() {
            "null".to_string()
        } else {
            format!("{{ {} }}", entries.join(", "))
        }
    }

    /// Build slots object for a component call.
    pub(super) fn build_component_slots(
        &mut self,
        state: &mut VaporElementState,
        indent: &str,
    ) -> TemplateCodeGenResult<String> {
        if state.kind.slot_children().is_none_or(|sc| sc.is_empty()) {
            return Ok("null".to_string());
        }

        let slots = std::mem::take(state.kind.slot_children_mut().ok_or(
            TemplateCodeGenError::MissingScope(
                "build_component_slots: kind must have slot_children",
            ),
        )?);
        let needs_vapor_ctx = matches!(
            state.kind,
            VaporElementKind::Component {
                needs_vapor_ctx: true,
                ..
            }
        );

        let mut static_slots = Vec::new();
        let mut dynamic_slots = Vec::new();

        for slot in slots {
            if slot.is_dynamic {
                dynamic_slots.push(slot);
            } else {
                static_slots.push(slot);
            }
        }

        let mut parts = Vec::new();

        for slot in &static_slots {
            let params = slot.params.as_deref().unwrap_or("");
            let wrapper_start = if needs_vapor_ctx && slot.name == "default" {
                format!("_withVaporCtx(({}) => {{\n", params)
            } else {
                format!("({}) => {{\n", params)
            };
            let wrapper_end = if needs_vapor_ctx && slot.name == "default" {
                format!("{}}})", indent)
            } else {
                format!("{}}}", indent)
            };
            parts.push(format!(
                "\"{}\": {}{}{}",
                slot.name, wrapper_start, slot.body, wrapper_end
            ));
        }

        if !dynamic_slots.is_empty() {
            let mut dyn_entries = Vec::new();
            for slot in &dynamic_slots {
                let name_expr = slot.dynamic_name_expr.as_deref().unwrap_or("\"default\"");
                let params = slot.params.as_deref().unwrap_or("");
                dyn_entries.push(format!(
                    "() => ({{\n{}  name: {},\n{}  fn: ({}) => {{\n{}{}\n{}  }}\n{}}})",
                    indent, name_expr, indent, params, indent, slot.body, indent, indent
                ));
            }
            parts.push(format!("$: [{}]", dyn_entries.join(", ")));
        }

        if parts.is_empty() {
            Ok("null".to_string())
        } else {
            Ok(format!(
                "{{\n{}  {}\n{}}}",
                indent,
                parts.join(&format!(",\n{}  ", indent)),
                indent
            ))
        }
    }

    /// Build a `_createSlot(...)` call for `<slot>` outlets.
    pub(super) fn build_slot_outlet_call(&mut self, state: &VaporElementState) -> String {
        self.imports.add(VaporImportDependencies::CREATE_SLOT);

        // Extract slot name from the `name` static attribute or `:name` binding.
        let mut slot_name = "\"default\"".to_string();
        let mut slot_props: Vec<String> = Vec::new();

        for effect in &state.effects {
            // Check for SetProp { attr: "name", .. } → dynamic slot name
            if let VaporEffect::SetProp { attr, expr, .. } = effect {
                if attr == "name" {
                    slot_name = expr.clone();
                } else if let Some(prop_entry) = effect.to_component_prop() {
                    slot_props.push(prop_entry);
                }
            } else if let Some(prop_entry) = effect.to_component_prop() {
                slot_props.push(prop_entry);
            }
        }

        // Check static attributes for name="..."
        // The name is stored in slot_name field if set via process_scopes,
        // but for <slot name="header">, it's a static prop in the HTML.
        // We need to check the tag_name context — for slot outlets,
        // the static `name` attribute was already baked into HTML (but shouldn't be).
        // Actually, for <slot>, we skip HTML building, so static props aren't in HTML.
        // We need to check statements for static name.
        if slot_name == "\"default\"" {
            // Check if there's a static name in the statements (from Value prop processing).
            // Actually, static `name` on <slot> is handled differently — it's a Prop::Value
            // that we need to extract. But since we skip process_props for slot outlets,
            // we need to handle it here. Let's check the state's tag_name context.
            // For now, the slot_name from the slot_name field takes precedence.
            if let VaporElementKind::SlotOutlet {
                slot_name: Some(ref sn),
                ..
            } = state.kind
            {
                slot_name = format!("\"{}\"", sn);
            }
        }

        let props_str = if slot_props.is_empty() {
            "null".to_string()
        } else {
            format!("{{ {} }}", slot_props.join(", "))
        };

        format!("_createSlot({}, {})", slot_name, props_str)
    }

    /// Build a `_createDynamicComponent(...)` call.
    pub(super) fn build_dynamic_component_call(
        &self,
        state: &VaporElementState,
        _indent: &str,
    ) -> String {
        let is_expr = if let VaporElementKind::DynamicComponent {
            ref dynamic_is_expr,
            ..
        } = state.kind
        {
            dynamic_is_expr.as_deref().unwrap_or("undefined")
        } else {
            "undefined"
        };
        format!(
            "_createDynamicComponent(() => ({}), null, null, true)",
            is_expr
        )
    }
}
