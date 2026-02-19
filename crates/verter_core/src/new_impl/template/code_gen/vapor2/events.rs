//! Vapor2 event handling code generation.
//!
//! Handles event delegation, modifiers, and dynamic events.

use crate::new_impl::ast::types::{ElementNode, PropFlags};
use crate::new_impl::template::code_gen::shared::helpers::{push_u32, VaporHelper};
use crate::new_impl::template::code_gen::types::CodeGenOutput;
use crate::new_impl::types::NodeId;

/// Events that can be delegated (standard DOM event types).
const DELEGATABLE_EVENTS: &[&str] = &[
    "click",
    "dblclick",
    "mousedown",
    "mouseup",
    "mousemove",
    "mouseenter",
    "mouseleave",
    "mouseover",
    "mouseout",
    "keydown",
    "keyup",
    "keypress",
    "input",
    "change",
    "focus",
    "blur",
    "submit",
    "reset",
    "scroll",
    "wheel",
    "touchstart",
    "touchmove",
    "touchend",
    "touchcancel",
    "pointerdown",
    "pointerup",
    "pointermove",
    "pointerenter",
    "pointerleave",
    "pointerover",
    "pointerout",
    "contextmenu",
    "drag",
    "dragstart",
    "dragend",
    "dragenter",
    "dragleave",
    "dragover",
    "drop",
    "focusin",
    "focusout",
];

/// Check if an event name is delegatable (standard DOM events without capture/passive/once).
fn is_delegatable(event_name: &str) -> bool {
    DELEGATABLE_EVENTS.contains(&event_name)
}

/// Runtime modifier names (wrapped via _withModifiers).
const RUNTIME_MODIFIERS: &[&str] = &[
    "stop", "prevent", "self", "ctrl", "shift", "alt", "meta", "left", "middle", "right", "exact",
];

/// Key modifier names (wrapped via _withKeys).
const KEY_MODIFIERS: &[&str] = &[
    "enter", "tab", "delete", "esc", "space", "up", "down", "left", "right",
];

