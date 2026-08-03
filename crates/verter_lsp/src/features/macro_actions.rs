// Macro code actions: generate/augment defineSlots and defineEmits from template usage.

use tower_lsp_server::ls_types::*;
use verter_semantic::analysis::types::{AnalysisFlags, AnalyzedBinding, AnalyzedMacroKind};
use verter_session::{AnalysisSourceRevision, FileAnalysisSnapshot};

use crate::documents::line_index::LineIndex;
use crate::documents::sfc_scanner::SfcBlock;
use crate::features::action_utils::{
    find_script_insert_offset, make_insert_action, needs_quoting, LiveEditTarget,
};

/// Context for resolving slot binding types.
///
/// Tier 1: look up expression in `bindings` by name, use `type_annotation`.
/// Tier 2 (future): fall back to TypeProvider hover for complex expressions.
pub struct SlotTypeContext<'a> {
    /// Script bindings from analysis (Tier 1).
    pub bindings: &'a [AnalyzedBinding],
    /// TypeProvider + mapping context (Tier 2 fallback, optional).
    pub type_provider: Option<()>, // placeholder for TypeProviderRef
}

/// Resolve the TypeScript type for a slot binding expression.
///
/// Tier 1: look up `expression` in `bindings` by name, return `type_annotation`.
/// Falls back to `"unknown"` if no match or no annotation.
fn resolve_binding_type(ctx: Option<&SlotTypeContext<'_>>, expression: &str) -> String {
    if let Some(ctx) = ctx {
        // Tier 1: direct binding lookup
        if let Some(binding) = ctx.bindings.iter().find(|b| b.name == expression) {
            if let Some(ref ann) = binding.type_annotation {
                return ann.clone();
            }
        }
        // Tier 2: TypeProvider fallback (future)
        // if let Some(ref tp) = ctx.type_provider { ... }
    }
    "unknown".to_string()
}

/// Build the slot member text for a single slot in the defineSlots type literal.
fn build_slot_member(
    slot: &verter_semantic::analysis::template::DefinedSlot,
    type_ctx: Option<&SlotTypeContext<'_>>,
) -> String {
    let mut member = String::new();
    member.push_str("    ");
    // Quote the slot name if it contains special characters
    let name = if needs_quoting(&slot.name) {
        format!("'{}'", slot.name)
    } else {
        slot.name.clone()
    };
    member.push_str(&name);
    member.push_str("(props: {");
    if slot.has_bindings && !slot.binding_names.is_empty() {
        for (i, binding) in slot.binding_names.iter().enumerate() {
            if i > 0 {
                member.push(',');
            }
            member.push(' ');
            member.push_str(binding);
            member.push_str(": ");
            // Resolve type from binding expression
            let expr = slot
                .binding_expressions
                .get(i)
                .map(|s| s.as_str())
                .unwrap_or(binding.as_str());
            member.push_str(&resolve_binding_type(type_ctx, expr));
        }
        member.push(' ');
    }
    member.push_str("}): any\n");
    member
}

