// Hover — binding name, kind, source location from verter_session analysis.
// Enhanced with full resolved type signature, JSDoc from TypeProvider.

use tower_lsp_server::ls_types::*;
use verter_session::framework::{
    ComponentContractAvailability, ComponentContractUnsupported, PublicParameter, PublicSlot,
    PublicTypeReference,
};
use verter_session::FileAnalysisSnapshot;
use verter_type_expr::{render_type_expr_display, PublicationResult, TypeExpr};

use crate::documents::carrier_structure::{
    classify_cursor, parse_opening_tag, CarrierBlockView, CarrierCursorContext,
};
use crate::documents::line_index::LineIndex;
use crate::features::hover_event_tokens::{
    camelize_event_name, capitalize_first, event_directive_hover, v_model_hover,
    vue_event_attr_label,
};

/// Hover result from verter's own analysis, optionally carrying a Vue-specific
/// kind label (e.g., "ref", "computed") to replace the type provider's generic kind prefix.
pub struct VerterHoverResult {
    pub hover: Hover,
    /// If set, replaces the type provider's `({kind})` prefix in the merged hover.
    pub vue_kind_label: Option<String>,
    /// Typed source-token provenance for this hover. When the hover describes a
    /// Vue template syntax token whose generated TSX counterpart differs (e.g. an
    /// `@event` directive lowered to `onEvent`), this carries the structured source
    /// identity so the merge layer can rewrite a paired TypeProvider hover back to
    /// the source token — WITHOUT reparsing rendered hover markdown.
    pub source_token: Option<HoverSourceToken>,
}

