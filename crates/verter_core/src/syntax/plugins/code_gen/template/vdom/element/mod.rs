mod close;
mod props;
pub(crate) use close::{handle_element_close, handle_element_close_self_closing};

use oxc_ast::ast::Expression;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    code_transform::CodeTransform,
    syntax::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::{
                build_prefixed_value_into, camelize_capitalize_into, camelize_into,
                collect_binding_patches, escape_js_string, escape_js_string_in_place,
                escape_js_string_into, is_valid_js_prop_key,
            },
            types::TemplateImportDependencies,
        },
        types::{ElementKind, OxcCompiledElementStart, PropKind},
    },
    utils::vue::PatchFlags,
};

use super::{DirectiveEntry, StateStack};

/// Bundles generator-level mutable state passed to `handle_element_open`.
///
/// This avoids the need to pass 8+ individual parameters through the function signature.
/// The struct borrows from `VdomTemplateGenerator` fields for the duration of element processing.
pub(crate) struct ElementOpenContext<'a, 'alloc> {
    pub bindings: &'a FxHashMap<&'alloc str, BindingType>,
    pub is_production: bool,
    pub inline: bool,
    pub hoist_static: bool,
    pub imports: &'a mut TemplateImportDependencies,
    pub resolved_components: &'a mut Vec<&'alloc str>,
    pub resolved_components_set: &'a mut FxHashSet<&'alloc str>,
    pub resolved_directives: &'a mut Vec<&'alloc str>,
    pub resolved_directives_set: &'a mut FxHashSet<&'alloc str>,
    pub hoisted_constants: &'a mut Vec<&'alloc str>,
}

/// Try to resolve a component tag name against setup bindings.
///
/// Checks exact match, camelCase, then PascalCase (matching Vue's resolver order).
/// Returns the binding name if found as a setup binding, None otherwise.
fn resolve_setup_component<'a>(
    tag_name: &str,
    bindings: &FxHashMap<&'a str, BindingType>,
    buf: &mut String,
) -> Option<&'a str> {
    // 1. Exact match (handles <MyComponent> when MyComponent is a binding)
    if let Some((&key, bt)) = bindings.get_key_value(tag_name) {
        if bt.is_setup() {
            return Some(key);
        }
    }

    // 2. camelCase (handles <my-component> → myComponent)
    buf.clear();
    camelize_into(tag_name, buf);
    if let Some((&key, bt)) = bindings.get_key_value(buf.as_str()) {
        if bt.is_setup() {
            return Some(key);
        }
    }

    // 3. PascalCase (handles <my-component> → MyComponent)
    buf.clear();
    camelize_capitalize_into(tag_name, buf);
    if let Some((&key, bt)) = bindings.get_key_value(buf.as_str()) {
        if bt.is_setup() {
            return Some(key);
        }
    }

    None
}

/// Check whether an event handler expression needs wrapping in `$event => (...)`.
///
/// Vue wraps expressions that are NOT simple identifiers, member accesses,
/// arrow functions, or function expressions. Call expressions like `fn($event)`
/// need wrapping because they execute immediately rather than deferring.
pub(super) fn needs_event_handler_wrap(exp: &Option<oxc_ast::ast::Expression>) -> bool {
    match exp {
        None => false,
        Some(expr) => !matches!(
            expr,
            Expression::Identifier(_)
                | Expression::StaticMemberExpression(_)
                | Expression::ComputedMemberExpression(_)
                | Expression::ArrowFunctionExpression(_)
                | Expression::FunctionExpression(_)
        ),
    }
}