/// Produce macro code actions based on template analysis vs script macros.
///
/// Returns actions for:
/// - B1: Generate `defineSlots` from template `<slot>` tags
/// - B2: Generate `defineEmits` from undeclared emit usage
/// - B3: Add missing slots to existing `defineSlots`
/// - B4: Add missing emits to existing `defineEmits`
///
/// When `cursor_offset` is `Some`, only actions relevant to the cursor position
/// are returned. Slot actions appear when the cursor is on a `<slot>` element or
/// on the `defineSlots` macro. Emit actions appear when the cursor is on an undeclared
/// emit usage or on the `defineEmits` macro. When `None`, all actions are returned.
///
/// `live_revision` is the content identity of the `source` bytes an edit will be
/// applied to. Every offset these actions carry — an anchor, an import span end —
/// was minted against the bytes the ANALYSIS observed, so the two identities must
/// agree before any edit is produced: an offset from another revision can be
/// perfectly in-bounds and still land in the wrong place.
pub fn macro_code_actions(
    source: &str,
    live_revision: AnalysisSourceRevision,
    analysis: Option<&FileAnalysisSnapshot>,
    blocks: &[SfcBlock],
    line_index: &LineIndex,
    cursor_offset: Option<u32>,
) -> Vec<CodeActionOrCommand> {
    let analysis = match analysis {
        Some(a) => a,
        None => return vec![],
    };

    // Revision gate, once and up front. An unstamped analysis also fails here,
    // so a snapshot whose producer never recorded its source identity yields no
    // edits rather than edits from unpaired geometry.
    if analysis.anchor_revision != live_revision {
        return vec![];
    }

    // Must have a <script setup> block
    let setup_block = match blocks.iter().find(|b| b.is_setup()) {
        Some(b) => b,
        None => return vec![],
    };

    let flags = AnalysisFlags::from_bits_truncate(analysis.script_flags);
    let template = match &analysis.template {
        Some(t) => t,
        None => return vec![],
    };

    // Build type context from analysis bindings
    let type_ctx = SlotTypeContext {
        bindings: &analysis.bindings,
        type_provider: None,
    };

    // Cursor-based filtering: determine which action categories are relevant.
    let (show_slot_actions, show_emit_actions) = match cursor_offset {
        None => (true, true), // No cursor → show all
        Some(offset) => {
            let on_slot_element = template
                .defined_slots
                .iter()
                .any(|s| offset >= s.span.start && offset <= s.span.end);
            let on_define_slots = analysis.macros.iter().any(|m| {
                m.kind == AnalyzedMacroKind::DefineSlots
                    && offset >= m.span.start
                    && offset <= m.span.end
            });
            let on_emit_usage = template
                .emit_definitions
                .iter()
                .filter(|e| !e.is_declared)
                .any(|e| offset >= e.span.start && offset <= e.span.end);
            let on_define_emits = analysis.macros.iter().any(|m| {
                m.kind == AnalyzedMacroKind::DefineEmits
                    && offset >= m.span.start
                    && offset <= m.span.end
            });
            (
                on_slot_element || on_define_slots,
                on_emit_usage || on_define_emits,
            )
        }
    };

    let mut actions = Vec::new();
    // The only capability the macro-augmentation flows get over the live
    // buffer: anchor → position. No `&str` reaches them.
    let edit_target = LiveEditTarget::new(source, line_index);

    // B1: Generate defineSlots (no existing defineSlots, template has <slot> tags)
    if show_slot_actions
        && !flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS)
        && !template.defined_slots.is_empty()
    {
        if let Some(action) = generate_define_slots_action(
            source,
            analysis,
            setup_block,
            template,
            line_index,
            &type_ctx,
        ) {
            actions.push(action);
        }
    }

    // B2: Generate defineEmits (no existing defineEmits, template has undeclared emits)
    if show_emit_actions && !flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
        let undeclared: Vec<&str> = template
            .emit_definitions
            .iter()
            .filter(|e| !e.is_declared)
            .map(|e| e.event_name.as_str())
            .collect();
        if !undeclared.is_empty() {
            if let Some(action) =
                generate_define_emits_action(source, analysis, setup_block, &undeclared, line_index)
            {
                actions.push(action);
            }
        }
    }

    // B3: Add missing slots to existing defineSlots
    if show_slot_actions && flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
        if let Some(action) = add_missing_slots_action(analysis, template, &edit_target, &type_ctx)
        {
            actions.push(action);
        }
    }

    // B5: Prop mismatch detection (missing props in defineSlots)
    if show_slot_actions && flags.contains(AnalysisFlags::HAS_DEFINE_SLOTS) {
        actions.extend(prop_mismatch_actions(
            analysis,
            template,
            &edit_target,
            &type_ctx,
        ));
    }

    // B4: Add missing emits to existing defineEmits
    if show_emit_actions && flags.contains(AnalysisFlags::HAS_DEFINE_EMITS) {
        if let Some(action) = add_missing_emits_action(analysis, template, &edit_target) {
            actions.push(action);
        }
    }

    actions
}

// ── B1: Generate defineSlots ─────────────────────────────────────────────