/// Structured provenance for a Vue template hover whose generated TSX token differs
/// from the source token the user wrote.
///
/// This is the typed channel the merge layer reads to decide whether (and how) to
/// rewrite a TypeProvider hover label back to Vue source syntax. The label travels
/// through this typed channel, never through the rendered hover text: display text
/// is display-only and is never reparsed for a backticked `@event` label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoverSourceToken {
    /// A `v-on` / `@event` directive token. `vue_attr` is the canonical Vue
    /// attribute label (`@click`, `@update:model-value`) that should replace the
    /// generated `onClick` prop label in a merged TypeProvider hover.
    EventDirective { vue_attr: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChildHoverTarget {
    ComponentTag(ComponentTagHoverTarget),
    ImportBinding(ImportBindingHoverTarget),
    EventAttribute(ComponentEventHoverTarget),
    SlotAttribute(SlotAttributeHoverTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentTagHoverTarget {
    pub component_name: String,
    pub import_source: String,
    pub usage_props: Vec<ComponentUsagePropInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportBindingHoverTarget {
    pub binding_name: String,
    pub import_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentEventHoverTarget {
    pub component_name: String,
    pub import_source: String,
    pub event_name: String,
    pub vue_attr: String,
}

/// A slot-name token on a component slot usage (`#header`, `v-slot:header`,
/// `#default`, kebab `#my-slot`, or arg-less `v-slot` = default) whose typed
/// hover comes from the CHILD's declared slots surface (defineSlots fields /
/// template defined slots) — never from the generated TSX, where the authored
/// name token lowers to a semantically dead string literal (D3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotAttributeHoverTarget {
    pub component_name: String,
    pub import_source: String,
    /// Authored slot argument (`my-slot` as written; `default` when arg-less).
    pub slot_name: String,
    /// Display label in the authored spelling (`#my-slot` / `v-slot:my-slot`).
    pub vue_attr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentUsagePropInfo {
    pub name: String,
    pub is_bound: bool,
    pub constness: verter_semantic::analysis::template::PropValueConstness,
}

impl From<Hover> for VerterHoverResult {
    fn from(hover: Hover) -> Self {
        Self {
            hover,
            vue_kind_label: None,
            source_token: None,
        }
    }
}

/// Attempt to provide hover information at a given position.
///
/// Strategy:
/// 1. Classify cursor context (opening tag, closing tag, content, root level)
/// 2. For SFC tags: show documentation for tag names and attributes
/// 3. For block content: look up bindings, imports, macros from analysis
pub fn hover_at_position(
    position: &Position,
    source: &str,
    blocks: &[CarrierBlockView],
    analysis: Option<&FileAnalysisSnapshot>,
    line_index: &LineIndex,
    ssr_context: bool,
) -> Option<VerterHoverResult> {
    let offset = line_index.position_to_offset(position)?;

    // Check SFC structural context BEFORE requiring analysis data.
    // This allows hover on tags even when analysis hasn't completed.
    match classify_cursor(offset, blocks) {
        CarrierCursorContext::OpeningTag { block } => {
            if let Some(hover) = sfc_tag_hover(source, &block, offset) {
                return Some(hover.into());
            }
            // A phantom custom block from the depth-ignorant scanner (an
            // ordinary element like `<b v-my-thing>` after a nested
            // `</template>` closed the real template block): the SFC attr
            // table has nothing for it, but the typed element tree still
            // owns the position as template markup (D6).
            if let Some(template) = analysis.and_then(|a| a.template.as_deref()) {
                if template
                    .elements
                    .iter()
                    .any(|el| offset >= el.span.start && offset < el.span.end)
                {
                    return hover_in_template(
                        offset as usize,
                        source,
                        analysis.unwrap(),
                        line_index,
                    );
                }
            }
            // Carrier markup class tokens (Svelte root markup scanned as a
            // custom block): show the declaring rule(s), fail closed otherwise.
            if let Some(analysis) = analysis {
                if let Some(token) =
                    crate::features::references::markup_class_token_at(offset as usize, analysis)
                {
                    return class_css_rule_hover(&token.name.clone(), None, source, analysis)
                        .map(Into::into);
                }
            }
            return None;
        }
        CarrierCursorContext::ClosingTag { block } => {
            return sfc_tag_name_hover(&block.tag_name).map(|h| h.into());
        }
        CarrierCursorContext::RootLevel => {
            // Vue template markup the SFC scanner can leave in a dead zone on
            // MALFORMED input: with the outer close missing, the scanner's
            // depth-balanced walk fails closed to the first-close boundary, so
            // markup after a nested `</template>` falls outside the block. The
            // typed element tree is the authority for what is still template
            // markup (D6); Svelte has no element IR here.
            if let Some(template) = analysis.and_then(|a| a.template.as_deref()) {
                if template
                    .elements
                    .iter()
                    .any(|el| offset >= el.span.start && offset < el.span.end)
                {
                    return hover_in_template(
                        offset as usize,
                        source,
                        analysis.unwrap(),
                        line_index,
                    );
                }
            }
            // Carrier markup class tokens (Svelte root markup — no template
            // element IR): show the declaring rule(s), fail closed otherwise.
            if let Some(analysis) = analysis {
                if let Some(token) =
                    crate::features::references::markup_class_token_at(offset as usize, analysis)
                {
                    return class_css_rule_hover(&token.name.clone(), None, source, analysis)
                        .map(Into::into);
                }
            }
            return None;
        }
        CarrierCursorContext::BlockContent { .. } => {} // fall through to analysis-based hover
    }

    let analysis = analysis?;
    let offset = offset as usize;

    // Determine which block the cursor is in
    let block = blocks.iter().find(|b| {
        let (content_start, content_end) = b.content_range();
        offset >= content_start as usize && offset < content_end as usize
    });

    match block {
        Some(b) => match b.tag_name.as_str() {
            "script" => hover_in_script(offset, source, analysis, ssr_context),
            "template" => hover_in_template(offset, source, analysis, line_index),
            "style" => crate::css::css_hover(position, source, blocks, Some(analysis), line_index)
                .map(|h| h.into()),
            _ => {
                // A phantom custom block from the depth-ignorant scanner (a
                // component usage after a nested `</template>` closed the real
                // template block). Its interior is still template markup when
                // the typed element tree owns the offset (D6).
                let template = analysis.template.as_deref()?;
                if template
                    .elements
                    .iter()
                    .any(|el| offset >= el.span.start as usize && offset < el.span.end as usize)
                {
                    hover_in_template(offset, source, analysis, line_index)
                } else {
                    None
                }
            }
        },
        None => {
            // No scanned block owns the offset — same dead-zone recovery.
            let template = analysis.template.as_deref()?;
            if template
                .elements
                .iter()
                .any(|el| offset >= el.span.start as usize && offset < el.span.end as usize)
            {
                hover_in_template(offset, source, analysis, line_index)
            } else {
                None
            }
        }
    }
}

// ── SFC Tag Hover ────────────────────────────────────────────────────────────

/// Hover on an SFC opening tag — dispatches to tag name or attribute hover.
/// Returns `None` for cursor inside `generic`/`attrs`/`attributes` attribute values
/// on script tags, letting the TypeProvider handle those via sourcemapped TSX positions.
fn sfc_tag_hover(source: &str, block: &CarrierBlockView, offset: u32) -> Option<Hover> {
    let ctx = parse_opening_tag(source, block);

    // Check if cursor is on the tag name
    if offset >= ctx.tag_name_start && offset < ctx.tag_name_end {
        return sfc_tag_name_hover(&ctx.tag_name);
    }

    // Check if cursor is on an attribute name or value
    for attr in &ctx.attrs {
        if offset >= attr.name_start && offset < attr.name_end {
            return sfc_attr_hover(&ctx.tag_name, &attr.name);
        }
        if let (Some(vs), Some(ve)) = (attr.value_start, attr.value_end) {
            if offset >= vs && offset < ve {
                // For generic/attrs/attributes values on script tags, return None
                // to delegate to TypeProvider for TS intellisense.
                if block.tag_name == "script"
                    && matches!(attr.name.as_str(), "generic" | "attrs" | "attributes")
                {
                    return None;
                }
                return sfc_attr_hover(&ctx.tag_name, &attr.name);
            }
        }
    }

    None
}

/// Hover documentation for SFC tag names.
fn sfc_tag_name_hover(tag_name: &str) -> Option<Hover> {
    let doc = match tag_name {
        "script" => "**`<script>`** — JavaScript/TypeScript logic block.\n\nContains component logic, imports, and exports. Use `setup` attribute for Composition API.",
        "template" => "**`<template>`** — HTML template block.\n\nContains the component's template markup with Vue directives and expressions.",
        "style" => "**`<style>`** — CSS style block.\n\nContains component styles. Use `scoped` for component-scoped CSS, `module` for CSS modules.",
        _ => return Some(make_hover(format!("**`<{tag_name}>`** — Custom block."))),
    };
    Some(make_hover(doc.to_string()))
}

/// Hover documentation for SFC tag attributes.
fn sfc_attr_hover(tag_name: &str, attr_name: &str) -> Option<Hover> {
    let doc = match (tag_name, attr_name) {
        // script attributes
        ("script", "setup") => "**`setup`** — Enables `<script setup>` syntax.\n\nAll top-level bindings are automatically exposed to the template. Compiler macros like `defineProps()` and `defineEmits()` are available.",
        ("script", "lang") => "**`lang`** — Script language.\n\nValues: `ts` (TypeScript), `tsx`, `jsx`. Defaults to JavaScript.",
        ("script", "generic") => "**`generic`** — Generic type parameters for `<script setup>`.\n\nDefines component-level generics: `<script setup generic=\"T extends object\">`.",
        ("script", "attrs" | "attributes") => "**`attrs`** — Typed `$attrs` declaration.\n\nDefines the type of `$attrs` / `useAttrs()` return value:\n```vue\n<script setup attrs=\"{ class?: string }\">\n```",
        ("script", "src") => "**`src`** — External script source.\n\nLoad script content from an external file: `<script src=\"./script.ts\">`.",
        // template attributes
        ("template", "lang") => "**`lang`** — Template language.\n\nValues: `pug`. Defaults to HTML.",
        // style attributes
        ("style", "scoped") => "**`scoped`** — Component-scoped CSS.\n\nStyles only apply to the current component via automatically added data attributes.",
        ("style", "module") => "**`module`** — CSS Modules.\n\nCSS classes are exposed as `$style` object. Named modules: `<style module=\"classes\">`.",
        ("style", "lang") => "**`lang`** — Style language.\n\nValues: `scss`, `sass`, `less`, `stylus`. Defaults to CSS.",
        // common
        (_, "lang") => "**`lang`** — Block language preprocessor.",
        _ => return None,
    };
    Some(make_hover(doc.to_string()))
}

fn make_hover(value: String) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: None,
    }
}

pub fn hover_text(hover: &Hover) -> String {
    match &hover.contents {
        HoverContents::Markup(markup) => markup.value.clone(),
        HoverContents::Scalar(MarkedString::String(text)) => text.clone(),
        HoverContents::Scalar(MarkedString::LanguageString(text)) => text.value.clone(),
        HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(text) => text.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn child_hover_target_at_offset(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<ChildHoverTarget> {
    if let Some(template) = analysis.template.as_deref() {
        if let Some(target) = component_event_hover_target(offset, template, analysis) {
            return Some(ChildHoverTarget::EventAttribute(target));
        }

        if let Some(target) = slot_attribute_hover_target(offset, template, analysis) {
            return Some(ChildHoverTarget::SlotAttribute(target));
        }

        if let Some(target) = component_tag_hover_target(offset, source, template, analysis) {
            return Some(ChildHoverTarget::ComponentTag(target));
        }
    }

    direct_import_binding_hover_target(offset, analysis).map(ChildHoverTarget::ImportBinding)
}

pub fn build_child_component_hover(
    component_name: &str,
    import_source: &str,
    availability: &ComponentContractAvailability,
    usage_props: &[ComponentUsagePropInfo],
) -> Hover {
    let mut lines = vec![format!("**`<{component_name}>`** (from `{import_source}`)")];
    let ComponentContractAvailability::Supported(contract) = availability else {
        let ComponentContractAvailability::Unsupported(unsupported) = availability else {
            unreachable!("component contract availability is closed")
        };
        lines.push(String::new());
        lines.push(render_contract_unsupported(unsupported));
        return make_hover(lines.join("\n"));
    };

    if !contract.props.is_empty() {
        lines.push(String::new());
        lines.push("**Props:**".to_string());
        lines.extend(contract.props.iter().map(|prop| {
            let optional_marker = if prop.optional { "?" } else { "" };
            format!(
                "- `{}`{}: {}",
                prop.name,
                optional_marker,
                render_public_type(&prop.ty)
            )
        }));
    }

    if !contract.events.is_empty() {
        lines.push(String::new());
        lines.push("**Emits:**".to_string());
        for event in contract.events.iter() {
            for signature in event.derived_handler.overloads.iter() {
                lines.push(format!(
                    "- `{}`{}",
                    event_summary_name(&event.name),
                    render_handler_signature(&signature.parameters, &signature.return_type)
                ));
            }
        }
    }

    if !contract.slots.is_empty() {
        lines.push(String::new());
        lines.push("**Slots:**".to_string());
        // The component-tag summary lists EVERY slot and deepens NONE
        // (path-precision: only a slot-name hover's rank-matched slot makes
        // a deepen demand).
        let no_deepen = crate::features::hover_slot_deepen::SlotBindingDeepenView::default();
        lines.extend(contract.slots.iter().map(|slot| {
            let optional_marker = if slot.optional { "?" } else { "" };
            format!(
                "- `{}`{}{}",
                slot.name,
                optional_marker,
                render_slot_signature(slot, &no_deepen)
            )
        }));
    }

    if !usage_props.is_empty() {
        lines.push(String::new());
        lines.push("**Usage:**".to_string());
        for prop in usage_props {
            let constness_label = match prop.constness {
                verter_semantic::analysis::template::PropValueConstness::Const => "const",
                verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic",
                verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown",
            };
            let icon = match prop.constness {
                verter_semantic::analysis::template::PropValueConstness::Const => "\u{2713}",
                verter_semantic::analysis::template::PropValueConstness::Dynamic => "\u{2197}",
                verter_semantic::analysis::template::PropValueConstness::Unknown => "?",
            };
            let bound = if prop.is_bound { ":" } else { "" };
            lines.push(format!(
                "- {icon} `{bound}{}` — *{constness_label}*",
                prop.name
            ));
        }
    }

    make_hover(lines.join("\n"))
}

pub fn build_child_event_hover(
    vue_attr: &str,
    availability: &ComponentContractAvailability,
) -> Option<Hover> {
    let ComponentContractAvailability::Supported(contract) = availability else {
        let ComponentContractAvailability::Unsupported(unsupported) = availability else {
            unreachable!("component contract availability is closed")
        };
        return Some(make_hover(render_contract_unsupported(unsupported)));
    };
    // Declared emits outrank the prop join: the CREO-projected events lane is
    // the primary public-event authority.
    if let Some(event) = contract
        .events
        .iter()
        .find(|event| public_event_matches_event_attr(&event.name, vue_attr))
    {
        let signatures = event
            .derived_handler
            .overloads
            .iter()
            .map(|signature| {
                format!(
                    "{vue_attr}{}",
                    render_handler_signature(&signature.parameters, &signature.return_type)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Some(make_hover(format!("```typescript\n{signatures}\n```")));
    }
    // Prop-backed event arm: a child may accept a listener as an `onX`
    // handler PROP (`onAlert?: (payload: string) => void`) instead of a
    // declared emit. The join is typed end-to-end — the JSX name maps to the
    // hovered attr through the sanctioned attr mapper AND the published type
    // is structurally function-shaped in typed IR (never name-only, no `on*`
    // suffix sniffing).
    let prop = contract.props.iter().find(|prop| {
        crate::type_provider::merge::jsx_prop_to_vue_attr(&prop.name).as_deref() == Some(vue_attr)
            && matches!(
                prop.ty.publication.materialized_type(),
                Some(TypeExpr::Function(_))
            )
    })?;
    let handler = prop.ty.publication.materialized_type()?;
    let optional_marker = if prop.optional { "?" } else { "" };
    Some(make_hover(format!(
        "```typescript\n{vue_attr}{optional_marker}: {}\n```",
        render_structured_type(handler)
    )))
}

fn render_public_type(reference: &PublicTypeReference) -> String {
    if let Some(expression) = reference.publication.materialized_type() {
        return render_structured_type(expression);
    }
    match reference.publication.publication() {
        PublicationResult::Absent { absence, .. } => {
            format!("<type absent: {absence:?}>")
        }
        PublicationResult::Failed { failure, .. } => {
            format!("<type publication failed: {failure:?}>")
        }
        PublicationResult::Published { .. } => "<published type unavailable>".to_string(),
    }
}

fn render_structured_type(expression: &TypeExpr) -> String {
    render_type_expr_display(expression)
        .map(|rendered| rendered.text)
        .unwrap_or_else(|error| format!("<structured type cannot be displayed: {error}>"))
}

fn render_parameter(parameter: &PublicParameter, index: usize) -> String {
    let name = parameter
        .name
        .as_deref()
        .map(str::to_string)
        .unwrap_or_else(|| format!("_arg{index}"));
    let rest = if parameter.rest { "..." } else { "" };
    let optional = if parameter.optional { "?" } else { "" };
    format!(
        "{rest}{name}{optional}: {}",
        render_structured_type(&parameter.ty)
    )
}

fn render_parameters(parameters: &[PublicParameter]) -> String {
    parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| render_parameter(parameter, index))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_handler_signature(parameters: &[PublicParameter], return_type: &TypeExpr) -> String {
    format!(
        "({}) => {}",
        render_parameters(parameters),
        render_structured_type(return_type)
    )
}

fn event_summary_name(event_name: &str) -> String {
    vue_event_attr_label(event_name)
        .trim_start_matches('@')
        .to_string()
}

fn public_event_matches_event_attr(event_name: &str, vue_attr: &str) -> bool {
    if vue_event_attr_label(event_name) == vue_attr {
        return true;
    }
    let listener_prop = format!("on{}", capitalize_first(&camelize_event_name(event_name)));
    crate::type_provider::merge::jsx_prop_to_vue_attr(&listener_prop).as_deref() == Some(vue_attr)
}

/// Render one slot's signature. `deepened` supplies demand-resolved views
/// for THIS slot's carrier bindings (empty for surfaces that made no deepen
/// demand — e.g. the component-tag summary, which lists every slot and stays
/// path-precise by never deepening any).
fn render_slot_signature(
    slot: &PublicSlot,
    deepened: &crate::features::hover_slot_deepen::SlotBindingDeepenView,
) -> String {
    let input = if slot.input.bindings.is_empty() {
        "()".to_string()
    } else {
        let bindings = slot
            .input
            .bindings
            .iter()
            .map(|binding| match deepened.deepened(&binding.name) {
                Some(view) => format!("{}: {}", binding.name, render_structured_type(view)),
                None => format!("{}: {}", binding.name, render_public_type(&binding.ty)),
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("(props: {{ {bindings} }})")
    };
    match slot.return_type.as_ref() {
        Some(return_type) => format!("{input}: {}", render_public_type(return_type)),
        None => input,
    }
}

fn render_contract_unsupported(unsupported: &ComponentContractUnsupported) -> String {
    let mut message = format!(
        "**Component contract unavailable:** `{:?}` (`{}`)",
        unsupported.reason,
        unsupported.adapter_id.as_str()
    );
    for diagnostic in unsupported.diagnostics.iter() {
        message.push_str(&format!(
            "\n\n- `{:?}`: {}",
            diagnostic.kind, diagnostic.context
        ));
    }
    message
}

fn hover_in_script(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    ssr_context: bool,
) -> Option<VerterHoverResult> {
    // Check if the cursor is on a Vue API call site — add context if so
    if let Some(api_hover) = vue_api_hover_at_offset(offset as u32, analysis, ssr_context) {
        return Some(api_hover.into());
    }

    let word = word_at_offset(source, offset)?;
    hover_for_word(&word, analysis)
}

fn direct_import_binding_hover_target(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<ImportBindingHoverTarget> {
    for import in &analysis.imports {
        if import.is_type_only {
            continue;
        }
        if let Some(binding) = import
            .bindings
            .iter()
            .find(|binding| offset >= binding.span.start && offset < binding.span.end)
        {
            return Some(ImportBindingHoverTarget {
                binding_name: binding.name.clone(),
                import_source: import.source.clone(),
            });
        }
    }
    None
}

fn hover_in_template(
    offset: usize,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    // Don't provide hover inside HTML comments
    if crate::features::definition::is_inside_html_comment(source, offset) {
        return None;
    }

    // Check if cursor is on a slot outlet element — show slot name and props
    if let Some(hover) = slot_outlet_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Check if cursor is on a v-slot / #name directive NAME+ARG (slot consumer).
    // Pattern positions inside the destructure are deliberately NOT answered
    // here: they map into the generated TSX and the provider supplies the
    // typed binding quickinfo (D4).
    if let Some(hover) = v_slot_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Slot destructure-pattern positions yield no Verter-native hover — the
    // provider answers them with the typed binding quickinfo through the
    // mapped pattern bytes (D4). This also prevents a pattern key that
    // collides with a same-named script binding from showing the WRONG
    // script-binding hover.
    if is_inside_slot_pattern(offset as u32, analysis) {
        return None;
    }

    // Check if cursor is on a template element tag name — show matching CSS rules
    if let Some(hover) = element_css_hover(offset as u32, analysis) {
        return Some(hover.into());
    }

    // Class-token hover: cursor on a `class="x"` entry / resolvable `:class`
    // entry — show the declaring CSS rule(s). A recognized class token FAILS
    // CLOSED when no rule declares it (no link, no same-named-binding hover).
    if let Some(template) = analysis.template.as_deref() {
        if let Some((crate::features::references::CssRefTarget::Class(name), element_idx)) =
            crate::features::references::find_css_target_in_template_refs_with_element(
                offset, source, template,
            )
        {
            return class_css_rule_hover(&name, Some((element_idx, template)), source, analysis)
                .map(Into::into);
        }
    }

    // Check if cursor is on a component element tag name — show prop constness info
    if let Some(hover) = component_prop_constness_hover(offset as u32, source, analysis) {
        return Some(hover.into());
    }

    // Source-owned Vue template syntax hovers. These run BEFORE the attribute-name
    // suppression below because event directives, modifiers, and the static `ref`
    // attribute are Vue syntax tokens — not script bindings. The generated TSX either
    // renames them (`@click` → `onClick`) or deletes them outright (modifiers,
    // no-value directives, static `ref`), so the TypeProvider can never describe the
    // source token. We reconstruct the hover label/range from the existing
    // `TemplateDirective` / `TemplateAttribute` spans instead.
    if let Some(hover) = event_directive_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }
    // Source-owned `v-model` directive-name + arg hover. Runs BEFORE the
    // attribute-name suppression: the `v-model` name and its `:show` arg are Vue
    // syntax tokens the generated TSX renames/overwrites, so the TypeProvider can
    // never describe the source token. TSGO supplies the bound prop TYPE (via the
    // mapped prop-name codegen); this hover supplies the Vue source context.
    if let Some(hover) = v_model_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }
    if let Some(hover) = template_ref_hover(offset as u32, source, analysis, line_index) {
        return Some(hover);
    }

    // D6: directive-NAME tokens. Built-ins get Volar-style doc hovers; custom
    // directives (`v-my-thing` → `vMyThing`) get the resolved binding's typed
    // hover. Both run BEFORE the attribute-name suppression below — the
    // generated TSX erases/lowers directive names, so the provider can never
    // describe the authored token.
    if let Some(hover) = crate::features::hover_directive_names::builtin_directive_name_hover(
        offset as u32,
        analysis,
    ) {
        return Some(hover);
    }
    if let Some(hover) =
        crate::features::hover_directive_names::custom_directive_name_hover(offset as u32, analysis)
    {
        return Some(hover);
    }

    // In template, look for bindings used in expressions like {{ myVar }}
    let word = word_at_offset(source, offset)?;

    // Guard: don't show binding/import hover for attribute names (e.g., `ref` attr
    // should not show Vue `ref()` import info). Let TSGO handle attribute names.
    if is_on_attribute_name(offset as u32, analysis) {
        return None;
    }

    hover_for_word(&word, analysis)
}

/// Build an LSP range from a source byte span via the document line index.
pub(super) fn span_to_range(line_index: &LineIndex, start: u32, end: u32) -> Option<Range> {
    Some(Range {
        start: line_index.offset_to_position(start)?,
        end: line_index.offset_to_position(end)?,
    })
}

/// Source-owned hover for a static template `ref` attribute (`<span ref="el">`).
///
/// The IDE codegen deletes the static `ref="..."` and reinserts an unmapped
/// synthetic `ref={"..."}`, and the generic attribute-name suppression would
/// otherwise route this through the imported Vue `ref()` symbol. We return a hover
/// describing the template-ref declaration from the existing `TemplateAttribute`
/// facts instead, without surfacing the import.
fn template_ref_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
    line_index: &LineIndex,
) -> Option<VerterHoverResult> {
    let template = analysis.template.as_deref()?;
    for el in &template.elements {
        for attr in &el.attributes {
            if attr.name != "ref" || attr.is_dynamic {
                continue;
            }
            if offset < attr.span.start || offset >= attr.span.end {
                continue;
            }
            let token = source
                .get(attr.span.start as usize..attr.span.end as usize)
                .unwrap_or("ref");
            let mut value = format!("`{token}`\n\nTemplate ref");
            match attr.value.as_deref() {
                Some(name) if !name.is_empty() => {
                    value.push_str(&format!(" — registers a reference named `{name}`."));
                }
                _ => value.push('.'),
            }
            return Some(VerterHoverResult {
                hover: Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: span_to_range(line_index, attr.span.start, attr.span.end),
                },
                vue_kind_label: None,
                source_token: None,
            });
        }
    }
    None
}

/// Check if the given offset falls on an attribute name or directive name in the template.
fn is_on_attribute_name(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    if let Some(template) = analysis.template.as_deref() {
        for el in &template.elements {
            for attr in &el.attributes {
                if offset >= attr.span.start && offset < attr.name_end {
                    return true;
                }
            }
            for dir in &el.directives {
                if offset >= dir.span.start && offset < dir.name_end {
                    return true;
                }
            }
        }
    }
    false
}

/// When the cursor is on a static `class` or `style` attribute that has been merged
/// with a dynamic `:class`/`:style` binding, the static attribute is removed from
/// the generated TSX and TSGO can't provide hover at that position.
///
/// This function detects that case and returns the corresponding directive's argument
/// start position (the `class` in `:class`) — which IS mapped in the generated TSX
/// and can be used to redirect the TSGO hover query.
pub fn merged_attribute_redirect_offset(
    offset: u32,
    analysis: &FileAnalysisSnapshot,
) -> Option<u32> {
    let template = analysis.template.as_deref()?;

    for el in &template.elements {
        // Check if cursor is on a static `class` or `style` attribute
        for attr in &el.attributes {
            if offset >= attr.span.start && offset < attr.name_end {
                let attr_name = &attr.name;
                if attr_name == "class" || attr_name == "style" {
                    // Check if this element also has a dynamic `:class` or `:style`
                    for dir in &el.directives {
                        if dir.name == "bind" && dir.argument.as_deref() == Some(attr_name.as_str())
                        {
                            // Return the directive argument start (the `class` in `:class`)
                            // which is mapped in TSX to the merged `class={normalizeClass(...)}`
                            if let Some(ref arg_span) = dir.arg_span {
                                return Some(arg_span.start);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Check if the given SFC offset is on slot-related syntax (outlet or v-slot directive).
/// The type provider returns unhelpful `() any`/`string` for these positions.
///
/// Scoped to the slot NAME+ARG region: destructure-pattern positions are
/// answered by the provider through the mapped pattern bytes (D4), so they are
/// NOT slot syntax for merge-suppression purposes.
pub fn is_on_slot_syntax(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    let Some(template) = analysis.template.as_deref() else {
        return false;
    };

    // Slot outlets: <slot name="x" />
    if template
        .elements
        .iter()
        .any(|el| el.tag == "slot" && offset >= el.span.start && offset < el.tag_span_end)
    {
        return true;
    }

    // v-slot / #name directive NAME+ARG region (never the pattern expression)
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset >= region_start && offset < region_end {
                return true;
            }
        }
    }
    false
}

/// Whether the offset sits inside a `v-slot` / `#name` destructure-pattern
/// expression span (D4 — provider-answered positions).
fn is_inside_slot_pattern(offset: u32, analysis: &FileAnalysisSnapshot) -> bool {
    let Some(template) = analysis.template.as_deref() else {
        return false;
    };
    template.elements.iter().any(|el| {
        el.directives.iter().any(|dir| {
            dir.name == "slot"
                && dir
                    .expression_span
                    .as_ref()
                    .is_some_and(|span| offset >= span.start && offset < span.end)
        })
    })
}

/// When hovering on a `<slot>` element (tag name, `name` attribute, or its value),
/// show slot outlet information: the slot name and any scoped props being passed.
///
/// Uses `DefinedSlot` from template analysis for richer binding info.
fn slot_outlet_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    // Find a <slot> element where the cursor is within the opening tag range.
    // span.start = `<`, tag_span_end = end of opening tag (after `>` or `/>`)
    let element = template
        .elements
        .iter()
        .find(|el| el.tag == "slot" && offset >= el.span.start && offset < el.tag_span_end)?;

    // Extract slot name from `name="..."` attribute
    let slot_name = element
        .attributes
        .iter()
        .find(|a| a.name == "name" && !a.is_dynamic)
        .and_then(|a| a.value.as_deref())
        .unwrap_or("default");

    let mut lines = Vec::new();
    lines.push(format!("**`<slot>`** outlet — **\"{slot_name}\"**"));
    lines.push("Renders content provided by the parent component.".to_string());

    // Use DefinedSlot for pre-extracted binding info
    if let Some(def) = template.defined_slots.iter().find(|s| s.name == slot_name) {
        if def.has_bindings && !def.binding_names.is_empty() {
            let props: Vec<String> = def
                .binding_names
                .iter()
                .zip(def.binding_expressions.iter())
                .map(|(name, expr)| format!(":{name}=\"{expr}\""))
                .collect();
            lines.push(format!("\n**Scoped props:** {}", props.join(", ")));
        }
    }

    Some(make_hover(lines.join("\n\n")))
}

/// When hovering on a `v-slot` / `#name` directive NAME+ARG, show slot content
/// information. This is the fallback surface for slots whose child component
/// cannot be resolved; a resolvable child is answered by the typed
/// [`build_child_slot_hover`] from the child's declared slots surface (D3).
/// Destructure-pattern positions are excluded — the provider answers them
/// with typed binding quickinfo through the mapped pattern bytes (D4).
fn v_slot_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }

            let slot_name = dir.argument.as_deref().unwrap_or("default");
            let mut lines = Vec::new();

            let syntax = if dir.raw_name.starts_with('#') {
                format!("`#{slot_name}`")
            } else if dir.argument.is_some() {
                format!("`v-slot:{slot_name}`")
            } else {
                "`v-slot`".to_string()
            };
            lines.push(format!("**Slot content** — {syntax}"));
            lines.push(format!(
                "Provides content for the **\"{slot_name}\"** slot."
            ));

            if let Some(ref expr) = dir.expression {
                lines.push(format!("\n**Scoped params:** `{expr}`"));
            }

            return Some(make_hover(lines.join("\n\n")));
        }
    }
    None
}

/// Hover for a markup class token: render every CSS rule declaring the class
/// (Volar-style `selector { declarations }` blocks), hierarchy-ranked against
/// the origin element. Returns `None` when no rule declares the class.
pub(crate) fn class_css_rule_hover(
    name: &str,
    element: Option<(
        usize,
        &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    )>,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    use crate::features::definition::class_rule_match_rank;

    // (rank, source order, rendered rule)
    let mut entries: Vec<(u8, u32, String)> = Vec::new();
    let mut seen: std::collections::HashSet<(usize, u32)> = std::collections::HashSet::new();

    for (style_idx, style) in analysis.styles.iter().enumerate() {
        let Some(css) = style.css.as_ref() else {
            continue;
        };
        for cls in &css.classes {
            if cls.name != name || cls.span.start == 0 {
                continue;
            }
            // Module-block rules are hashed-local: never rendered for a
            // plain class token (fail closed).
            if !crate::css::global_classes::class_plain_addressable(style, cls.span) {
                continue;
            }
            let Some(si) = cls.selector_index else {
                continue;
            };
            let Some(selector) = css.selectors.get(si as usize) else {
                continue;
            };
            if !seen.insert((style_idx, si)) {
                continue;
            }
            let rank = class_rule_match_rank(cls, css, element);
            let body = selector
                .rule_body_span
                .and_then(|b| source.get(b.start as usize..b.end as usize))
                .map(render_rule_body)
                .unwrap_or_else(|| "{ … }".to_string());
            let scope_label = if style.scoped { " (scoped)" } else { "" };
            let rendered = format!("```css\n{} {}\n```{}", selector.text, body, scope_label);
            entries.push((rank, cls.span.start, rendered));
        }
    }

    if entries.is_empty() {
        return None;
    }

    entries.sort_by_key(|e| (e.0, e.1));
    const MAX_RULES: usize = 4;
    let total = entries.len();
    let mut parts: Vec<String> = entries
        .into_iter()
        .take(MAX_RULES)
        .map(|(_, _, md)| md)
        .collect();
    if total > MAX_RULES {
        parts.push(format!("…and {} more rule(s)", total - MAX_RULES));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: parts.join("\n\n"),
        }),
        range: None,
    })
}

/// Render a rule's brace-inclusive declaration block for hover display,
/// truncating oversized bodies.
fn render_rule_body(body: &str) -> String {
    const MAX_BODY: usize = 400;
    let trimmed = body.trim();
    if trimmed.len() <= MAX_BODY {
        return trimmed.to_string();
    }
    let mut cut = MAX_BODY;
    while cut > 0 && !trimmed.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n  /* … */ }}", &trimmed[..cut])
}

fn element_css_hover(offset: u32, analysis: &FileAnalysisSnapshot) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    // Find element at cursor position — match only the tag name, not the full element span.
    // Element span starts at '<', so tag name starts at span.start + 1.
    let (el_idx, element) = template.elements.iter().enumerate().find(|(_, el)| {
        let tag_start = el.span.start + 1; // skip '<'
        let tag_end = tag_start + el.tag.len() as u32;
        offset >= tag_start && offset < tag_end
    })?;

    // Collect matching selectors from all style blocks
    let mut matches: Vec<(
        &str,
        (u32, u32, u32),
        verter_semantic::analysis::MatchResult,
    )> = Vec::new();

    for style in analysis.styles.iter() {
        if let Some(css) = &style.css {
            for sel in &css.selectors {
                if let Some(ref structure) = sel.structure {
                    let result = verter_semantic::analysis::match_selector(
                        structure,
                        el_idx,
                        &template.elements,
                    );
                    if !matches!(result, verter_semantic::analysis::MatchResult::NoMatch) {
                        matches.push((&sel.text, sel.specificity, result));
                    }
                }
            }
        }
    }

    if matches.is_empty() {
        return None;
    }

    // Sort by specificity (highest first)
    matches.sort_by_key(|m| std::cmp::Reverse(m.1));

    let classes: Vec<&str> = element.static_classes().collect();
    let class_info = if classes.is_empty() {
        String::new()
    } else {
        format!(" class=\"{}\"", classes.join(" "))
    };
    let id_info = element
        .static_id()
        .map(|id| format!(" id=\"{id}\""))
        .unwrap_or_default();

    let mut lines = Vec::new();
    lines.push(format!(
        "**`<{}{id_info}{class_info}>`**\n\n**CSS rules ({}):**",
        element.tag,
        matches.len()
    ));

    for (text, spec, result) in &matches {
        let certainty = match result {
            verter_semantic::analysis::MatchResult::Matches => "",
            verter_semantic::analysis::MatchResult::MaybeMatches => " *(maybe)*",
            verter_semantic::analysis::MatchResult::NoMatch => unreachable!(),
        };
        lines.push(format!(
            "- `{}` — specificity `({}, {}, {})`{certainty}",
            text, spec.0, spec.1, spec.2,
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

/// When hovering on a component element tag name in the template, show prop constness info.
///
/// This helps visualize cross-file optimization: which props are always const
/// (optimizable) vs dynamic (require reactive tracking).
fn component_prop_constness_hover(
    offset: u32,
    source: &str,
    analysis: &FileAnalysisSnapshot,
) -> Option<Hover> {
    let template = analysis.template.as_deref()?;

    // Find component usage at cursor position — match only the tag name.
    // c.name is PascalCase-normalized but the source tag may be kebab-case,
    // so scan the source to find the actual tag name end.
    let comp = find_component_usage_at_tag_offset(offset, source, template)?;

    let mut lines = Vec::new();
    let source_info = comp
        .import_source
        .as_deref()
        .map(|s| format!(" (from `{s}`)"))
        .unwrap_or_default();
    lines.push(format!("**`<{}>`**{source_info}\n", comp.name));

    if comp.props.is_empty() {
        // Return component info even without props — TSGO may also return None,
        // so this ensures the user always sees at least the component name.
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: lines.join("\n"),
            }),
            range: None,
        });
    }

    lines.push("**Props:**".to_string());

    for prop in &comp.props {
        let constness_label = match prop.constness {
            verter_semantic::analysis::template::PropValueConstness::Const => "const",
            verter_semantic::analysis::template::PropValueConstness::Dynamic => "dynamic",
            verter_semantic::analysis::template::PropValueConstness::Unknown => "unknown",
        };
        let icon = match prop.constness {
            verter_semantic::analysis::template::PropValueConstness::Const => "\u{2713}", // ✓
            verter_semantic::analysis::template::PropValueConstness::Dynamic => "\u{2197}", // ↗
            verter_semantic::analysis::template::PropValueConstness::Unknown => "?",
        };
        let bound = if prop.is_bound { ":" } else { "" };
        lines.push(format!(
            "- {icon} `{bound}{}` — *{constness_label}*",
            prop.name
        ));
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: lines.join("\n"),
        }),
        range: None,
    })
}