/// Process the opening of an element.
///
/// Transforms the open tag source (`<tagname ...attrs... >`) into the beginning
/// of a VNode call: `_createElementVNode("tagname", {props}` for native elements,
/// or `_createVNode(TagName, {props}` for components.
///
/// Does NOT add a parent separator — the parent's close phase handles that
/// retroactively via `prepend_left`.
///
/// Mutates `state`: sets `is_component`, `patch_flag`, `dynamic_props`, `open_tag_end`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_element_open<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    ev: &OxcCompiledElementStart<'alloc>,
    ctx: &SyntaxPluginContext<'alloc>,
    state: &mut StateStack<'alloc>,
    ectx: &mut ElementOpenContext<'_, 'alloc>,
    binding_patches: &mut Vec<(u32, &'alloc str)>,
    pending_overwrites: &mut Vec<(u32, u32, &'alloc str)>,
    buf: &mut String,
) {
    let bindings = ectx.bindings;
    let is_production = ectx.is_production;
    let imports = &mut *ectx.imports;
    let resolved_components = &mut *ectx.resolved_components;
    let resolved_components_set = &mut *ectx.resolved_components_set;
    let resolved_directives = &mut *ectx.resolved_directives;
    let resolved_directives_set = &mut *ectx.resolved_directives_set;
    let hoisted_constants = &mut *ectx.hoisted_constants;
    let open_tag = &ev.event.event_open_tag;
    let open_tag_end = &ev.event.event_open_tag_end;
    let is_component = open_tag.kind.is_component();

    state.is_component = is_component;
    state.open_tag_start = open_tag.start;
    state.open_tag_end = open_tag_end.end;

    // Named slot template: <template #name> inside a component.
    // Don't emit a VNode — the close handler will emit the slot entry directly.
    if state.is_named_slot_template {
        pending_overwrites.push((open_tag.start, open_tag_end.end, ""));
        return;
    }

    // Slot outlet: <slot/> or <slot name="xxx"/> → _renderSlot($slots, "name")
    if open_tag.kind == ElementKind::SlotOutlet {
        state.is_slot_outlet = true;
        let imports = &mut *ectx.imports;
        imports.add(TemplateImportDependencies::RENDER_SLOT);

        // Extract static slot name from props (defaults to "default")
        let slot_name = ev.props.iter().find_map(|p| {
            if p.event.kind == PropKind::Value {
                let name = &ctx.input[p.event.start as usize..p.event.name_end as usize];
                if name == "name" {
                    return p
                        .event
                        .value
                        .map(|v| &ctx.input[v.start as usize..v.end as usize]);
                }
            }
            None
        });

        buf.clear();
        buf.push_str("_renderSlot(_ctx.$slots, \"");
        buf.push_str(slot_name.unwrap_or("default"));
        buf.push('"');

        let s = code_transform.alloc_str(buf);
        pending_overwrites.push((open_tag.start, open_tag_end.end, s));
        return;
    }

    // Inherit patch flags already estimated during syntax parsing
    state.patch_flag = open_tag.patch_flag;

    // Tag name from source bytes
    let tag_name = &ctx.input[open_tag.start as usize + 1..open_tag.name_end as usize];

    // For components, check setup bindings first. If the component is available from
    // setup, use $setup["Name"] (standalone) or bare Name (inline) instead of _resolveComponent.
    let inline = ectx.inline;
    let component_var: Option<&'alloc str> = if is_component {
        if let Some(binding_name) = resolve_setup_component(tag_name, bindings, buf) {
            // Component found in setup bindings — use direct reference
            buf.clear();
            if inline {
                buf.push_str(binding_name);
            } else {
                buf.push_str("$setup[\"");
                buf.push_str(binding_name);
                buf.push_str("\"]");
            }
            Some(code_transform.alloc_str(buf))
        } else {
            // Not in setup — fall back to _resolveComponent
            buf.clear();
            buf.push_str("_component_");
            buf.push_str(tag_name);
            let var_name = code_transform.alloc_str(buf);
            if resolved_components_set.insert(tag_name) {
                resolved_components.push(tag_name);
                imports.add(TemplateImportDependencies::RESOLVE_COMPONENT);
            }
            Some(var_name)
        }
    } else {
        None
    };

    // Pre-scan: will this element need _withDirectives() wrapping?
    // Native v-model, v-show, and custom directives all produce runtime directives.
    // Component v-model is prop-based, not directive-based.
    let needs_with_directives = !is_component
        && ev.props.iter().any(|p| {
            matches!(
                p.event.kind,
                PropKind::Model | PropKind::Show | PropKind::Directive
            )
        });
    let wd_prefix = if needs_with_directives {
        "_withDirectives("
    } else {
        ""
    };

    // <template v-if/v-for> uses _Fragment instead of string "template".
    let is_template_fragment = open_tag.kind == ElementKind::Template;

    // Build the VNode call prefix into buf (deferred — may merge with props overwrite)
    if state.is_block_root {
        // Block root: (_openBlock(), _createElementBlock("tag" or _createBlock(_component_Tag
        imports.add(TemplateImportDependencies::OPEN_BLOCK);
        if let Some(var) = component_var {
            imports.add(TemplateImportDependencies::CREATE_BLOCK);
            buf.clear();
            buf.push_str(wd_prefix);
            buf.push_str("(_openBlock(), _createBlock(");
            buf.push_str(var);
        } else if is_template_fragment {
            imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
            imports.add(TemplateImportDependencies::FRAGMENT);
            buf.clear();
            buf.push_str(wd_prefix);
            buf.push_str("(_openBlock(), _createElementBlock(_Fragment");
        } else {
            imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
            buf.clear();
            buf.push_str(wd_prefix);
            buf.push_str("(_openBlock(), _createElementBlock(\"");
            buf.push_str(tag_name);
            buf.push('"');
        }
    } else if let Some(var) = component_var {
        imports.add(TemplateImportDependencies::CREATE_VNODE);
        buf.clear();
        buf.push_str(wd_prefix);
        buf.push_str("_createVNode(");
        buf.push_str(var);
    } else if is_template_fragment {
        imports.add(TemplateImportDependencies::CREATE_ELEMENT_VNODE);
        imports.add(TemplateImportDependencies::FRAGMENT);
        buf.clear();
        buf.push_str(wd_prefix);
        buf.push_str("_createElementVNode(_Fragment");
    } else {
        imports.add(TemplateImportDependencies::CREATE_ELEMENT_VNODE);
        buf.clear();
        buf.push_str(wd_prefix);
        buf.push_str("_createElementVNode(\"");
        buf.push_str(tag_name);
        buf.push('"');
    }

    // -- Props --
    // For empty or all-static props, merge the tag prefix + props into a single
    // overwrite spanning (start, open_tag_end.end), halving deferred-op count
    // for the ~100 static elements in template-heavy.
    if ev.props.is_empty() {
        // Merge tag prefix + `, null` or `, { key: N }` into one overwrite
        if let Some(k) = state.vif_branch_key {
            buf.push_str(", { key: ");
            super::helper::push_u32(buf, k);
            buf.push_str(" }");
        } else {
            buf.push_str(", null");
        }
        let s = code_transform.alloc_str(buf);
        pending_overwrites.push((open_tag.start, open_tag_end.end, s));
    } else {
        state.has_props = true;

        // Check if ALL props are static (hoistable).
        // Static prop kinds: Value, ClassValue, StyleValue.
        // Components never get props hoisted (Vue rule).
        // Hoisting can be disabled via the hoist_static option.
        let all_static = ectx.hoist_static
            && !is_component
            && ev.props.iter().all(|p| {
                matches!(
                    p.event.kind,
                    PropKind::Value | PropKind::ClassValue | PropKind::StyleValue
                )
            });

        if all_static {
            // Build the props object string for hoisting into the shared buf.
            // Uses save/truncate pattern to avoid a separate String allocation.
            let saved = buf.len();
            buf.push_str("{ ");
            for (i, prop) in ev.props.iter().enumerate() {
                if i > 0 {
                    buf.push_str(", ");
                }
                match &prop.event.kind {
                    PropKind::ClassValue => {
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            buf.push_str("class: \"");
                            escape_js_string_into(buf, val);
                            buf.push('"');
                        }
                    }
                    PropKind::StyleValue => {
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            buf.push_str("style: \"");
                            escape_js_string_into(buf, val);
                            buf.push('"');
                        }
                    }
                    PropKind::Value => {
                        let name =
                            &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
                        let needs_quote = !is_valid_js_prop_key(name);
                        if let Some(val_span) = prop.event.value {
                            let val = &ctx.input[val_span.start as usize..val_span.end as usize];
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(name);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(": \"");
                            escape_js_string_into(buf, val);
                            buf.push('"');
                        } else {
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(name);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(": \"\"");
                        }
                    }
                    _ => unreachable!("all_static check guarantees only static prop kinds"),
                }
            }
            buf.push_str(" }");

            // Bump-allocate the props string and add to hoisted constants.
            let props_str = code_transform.alloc_str(&buf[saved..]);
            buf.truncate(saved);
            hoisted_constants.push(props_str);
            let hoist_id = hoisted_constants.len(); // 1-indexed
            state.has_all_static_props = true;

            // Merge tag prefix + `, _hoisted_N` into one overwrite
            buf.push_str(", _hoisted_");
            super::helper::push_u32(buf, hoist_id as u32);
            let s = code_transform.alloc_str(buf);
            pending_overwrites.push((open_tag.start, open_tag_end.end, s));
        } else {
            // Dynamic props can't be merged — push tag prefix as separate overwrite
            let tag_s = code_transform.alloc_str(buf);
            pending_overwrites.push((open_tag.start, open_tag.name_end, tag_s));

            state.has_all_static_props = false;

            // Pre-scan for class/style merging:
            // When both static (ClassValue/StyleValue) and dynamic (ClassBind/StyleBind)
            // exist, Vue merges them: class: _normalizeClass(["static", dynamic])
            let static_class: Option<String> = ev.props.iter().find_map(|p| {
                if p.event.kind == PropKind::ClassValue {
                    p.event
                        .value
                        .map(|v| escape_js_string(&ctx.input[v.start as usize..v.end as usize]))
                } else {
                    None
                }
            });
            let has_class_bind = ev.props.iter().any(|p| p.event.kind == PropKind::ClassBind);
            let merge_class = static_class.is_some() && has_class_bind;

            let static_style: Option<String> = ev.props.iter().find_map(|p| {
                if p.event.kind == PropKind::StyleValue {
                    p.event
                        .value
                        .map(|v| escape_js_string(&ctx.input[v.start as usize..v.end as usize]))
                } else {
                    None
                }
            });
            let has_style_bind = ev.props.iter().any(|p| p.event.kind == PropKind::StyleBind);
            let merge_style = static_style.is_some() && has_style_bind;

            // Check if all props are spread-like (BindSpread/OnSpread).
            // Spread props replace the entire props object (no `{}`).
            let is_spread_only = ev
                .props
                .iter()
                .all(|p| matches!(p.event.kind, PropKind::BindSpread | PropKind::OnSpread));

            // Normal inline props processing
            let first_prop_start = ev.props[0].event.start;
            if is_spread_only {
                pending_overwrites.push((open_tag.name_end, first_prop_start, ", "));
            } else if let Some(k) = state.vif_branch_key {
                // Inject v-if branch key at the beginning of the props object
                buf.clear();
                buf.push_str(", {key: ");
                super::helper::push_u32(buf, k);
                buf.push_str(", ");
                let s = code_transform.alloc_str(buf);
                pending_overwrites.push((open_tag.name_end, first_prop_start, s));
            } else {
                pending_overwrites.push((open_tag.name_end, first_prop_start, ", {"));
            }

            // Track how many props we've actually written (for separator logic).
            // Props that are skipped (e.g. ClassValue when merging) don't count.
            let mut written: usize = 0;

            for prop in ev.props.iter() {
                // When merging, skip the static ClassValue/StyleValue — they're folded
                // into the ClassBind/StyleBind handler below.
                if merge_class && prop.event.kind == PropKind::ClassValue {
                    pending_overwrites.push((prop.event.start, prop.event.end, ""));
                    continue;
                }
                if merge_style && prop.event.kind == PropKind::StyleValue {
                    pending_overwrites.push((prop.event.start, prop.event.end, ""));
                    continue;
                }

                let sep = if written > 0 { ", " } else { "" };

                match &prop.event.kind {
                    PropKind::Value => {
                        // Static attribute: name="value"
                        let name =
                            &ctx.input[prop.event.start as usize..prop.event.name_end as usize];
                        let needs_quote = !is_valid_js_prop_key(name);
                        if let Some(val_span) = prop.event.value {
                            // Split overwrite: prefix before value, escape value in-place, suffix after
                            buf.clear();
                            buf.push_str(sep);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(name);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(": \"");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            escape_js_string_in_place(
                                code_transform,
                                val_span.start,
                                val_span.end,
                                ctx.input,
                                pending_overwrites,
                            );
                            pending_overwrites.push((val_span.end, prop.event.end, "\""));
                        } else {
                            buf.clear();
                            buf.push_str(sep);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(name);
                            if needs_quote {
                                buf.push('"');
                            }
                            buf.push_str(": \"\"");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, prop.event.end, s));
                        }
                        written += 1;
                    }

                    PropKind::ClassValue => {
                        // Static class (no merging — merge_class is false here)
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("class: \"");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            escape_js_string_in_place(
                                code_transform,
                                val_span.start,
                                val_span.end,
                                ctx.input,
                                pending_overwrites,
                            );
                            pending_overwrites.push((val_span.end, prop.event.end, "\""));
                        }
                        written += 1;
                    }

                    PropKind::StyleValue => {
                        // Static style (no merging — merge_style is false here)
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("style: \"");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            escape_js_string_in_place(
                                code_transform,
                                val_span.start,
                                val_span.end,
                                ctx.input,
                                pending_overwrites,
                            );
                            pending_overwrites.push((val_span.end, prop.event.end, "\""));
                        }
                        written += 1;
                    }

                    PropKind::Bind => {
                        // :prop="expr" → prop_name: expr
                        let prop_name: &'alloc str = if let Some(arg_span) = prop.event.arg {
                            let raw = &ctx.input[arg_span.start as usize..arg_span.end as usize];
                            if prop.event.has_dynamic_arg {
                                // Dynamic arg: :[foo]="value" → [_ctx.foo]: value
                                // Built in buffer — computed property, no quoting needed
                                let saved = buf.len();
                                build_prefixed_value_into(
                                    buf,
                                    raw,
                                    arg_span.start,
                                    prop.arg.as_ref().and_then(|a| a.bindings.as_ref()),
                                    bindings,
                                    is_production,
                                    &[],
                                );
                                let prop_name = code_transform.alloc_str(&buf[saved..]);
                                buf.truncate(saved);

                                if let Some(val_span) = prop.event.value {
                                    buf.clear();
                                    buf.push_str(sep);
                                    buf.push_str(prop_name);
                                    buf.push_str(": ");
                                    let s = code_transform.alloc_str(buf);
                                    pending_overwrites.push((prop.event.start, val_span.start, s));
                                    pending_overwrites.push((val_span.end, prop.event.end, ""));

                                    if let Some(exp) = &prop.exp {
                                        collect_binding_patches(
                                            exp.bindings.as_ref(),
                                            bindings,
                                            is_production,
                                            binding_patches,
                                        );
                                    }
                                } else {
                                    buf.clear();
                                    buf.push_str(sep);
                                    buf.push_str(prop_name);
                                    buf.push_str(": undefined");
                                    let s = code_transform.alloc_str(buf);
                                    pending_overwrites.push((prop.event.start, prop.event.end, s));
                                }
                                prop_name
                            } else {
                                // Static arg: split overwrite — name stays from source,
                                // quotes added via boundary overwrite strings.
                                let needs_quote = !is_valid_js_prop_key(raw);
                                let before: &'static str = match (sep.is_empty(), needs_quote) {
                                    (true, false) => "",
                                    (true, true) => "\"",
                                    (false, false) => ", ",
                                    (false, true) => ", \"",
                                };

                                if let Some(val_span) = prop.event.value {
                                    pending_overwrites.push((
                                        prop.event.start,
                                        arg_span.start,
                                        before,
                                    ));
                                    let after: &'static str =
                                        if needs_quote { "\": " } else { ": " };
                                    pending_overwrites.push((arg_span.end, val_span.start, after));
                                    pending_overwrites.push((val_span.end, prop.event.end, ""));

                                    if let Some(exp) = &prop.exp {
                                        collect_binding_patches(
                                            exp.bindings.as_ref(),
                                            bindings,
                                            is_production,
                                            binding_patches,
                                        );
                                    }
                                } else {
                                    pending_overwrites.push((
                                        prop.event.start,
                                        arg_span.start,
                                        before,
                                    ));
                                    let after: &'static str = if needs_quote {
                                        "\": undefined"
                                    } else {
                                        ": undefined"
                                    };
                                    pending_overwrites.push((arg_span.end, prop.event.end, after));
                                }
                                raw
                            }
                        } else {
                            // No arg span — "unknown" fallback
                            if let Some(val_span) = prop.event.value {
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("unknown: ");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, val_span.start, s));
                                pending_overwrites.push((val_span.end, prop.event.end, ""));

                                if let Some(exp) = &prop.exp {
                                    collect_binding_patches(
                                        exp.bindings.as_ref(),
                                        bindings,
                                        is_production,
                                        binding_patches,
                                    );
                                }
                            } else {
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("unknown: undefined");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, prop.event.end, s));
                            }
                            "unknown"
                        };

                        // If this is a :key prop inside a v-for, mark the fragment as keyed
                        if prop_name == "key" {
                            for close in state.pending_scope_closes.iter_mut() {
                                if let super::ScopeClose::For { is_keyed } = close {
                                    *is_keyed = true;
                                }
                            }
                        }

                        state.dynamic_props.push(prop_name);
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::On => {
                        written += props::handle_prop_on(
                            code_transform,
                            prop,
                            sep,
                            ctx,
                            state,
                            bindings,
                            is_production,
                            imports,
                            buf,
                            binding_patches,
                            pending_overwrites,
                        );
                    }

                    PropKind::ClassBind => {
                        // :class="expr" → class: _normalizeClass(expr)
                        // With merge: class: _normalizeClass(["static", expr])
                        imports.add(TemplateImportDependencies::NORMALIZE_CLASS);

                        if let Some(val_span) = prop.event.value {
                            if merge_class {
                                let static_val = static_class.as_ref().unwrap();
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("class: _normalizeClass([\"");
                                buf.push_str(static_val);
                                buf.push_str("\", ");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, val_span.start, s));
                                pending_overwrites.push((val_span.end, prop.event.end, "])"));
                            } else {
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("class: _normalizeClass(");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, val_span.start, s));
                                pending_overwrites.push((val_span.end, prop.event.end, ")"));
                            }

                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }

                        state.patch_flag = state.patch_flag.add(PatchFlags::Class);
                        written += 1;
                    }

                    PropKind::StyleBind => {
                        // :style="expr" → style: _normalizeStyle(expr)
                        // With merge: style: _normalizeStyle(["static", expr])
                        imports.add(TemplateImportDependencies::NORMALIZE_STYLE);

                        if let Some(val_span) = prop.event.value {
                            if merge_style {
                                let static_val = static_style.as_ref().unwrap();
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("style: _normalizeStyle([\"");
                                buf.push_str(static_val);
                                buf.push_str("\", ");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, val_span.start, s));
                                pending_overwrites.push((val_span.end, prop.event.end, "])"));
                            } else {
                                buf.clear();
                                buf.push_str(sep);
                                buf.push_str("style: _normalizeStyle(");
                                let s = code_transform.alloc_str(buf);
                                pending_overwrites.push((prop.event.start, val_span.start, s));
                                pending_overwrites.push((val_span.end, prop.event.end, ")"));
                            }

                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }

                        state.patch_flag = state.patch_flag.add(PatchFlags::Style);
                        written += 1;
                    }

                    PropKind::Model => {
                        written += props::handle_prop_model(
                            code_transform,
                            prop,
                            sep,
                            ctx,
                            state,
                            bindings,
                            is_production,
                            imports,
                            tag_name,
                            is_component,
                            &ev.props,
                            buf,
                            pending_overwrites,
                        );
                    }

                    PropKind::Show => {
                        // v-show="expr" → withDirectives(vnode, [[vShow, expr]])
                        // No prop emitted — just add directive entry + NEED_PATCH flag
                        imports.add(TemplateImportDependencies::V_SHOW);
                        imports.add(TemplateImportDependencies::WITH_DIRECTIVES);

                        if let Some(val_span) = prop.event.value {
                            let val_text =
                                &ctx.input[val_span.start as usize..val_span.end as usize];
                            let prefixed_val: &'alloc str = if let Some(exp) = &prop.exp {
                                let saved = buf.len();
                                build_prefixed_value_into(
                                    buf,
                                    val_text,
                                    val_span.start,
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    &[],
                                );
                                let result = code_transform.alloc_str(&buf[saved..]);
                                buf.truncate(saved);
                                result
                            } else {
                                val_text
                            };

                            state.runtime_directives.push(DirectiveEntry {
                                directive: "_vShow",
                                value: prefixed_val,
                                arg: "",
                                modifiers: "",
                            });
                        }
                        // Remove v-show from props output
                        pending_overwrites.push((prop.event.start, prop.event.end, ""));
                        state.patch_flag = state.patch_flag.add(PatchFlags::NeedPatch);
                    }

                    PropKind::Html => {
                        // v-html="expr" → innerHTML: expr (as prop, no directive)
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("innerHTML: ");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            pending_overwrites.push((val_span.end, prop.event.end, ""));
                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }
                        state.dynamic_props.push("innerHTML");
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::Text => {
                        // v-text="expr" → textContent: _toDisplayString(expr)
                        imports.add(TemplateImportDependencies::TO_DISPLAY_STRING);
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("textContent: _toDisplayString(");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            pending_overwrites.push((val_span.end, prop.event.end, ")"));
                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }
                        state.dynamic_props.push("textContent");
                        state.patch_flag = state.patch_flag.add(PatchFlags::Props);
                        written += 1;
                    }

                    PropKind::BindSpread => {
                        // v-bind="obj" → _normalizeProps(_guardReactiveProps(expr))
                        imports.add(TemplateImportDependencies::NORMALIZE_PROPS);
                        imports.add(TemplateImportDependencies::GUARD_REACTIVE_PROPS);
                        // BindSpread replaces the entire props object, not a single prop.
                        // We mark this and handle it specially after the loop.
                        // For now: overwrite with _normalizeProps wrapper
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("_normalizeProps(_guardReactiveProps(");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            pending_overwrites.push((val_span.end, prop.event.end, "))"));
                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }
                        state.patch_flag = state.patch_flag.add(PatchFlags::FullProps);
                        written += 1;
                    }

                    PropKind::OnSpread => {
                        // v-on="handlers" → _toHandlers(expr, true)
                        imports.add(TemplateImportDependencies::TO_HANDLERS);
                        if let Some(val_span) = prop.event.value {
                            buf.clear();
                            buf.push_str(sep);
                            buf.push_str("_toHandlers(");
                            let s = code_transform.alloc_str(buf);
                            pending_overwrites.push((prop.event.start, val_span.start, s));
                            pending_overwrites.push((val_span.end, prop.event.end, ", true)"));
                            if let Some(exp) = &prop.exp {
                                collect_binding_patches(
                                    exp.bindings.as_ref(),
                                    bindings,
                                    is_production,
                                    binding_patches,
                                );
                            }
                        }
                        state.patch_flag = state.patch_flag.add(PatchFlags::FullProps);
                        written += 1;
                    }

                    PropKind::Directive => {
                        props::handle_prop_directive(
                            code_transform,
                            prop,
                            ctx,
                            state,
                            bindings,
                            is_production,
                            imports,
                            resolved_directives,
                            resolved_directives_set,
                            buf,
                            pending_overwrites,
                        );
                    }

                    // v-if, v-else-if, v-else, v-for, v-slot, v-once are handled
                    // as scopes in directives/mod.rs, not as prop kinds here.
                    _ => {
                        pending_overwrites.push((prop.event.start, prop.event.end, ""));
                    }
                }
            }

            // Close the props object: overwrite `>` with `}` (or empty for spread-only)
            let last_prop_end = ev.props.last().unwrap().event.end;
            if is_spread_only {
                pending_overwrites.push((last_prop_end, open_tag_end.end, ""));
            } else {
                pending_overwrites.push((last_prop_end, open_tag_end.end, "}"));
            }
        }
    }
}