fn generate_define_slots_action(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &SfcBlock,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    line_index: &LineIndex,
    type_ctx: &SlotTypeContext<'_>,
) -> Option<CodeActionOrCommand> {
    let slots = &template.defined_slots;
    if slots.is_empty() {
        return None;
    }

    // Build the type literal for defineSlots
    let mut type_members = String::new();
    for slot in slots {
        type_members.push_str(&build_slot_member(slot, Some(type_ctx)));
    }

    let insert_text = format!("defineSlots<{{\n{}}}>()\n", type_members);
    let insert_offset = find_script_insert_offset(source, analysis, setup_block);
    let position = line_index.offset_to_position(insert_offset)?;

    Some(make_insert_action(
        "Generate defineSlots from template",
        CodeActionKind::QUICKFIX,
        &insert_text,
        position,
    ))
}

// ── B2: Generate defineEmits ─────────────────────────────────────────────

fn generate_define_emits_action(
    source: &str,
    analysis: &FileAnalysisSnapshot,
    setup_block: &SfcBlock,
    undeclared: &[&str],
    line_index: &LineIndex,
) -> Option<CodeActionOrCommand> {
    if undeclared.is_empty() {
        return None;
    }

    // Build type-based defineEmits
    let mut type_members = String::new();
    for event in undeclared {
        type_members.push_str("    (e: '");
        type_members.push_str(event);
        type_members.push_str("', ...args: any[]): void\n");
    }

    let insert_text = format!("const emit = defineEmits<{{\n{}}}>()\n", type_members);
    let insert_offset = find_script_insert_offset(source, analysis, setup_block);
    let position = line_index.offset_to_position(insert_offset)?;

    Some(make_insert_action(
        "Generate defineEmits from template usage",
        CodeActionKind::QUICKFIX,
        &insert_text,
        position,
    ))
}

// ── B3: Add missing slots to existing defineSlots ────────────────────────

/// Membership from `slot_fields`; placement from the macro's type-literal
/// anchor. An unsupported anchor (a bare type reference, an intersection, a
/// runtime macro) yields no action — never a fallback offset.
fn add_missing_slots_action(
    analysis: &FileAnalysisSnapshot,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    edit_target: &LiveEditTarget<'_>,
    type_ctx: &SlotTypeContext<'_>,
) -> Option<CodeActionOrCommand> {
    // Find the defineSlots macro
    let slots_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)?;

    // The macro's own authored member list must be appendable at a known
    // position before any member may be offered.
    let anchor = slots_macro.edit_anchors.type_literal.available()?;

    let missing: Vec<_> = template
        .defined_slots
        .iter()
        .filter(|s| !slots_macro.slot_fields.iter().any(|f| f.name == s.name))
        .collect();

    if missing.is_empty() {
        return None;
    }

    // Build the new members to insert
    let mut new_members = String::new();
    for slot in &missing {
        new_members.push_str(&build_slot_member(slot, Some(type_ctx)));
    }

    let position = edit_target.anchor_position(anchor)?;

    let title = if missing.len() == 1 {
        format!("Add slot '{}' to defineSlots", missing[0].name)
    } else {
        format!("Add {} missing slots to defineSlots", missing.len())
    };

    Some(make_insert_action(
        &title,
        CodeActionKind::QUICKFIX,
        &new_members,
        position,
    ))
}

// ── B5: Prop mismatch detection ──────────────────────────────────────────