fn component_tag_hover_target(
    offset: u32,
    source: &str,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<ComponentTagHoverTarget> {
    let comp = find_component_usage_at_tag_offset(offset, source, template)?;
    let import_source = component_import_source(comp, analysis)?;
    let usage_props = comp
        .props
        .iter()
        .map(|prop| ComponentUsagePropInfo {
            name: prop.name.clone(),
            is_bound: prop.is_bound,
            constness: prop.constness,
        })
        .collect();

    Some(ComponentTagHoverTarget {
        component_name: comp.name.clone(),
        import_source,
        usage_props,
    })
}

fn component_event_hover_target(
    offset: u32,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<ComponentEventHoverTarget> {
    for el in &template.elements {
        if !el.is_component {
            continue;
        }
        for dir in &el.directives {
            if dir.name != "on" {
                continue;
            }
            let Some(arg_span) = dir.arg_span.as_ref() else {
                continue;
            };
            let hover_start = dir.span.start.saturating_sub(1);
            let hover_end = arg_span.end;
            if offset < hover_start || offset >= hover_end {
                continue;
            }
            let component = template.components.iter().find(|component| {
                component.span.start == el.span.start && component.span.end == el.span.end
            })?;
            let import_source = component_import_source(component, analysis)?;
            let event_name = dir.argument.clone()?;
            return Some(ComponentEventHoverTarget {
                component_name: component.name.clone(),
                import_source,
                vue_attr: vue_event_attr_label(&event_name),
                event_name,
            });
        }
    }
    None
}

/// The slot-NAME region of a `v-slot` / `#name` directive: the directive name
/// plus its argument, EXCLUDING the destructure-pattern expression. Pattern
/// positions map into the generated TSX (the pattern is emitted as verbatim
/// mapped bytes inside the slot IIFE) and are answered by the provider's typed
/// binding quickinfo (D4); only the name/arg region is Verter-owned (D3).
fn slot_directive_name_arg_region(
    dir: &verter_semantic::analysis::template::TemplateDirective,
) -> (u32, u32) {
    let end = dir
        .arg_span
        .as_ref()
        .map(|span| span.end)
        .unwrap_or(dir.name_end);
    (dir.span.start, end)
}

/// Identify a slot-name token (`#header`, `v-slot:header`, `#default`, kebab
/// `#my-slot`, arg-less `v-slot`) on a component slot usage, resolving the
/// owning child component — the element itself for `<MyComp #header>`, or the
/// parent component element for `<template #header>`.
fn slot_attribute_hover_target(
    offset: u32,
    template: &verter_semantic::analysis::template::TemplateAnalysisSnapshot,
    analysis: &FileAnalysisSnapshot,
) -> Option<SlotAttributeHoverTarget> {
    for el in &template.elements {
        for dir in &el.directives {
            if dir.name != "slot" {
                continue;
            }
            let (region_start, region_end) = slot_directive_name_arg_region(dir);
            if offset < region_start || offset >= region_end {
                continue;
            }
            let component = if el.is_component {
                template.components.iter().find(|component| {
                    component.span.start == el.span.start && component.span.end == el.span.end
                })?
            } else if el.tag == "template" {
                let parent = el
                    .parent_index
                    .and_then(|idx| template.elements.get(idx as usize))?;
                if !parent.is_component {
                    continue;
                }
                template.components.iter().find(|component| {
                    component.name == parent.tag
                        || component.name == crate::server::to_pascal_case(&parent.tag)
                })?
            } else {
                continue;
            };
            let import_source = component_import_source(component, analysis)?;
            let slot_name = dir
                .argument
                .clone()
                .unwrap_or_else(|| "default".to_string());
            let vue_attr = if dir.raw_name.starts_with('#') {
                format!("#{slot_name}")
            } else if dir.argument.is_some() {
                format!("v-slot:{slot_name}")
            } else {
                "v-slot".to_string()
            };
            return Some(SlotAttributeHoverTarget {
                component_name: component.name.clone(),
                import_source,
                slot_name,
                vue_attr,
            });
        }
    }
    None
}

/// Build the typed slot-name hover from the CHILD's declared slots surface
/// (D3): the slot's defineSlots signature — name, slot-props payload, return
/// type — resolved with Vue's kebab↔camel equivalence (`#my-slot` → `mySlot`).
/// The authored spelling is presentation context only. A slot absent from the
/// contract yields `None`; no child-analysis fallback fabricates a signature.
///
/// `deepened` carries the rank-matched slot's demand-resolved carrier views
/// (derived from the SAME contract rows through the one sanctioned deepen
/// route); a binding without a deepened view renders its published form —
/// the typed refusal stays the fail-closed branch for a carrier that could
/// not deepen.
pub fn build_child_slot_hover(
    vue_attr: &str,
    slot_name: &str,
    availability: &ComponentContractAvailability,
    deepened: &crate::features::hover_slot_deepen::SlotBindingDeepenView,
) -> Option<Hover> {
    let ComponentContractAvailability::Supported(contract) = availability else {
        let ComponentContractAvailability::Unsupported(unsupported) = availability else {
            unreachable!("component contract availability is closed")
        };
        return Some(make_hover(render_contract_unsupported(unsupported)));
    };
    let slot =
        crate::features::hover_slot_deepen::select_contract_slot(&contract.slots, slot_name)?;
    Some(make_hover(format!(
        "```typescript\n(slot) {}{}\n```\n\n**Authored as:** `{vue_attr}`",
        slot.name,
        render_slot_signature(slot, deepened)
    )))
}

/// Resolve the authored module source for a template component.
///
/// The template analyzer normally stamps `import_source` directly. During an
/// editor/provider resync, however, a newly rebuilt template snapshot can
/// temporarily retain the component usage while that optional convenience
/// field is absent. The script import inventory remains authoritative and is
/// enough to recover the same source by the component's local binding. Global
/// components and unresolved tags still fail closed because they have no
/// matching value import.
fn component_import_source(
    component: &verter_semantic::analysis::template::TemplateComponentUsage,
    analysis: &FileAnalysisSnapshot,
) -> Option<String> {
    component.import_source.clone().or_else(|| {
        analysis
            .imports
            .iter()
            .filter(|import| !import.is_type_only)
            .find(|import| {
                import
                    .bindings
                    .iter()
                    .any(|binding| !binding.is_type_only && binding.name == component.name)
            })
            .map(|import| import.source.clone())
    })
}

fn find_component_usage_at_tag_offset<'a>(
    offset: u32,
    source: &str,
    template: &'a verter_semantic::analysis::template::TemplateAnalysisSnapshot,
) -> Option<&'a verter_semantic::analysis::template::TemplateComponentUsage> {
    template.components.iter().find(|component| {
        let tag_start = component.span.start + 1;
        let tag_end = source
            .get(tag_start as usize..)
            .and_then(|slice| {
                slice
                    .find(|ch: char| !ch.is_alphanumeric() && ch != '-' && ch != '_')
                    .map(|idx| tag_start + idx as u32)
            })
            .unwrap_or(component.span.end);
        offset >= tag_start && offset < tag_end
    })
}

/// Check if the offset is on a Vue API call site name, and if so return a hover
/// with Vue API context (category, sync requirement, description).
/// Client-only lifecycle hooks that never fire during SSR.
const CLIENT_ONLY_HOOKS: &[verter_semantic::analysis::VueApiClassification] = &[
    verter_semantic::analysis::VueApiClassification::OnMounted,
    verter_semantic::analysis::VueApiClassification::OnUpdated,
    verter_semantic::analysis::VueApiClassification::OnActivated,
    verter_semantic::analysis::VueApiClassification::OnDeactivated,
    verter_semantic::analysis::VueApiClassification::OnBeforeUpdate,
    verter_semantic::analysis::VueApiClassification::OnBeforeMount,
];

mod vue_api;
pub(super) use vue_api::hover_for_word;
use vue_api::vue_api_hover_at_offset;

use crate::utils::word_at_offset;

#[cfg(test)]
#[path = "hover_tests.rs"]
mod hover_tests;

#[cfg(test)]
#[path = "hover/public_contract_guard_tests.rs"]
mod public_contract_guard_tests;