/// Process event listeners on an element.
///
/// Emits event handler statements (NOT inside renderEffect).
/// Returns `true` if any events were processed.
pub fn process_events<'alloc>(
    id: NodeId,
    element: &ElementNode,
    source: &'alloc str,
    body_lines: &mut Vec<&'alloc str>,
    delegated_events: &mut Vec<&'alloc str>,
    delegated_events_set: &mut rustc_hash::FxHashSet<&'alloc str>,
    out: &mut CodeGenOutput<'alloc>,
) -> bool {
    if !element.prop_flag.has(PropFlags::HasEventListener) {
        return false;
    }

    let mut emitted = false;

    for prop in &element.props {
        if !prop.is_directive {
            continue;
        }

        let name = &source[prop.start as usize..prop.name_end as usize];

        // Detect event listeners: @click or v-on:click
        // Note: For shorthand `@click`, the AST stores name as just `"@"` with
        // the event name in arg_start..arg_end. For `v-on:click`, name is `"v-on"`
        // and arg holds `"click"`.
        let event_name = if let Some(after_at) = name.strip_prefix('@') {
            // Shorthand: name might be "@" (just prefix) or "@click" (inline)
            if after_at.is_empty() {
                // Event name stored in arg field
                if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                    &source[s as usize..e as usize]
                } else {
                    continue;
                }
            } else {
                after_at
            }
        } else if name == "v-on" {
            // v-on:event or v-on="obj"
            if let (Some(s), Some(e)) = (prop.arg_start, prop.arg_end) {
                &source[s as usize..e as usize]
            } else {
                continue; // v-on="obj" spread — skip for now
            }
        } else {
            continue; // Not an event
        };

        let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
            continue;
        };
        let handler = &source[vs as usize..ve as usize];

        // Check if dynamic arg
        let is_dynamic_arg = prop.is_dynamic == Some(true);

        // Collect modifiers
        let mut runtime_mods: Vec<&str> = Vec::new();
        let mut key_mods: Vec<&str> = Vec::new();
        let mut has_capture = false;
        let mut has_passive = false;
        let mut has_once = false;

        for modifier in &prop.modifiers {
            let mod_name = &source[modifier.start as usize..modifier.end as usize];
            match mod_name {
                "capture" => has_capture = true,
                "passive" => has_passive = true,
                "once" => has_once = true,
                m if RUNTIME_MODIFIERS.contains(&m) => runtime_mods.push(m),
                m if KEY_MODIFIERS.contains(&m) => key_mods.push(m),
                _ => {} // Unknown modifiers ignored
            }
        }

        let non_delegatable = has_capture || has_passive || has_once || is_dynamic_arg;

        if is_dynamic_arg {
            // Dynamic event: _on(n{id}, _ctx.eventName, handler, { effect: true })
            // Goes inside renderEffect — but for simplicity, emit as statement
            let mut line = String::with_capacity(64);
            line.push_str("  _on(n");
            push_u32(&mut line, id.0 as u32);
            line.push_str(", ");
            line.push_str(event_name);
            line.push_str(", ");
            line.push_str(handler);
            line.push(')');
            body_lines.push(out.alloc_str(&line));
            out.add_vapor_import(VaporHelper::On);
            emitted = true;
        } else if non_delegatable || !is_delegatable(event_name) {
            // Non-delegatable: _on(n{id}, "event", handler, options?)
            let mut line = String::with_capacity(64);
            line.push_str("  _on(n");
            push_u32(&mut line, id.0 as u32);
            line.push_str(", \"");
            line.push_str(event_name);
            line.push_str("\", ");
            write_wrapped_handler(&mut line, handler, &runtime_mods, &key_mods);

            // Options
            if has_capture || has_passive || has_once {
                line.push_str(", { ");
                let mut first = true;
                if has_capture {
                    line.push_str("capture: true");
                    first = false;
                }
                if has_passive {
                    if !first {
                        line.push_str(", ");
                    }
                    line.push_str("passive: true");
                    first = false;
                }
                if has_once {
                    if !first {
                        line.push_str(", ");
                    }
                    line.push_str("once: true");
                }
                line.push_str(" }");
            }

            line.push(')');
            body_lines.push(out.alloc_str(&line));
            out.add_vapor_import(VaporHelper::On);
            emitted = true;
        } else {
            // Delegatable: event delegation pattern
            // Register delegate
            let event_alloc = out.alloc_str(event_name);
            if delegated_events_set.insert(event_alloc) {
                delegated_events.push(event_alloc);
            }

            // n{id}.$evt{event} = _createInvoker(handler)
            let mut line = String::with_capacity(64);
            line.push_str("  n");
            push_u32(&mut line, id.0 as u32);
            line.push_str(".$evt");
            line.push_str(event_name);
            line.push_str(" = _createInvoker(");
            write_wrapped_handler(&mut line, handler, &runtime_mods, &key_mods);
            line.push(')');
            body_lines.push(out.alloc_str(&line));
            out.add_vapor_import(VaporHelper::DelegateEvents);
            out.add_vapor_import(VaporHelper::CreateInvoker);
            emitted = true;
        }
    }

    emitted
}

/// Write a handler expression, wrapping with _withModifiers / _withKeys if needed.
fn write_wrapped_handler(
    buf: &mut String,
    handler: &str,
    runtime_mods: &[&str],
    key_mods: &[&str],
) {
    if runtime_mods.is_empty() && key_mods.is_empty() {
        buf.push_str(handler);
        return;
    }

    if !key_mods.is_empty() {
        buf.push_str("_withKeys(");
    }
    if !runtime_mods.is_empty() {
        buf.push_str("_withModifiers(");
        buf.push_str(handler);
        buf.push_str(", [");
        for (i, m) in runtime_mods.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            buf.push_str(m);
            buf.push('"');
        }
        buf.push_str("])");
    } else {
        buf.push_str(handler);
    }
    if !key_mods.is_empty() {
        buf.push_str(", [");
        for (i, m) in key_mods.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push('"');
            buf.push_str(m);
            buf.push('"');
        }
        buf.push_str("])");
    }
}