/// Declared props come from `slot_field.bindings`; placement comes from that
/// slot's own `props_anchor`, so a prop can only ever land in the props object
/// of the slot it belongs to.
fn prop_mismatch_actions(
    analysis: &FileAnalysisSnapshot,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    edit_target: &LiveEditTarget<'_>,
    type_ctx: &SlotTypeContext<'_>,
) -> Vec<CodeActionOrCommand> {
    let mut actions = Vec::new();

    let slots_macro = match analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineSlots)
    {
        Some(m) => m,
        None => return actions,
    };

    // The slot members must be authored inline in this macro's own type
    // literal. When the member list lives elsewhere (a bare type reference, an
    // intersection) the macro is fail-closed for every augmentation.
    if !slots_macro.edit_anchors.type_literal.is_available() {
        return actions;
    }

    // For each template slot that also exists in defineSlots, compare props
    for template_slot in &template.defined_slots {
        let slot_field = match slots_macro
            .slot_fields
            .iter()
            .find(|f| f.name == template_slot.name)
        {
            Some(f) => f,
            None => continue, // slot not in defineSlots — handled by B3
        };

        // Missing props: template has `:prop` but the slot's declared bindings
        // do not carry it.
        let missing_props: Vec<&str> = template_slot
            .binding_names
            .iter()
            .filter(|name| !slot_field.bindings.iter().any(|b| &&b.name == name))
            .map(|s| s.as_str())
            .collect();

        if missing_props.is_empty() {
            continue;
        }

        // This slot's props surface must be an appendable member list. A
        // `Pick<…>` surface, or a slot declared without a props parameter, is a
        // typed miss: no action, never a neighbouring slot's object.
        let Some(anchor) = slot_field.props_anchor.available() else {
            continue;
        };
        let Some(position) = edit_target.anchor_position(anchor) else {
            continue;
        };

        // Build insertion text for missing props
        let mut new_props = String::new();
        for prop_name in &missing_props {
            if !new_props.is_empty() {
                new_props.push_str(", ");
            }
            new_props.push_str(prop_name);
            new_props.push_str(": ");
            // Resolve type from binding expression
            let expr = template_slot
                .binding_expressions
                .iter()
                .zip(template_slot.binding_names.iter())
                .find(|(_, name)| name.as_str() == *prop_name)
                .map(|(expr, _)| expr.as_str())
                .unwrap_or(prop_name);
            new_props.push_str(&resolve_binding_type(Some(type_ctx), expr));
        }

        let insert_text = if anchor.is_empty() {
            format!(" {} ", new_props)
        } else {
            format!(", {}", new_props)
        };

        let title = if missing_props.len() == 1 {
            format!(
                "Add prop '{}' to slot '{}' in defineSlots",
                missing_props[0], template_slot.name
            )
        } else {
            format!(
                "Add {} missing props to slot '{}' in defineSlots",
                missing_props.len(),
                template_slot.name
            )
        };

        actions.push(make_insert_action(
            &title,
            CodeActionKind::QUICKFIX,
            &insert_text,
            position,
        ));
    }

    actions
}

// ── B4: Add missing emits to existing defineEmits ────────────────────────

/// Type-based emits append to the type literal's anchor; runtime emits append to
/// the runtime ARRAY's element-list anchor. A runtime OBJECT argument has no
/// element list, so it is fail-closed rather than emitting invalid code.
fn add_missing_emits_action(
    analysis: &FileAnalysisSnapshot,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    edit_target: &LiveEditTarget<'_>,
) -> Option<CodeActionOrCommand> {
    // Find the defineEmits macro
    let emits_macro = analysis
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineEmits)?;

    // Find undeclared emits
    let undeclared: Vec<&str> = template
        .emit_definitions
        .iter()
        .filter(|e| !e.is_declared)
        .map(|e| e.event_name.as_str())
        .collect();

    if undeclared.is_empty() {
        return None;
    }

    let title = if undeclared.len() == 1 {
        format!("Add emit '{}' to defineEmits", undeclared[0])
    } else {
        format!("Add {} missing emits to defineEmits", undeclared.len())
    };

    if emits_macro.is_type_based {
        // Type-based: append new call signatures to the type literal's members.
        let anchor = emits_macro.edit_anchors.type_literal.available()?;
        let position = edit_target.anchor_position(anchor)?;

        let mut new_members = String::new();
        for event in &undeclared {
            new_members.push_str("    (e: '");
            new_members.push_str(event);
            new_members.push_str("', ...args: any[]): void\n");
        }

        Some(make_insert_action(
            &title,
            CodeActionKind::QUICKFIX,
            &new_members,
            position,
        ))
    } else {
        // Runtime array form: defineEmits(['existing', ...]) — append to the
        // array's element list.
        let anchor = emits_macro.edit_anchors.runtime_array.available()?;
        let position = edit_target.anchor_position(anchor)?;

        let new_entries: String = undeclared
            .iter()
            .enumerate()
            .map(|(index, event)| {
                if index == 0 && anchor.is_empty() {
                    format!("'{}'", event)
                } else {
                    format!(", '{}'", event)
                }
            })
            .collect();

        Some(make_insert_action(
            &title,
            CodeActionKind::QUICKFIX,
            &new_entries,
            position,
        ))
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────
// find_insert_offset, needs_quoting, make_insert_action are now in action_utils.rs

#[cfg(test)]
#[path = "macro_actions_tests.rs"]
mod macro_actions_tests;
