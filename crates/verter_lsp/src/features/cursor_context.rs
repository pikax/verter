use oxc_span::GetSpan;
use verter_session::FileAnalysisSnapshot;

use crate::documents::sfc_scanner::{classify_cursor, SfcBlock, SfcCursorContext};

// =============================================================================
// Types
// =============================================================================

/// Top-level cursor context within an SFC file.
#[derive(Debug)]
pub enum CursorContext {
    /// Inside a <script> block — delegate to TypeProvider
    Script,
    /// Inside a <template> block — granular sub-context
    Template(TemplateCursorContext),
    /// Inside a <style> block
    Style(StyleCursorContext),
    /// Inside a custom block (<i18n>, etc.)
    CustomBlock { tag_name: String },
    /// On an SFC block's opening tag (<template lang="pug">)
    BlockOpeningTag { tag_name: String },
    /// On an SFC block's closing tag
    BlockClosingTag,
    /// Outside all blocks (root level of .vue file)
    RootLevel,
}

/// Granular template cursor context — determined from TemplateAnalysisSnapshot AST.
#[derive(Debug)]
pub enum TemplateCursorContext {
    /// After `<` — offer component/element names
    TagName {
        /// Partial tag name typed so far (for filtering)
        partial: String,
    },
    /// Inside `</...>` — offer matching closing tag
    ClosingTagName { partial: String },
    /// Attribute name position: `<div |` or `<div cla|`
    /// Offer: HTML attrs, Vue directives, component props/events
    AttributeName {
        tag_name: String,
        is_component: bool,
        /// Attribute names already present (for dedup)
        existing_attrs: Vec<String>,
    },
    /// Event modifier: `@click.prev|` or `@click.prevent.|`
    EventModifier {
        event_name: String,
        existing_modifiers: Vec<String>,
    },
    /// v-model modifier: `v-model.laz|`
    VModelModifier { existing_modifiers: Vec<String> },
    /// Directive argument: `v-slot:|name` or `v-bind:|prop`
    DirectiveArgument { directive: String, tag_name: String },
    /// Expression inside a dynamic prop: `:prop="expr|"`
    /// Or v-if/v-show/v-for/v-slot expression
    Expression {
        /// Which directive/prop this expression belongs to
        kind: ExpressionKind,
    },
    /// Interpolation expression: `{{ expr| }}`
    Interpolation,
    /// Static attribute value (non-dynamic): `title="hel|lo"`
    StaticValue { attr_name: String },
    /// Plain text between elements
    TextContent,
}

/// What kind of expression the cursor is inside.
#[derive(Debug)]
pub enum ExpressionKind {
    /// :prop="expr" or v-bind:prop="expr"
    Prop { prop_name: String },
    /// v-if or v-else-if
    VIf,
    /// v-for
    VFor,
    /// v-show
    VShow,
    /// v-slot="pattern"
    VSlot,
    /// v-on:event="expr" or @event="expr"
    EventHandler { event_name: String },
    /// v-html, v-text
    ContentDirective { name: String },
    /// v-model expression
    VModel,
    /// v-memo="[deps]"
    VMemo,
    /// Any other directive expression
    Other { directive: String },
}

/// Expression sub-context from OXC AST analysis.
/// Only computed when Layer 1 returns an expression/interpolation context.
#[derive(Debug)]
pub enum ExpressionContext {
    /// Cursor is where a variable reference is valid — show verter bindings
    IdentifierExpected,
    /// After `.` or `?.` — suppress verter, show TypeProvider members
    MemberAccess,
    /// Inside a literal (string, number, regex, template text) — suppress verter
    Literal,
    /// In a TypeScript type position (as T, satisfies T) — suppress verter
    TypePosition,
    /// On a non-computed object property key — suppress verter
    PropertyKey,
    /// Parse error or unmapped region — conservative: show verter
    Unknown,
}

/// Style block cursor sub-context.
#[derive(Debug)]
pub enum StyleCursorContext {
    /// Inside v-bind() expression — offer reactive bindings
    VBind,
    /// General CSS position — delegate to VS Code CSS service
    General,
}

// =============================================================================
// Layer 1: AST-Based Structural Detection
// =============================================================================

/// Classify the cursor position within an SFC file using AST data from analysis.
///
/// **Layer 1**: Determines WHERE in the SFC the cursor is — script, template sub-context
/// (tag name, attribute, directive expression, interpolation, etc.), style, or root level.
pub fn classify_cursor_context(
    offset: u32,
    source: &str,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
) -> CursorContext {
    // Step 1: SFC block detection using existing scanner
    match classify_cursor(offset, blocks) {
        SfcCursorContext::RootLevel => return CursorContext::RootLevel,
        SfcCursorContext::OpeningTag { block_index } => {
            return CursorContext::BlockOpeningTag {
                tag_name: blocks[block_index].tag_name.clone(),
            };
        }
        SfcCursorContext::ClosingTag { .. } => return CursorContext::BlockClosingTag,
        SfcCursorContext::BlockContent { .. } => {} // fall through to block-specific
    }

    // Find which block the cursor is in
    let block = match blocks.iter().find(|b| {
        let (cs, ce) = b.content_range();
        offset >= cs && offset <= ce
    }) {
        Some(b) => b,
        None => return CursorContext::RootLevel,
    };

    match block.tag_name.as_str() {
        "script" => CursorContext::Script,
        "template" => classify_template_context(offset, source, analysis),
        "style" => classify_style_context(offset, blocks, analysis),
        tag_name => CursorContext::CustomBlock {
            tag_name: tag_name.to_string(),
        },
    }
}

/// Classify cursor position within a template block using AST data.
fn classify_template_context(
    offset: u32,
    source: &str,
    analysis: Option<&FileAnalysisSnapshot>,
) -> CursorContext {
    let template = match analysis.and_then(|a| a.template.as_ref()) {
        Some(t) => t,
        None => {
            // No analysis available — fall back to text scanning
            return CursorContext::Template(classify_template_text_fallback(offset, source));
        }
    };

    // Find the deepest element containing the cursor
    let deepest = find_deepest_element(offset, &template.elements);

    match deepest {
        Some(el) => classify_within_element(offset, source, el, &template.elements),
        None => {
            // Cursor is in template content but not inside any element's span.
            // This can happen between top-level elements or when analysis is incomplete.
            CursorContext::Template(classify_template_text_fallback(offset, source))
        }
    }
}

/// Find the deepest (most nested) element whose span contains the offset.
fn find_deepest_element(
    offset: u32,
    elements: &[verter_semantic::analysis::template::TemplateElement],
) -> Option<&verter_semantic::analysis::template::TemplateElement> {
    let mut best: Option<&verter_semantic::analysis::template::TemplateElement> = None;
    let mut best_size = u32::MAX;

    for el in elements {
        if offset >= el.span.start && offset < el.span.end {
            let size = el.span.end - el.span.start;
            if size < best_size {
                best = Some(el);
                best_size = size;
            }
        }
    }
    best
}

/// Classify cursor position within a specific element.
fn classify_within_element(
    offset: u32,
    source: &str,
    el: &verter_semantic::analysis::template::TemplateElement,
    all_elements: &[verter_semantic::analysis::template::TemplateElement],
) -> CursorContext {
    // Case A: cursor is in the opening tag (before tag_span_end)
    if offset < el.tag_span_end {
        return classify_in_opening_tag(offset, source, el);
    }

    // Case B: cursor is in the closing tag (after content_end)
    if offset >= el.content_end {
        let partial = extract_partial_after(offset, source, b'/');
        return CursorContext::Template(TemplateCursorContext::ClosingTagName { partial });
    }

    // Case C: cursor is in element content (between opening and closing tag)
    classify_in_content(offset, el, all_elements)
}

/// Classify cursor within an element's opening tag.
fn classify_in_opening_tag(
    offset: u32,
    source: &str,
    el: &verter_semantic::analysis::template::TemplateElement,
) -> CursorContext {
    // Check if cursor is on the tag name itself
    // Tag name starts right after '<' (el.span.start + 1)
    let tag_name_start = el.span.start + 1;
    let tag_name_end = tag_name_start + el.tag.len() as u32;
    if offset >= tag_name_start && offset < tag_name_end {
        let partial = if offset > tag_name_start {
            source
                .get(tag_name_start as usize..offset as usize)
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };
        return CursorContext::Template(TemplateCursorContext::TagName { partial });
    }

    // Check directives first (they have richer span info)
    for dir in &el.directives {
        // Check if cursor is within or right after the directive span.
        // Cursor can be past span.end when typing a modifier dot (e.g., @click.prevent.|)
        let in_directive = (offset >= dir.span.start && offset < dir.span.end)
            || (offset >= dir.span.end && offset <= el.tag_span_end && {
                let at_cursor = source.as_bytes().get(offset as usize).copied();
                let before_cursor = if offset > 0 {
                    source.as_bytes().get(offset as usize - 1).copied()
                } else {
                    None
                };
                let between = source
                    .get(dir.span.end as usize..offset as usize)
                    .unwrap_or("");
                at_cursor == Some(b'.')
                    || before_cursor == Some(b'.')
                    || between.contains('.')
                    || (!between.is_empty()
                        && between.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'
                        }))
            });
        if !in_directive {
            continue;
        }

        // Unified modifier detection: check if cursor is in a modifier position.
        // This handles all cases:
        // 1. @click.prev| — cursor within a modifier span
        // 2. @click.prevent.| — cursor after last modifier, at a new dot
        // 3. @click.| — cursor after first dot, no existing modifiers
        if dir.name == "on" || dir.name == "model" {
            // Check the source text from directive start to cursor for dots after the argument/name
            let dir_text = source
                .get(dir.span.start as usize..offset as usize)
                .unwrap_or("");

            // Find where modifiers could start (after arg or after directive name)
            let modifier_region_start = if let Some(ref arg_span) = dir.arg_span {
                (arg_span.end - dir.span.start) as usize
            } else {
                (dir.name_end - dir.span.start) as usize
            };

            let after_name = dir_text.get(modifier_region_start..).unwrap_or("");
            if after_name.contains('.') {
                let existing: Vec<String> = dir.modifiers.clone();
                if dir.name == "model" {
                    return CursorContext::Template(TemplateCursorContext::VModelModifier {
                        existing_modifiers: existing,
                    });
                } else {
                    return CursorContext::Template(TemplateCursorContext::EventModifier {
                        event_name: dir.argument.clone().unwrap_or_default(),
                        existing_modifiers: existing,
                    });
                }
            }
            // Also check if the byte right at cursor is a dot (just typed)
            if source.as_bytes().get(offset as usize) == Some(&b'.') {
                let existing: Vec<String> = dir.modifiers.clone();
                if dir.name == "model" {
                    return CursorContext::Template(TemplateCursorContext::VModelModifier {
                        existing_modifiers: existing,
                    });
                } else {
                    return CursorContext::Template(TemplateCursorContext::EventModifier {
                        event_name: dir.argument.clone().unwrap_or_default(),
                        existing_modifiers: existing,
                    });
                }
            }
        }

        // Check expression span
        if let Some(ref expr_span) = dir.expression_span {
            // Normal case: cursor within expression span
            if (offset >= expr_span.start && offset < expr_span.end)
                || (expr_span.start == expr_span.end && offset == expr_span.start)
            {
                let kind = directive_to_expression_kind(dir);
                return CursorContext::Template(TemplateCursorContext::Expression { kind });
            }
            // Stale analysis / boundary fallback: cursor is at or past expression_span.end
            // but still within the directive span. The user likely typed more into the
            // expression since analysis was last computed (e.g., "action.icon" → "action.icon || x").
            if offset >= expr_span.end {
                let kind = directive_to_expression_kind(dir);
                return CursorContext::Template(TemplateCursorContext::Expression { kind });
            }
        }

        // Check argument span
        if let Some(ref arg_span) = dir.arg_span {
            if offset >= arg_span.start && offset < arg_span.end {
                return CursorContext::Template(TemplateCursorContext::DirectiveArgument {
                    directive: dir.name.clone(),
                    tag_name: el.tag.clone(),
                });
            }
        }

        // Cursor is on the directive name itself — treat as attribute name
        if offset < dir.name_end {
            return CursorContext::Template(TemplateCursorContext::AttributeName {
                tag_name: el.tag.clone(),
                is_component: el.is_component,
                existing_attrs: collect_existing_attrs(el),
            });
        }
    }

    // Check attributes
    for attr in &el.attributes {
        if offset < attr.span.start || offset >= attr.span.end {
            continue;
        }

        // Check if cursor is in value span
        if let Some(ref val_span) = attr.value_span {
            if (offset >= val_span.start && offset <= val_span.end)
                || (val_span.start == val_span.end && offset == val_span.start)
            {
                if attr.is_dynamic {
                    // Dynamic attribute value — this is really a directive expression
                    // but tracked as an attribute in the analysis
                    return CursorContext::Template(TemplateCursorContext::Expression {
                        kind: ExpressionKind::Prop {
                            prop_name: attr.name.clone(),
                        },
                    });
                } else {
                    return CursorContext::Template(TemplateCursorContext::StaticValue {
                        attr_name: attr.name.clone(),
                    });
                }
            }
            // Stale analysis / boundary fallback: cursor past value_span.end but
            // within the attribute span. User typed more since last analysis.
            if offset >= val_span.end && attr.is_dynamic {
                return CursorContext::Template(TemplateCursorContext::Expression {
                    kind: ExpressionKind::Prop {
                        prop_name: attr.name.clone(),
                    },
                });
            }
        }

        // Cursor is on the attribute name
        if offset < attr.name_end {
            return CursorContext::Template(TemplateCursorContext::AttributeName {
                tag_name: el.tag.clone(),
                is_component: el.is_component,
                existing_attrs: collect_existing_attrs(el),
            });
        }
    }

    // Cursor is in the opening tag but not on any attribute/directive — attribute name position
    CursorContext::Template(TemplateCursorContext::AttributeName {
        tag_name: el.tag.clone(),
        is_component: el.is_component,
        existing_attrs: collect_existing_attrs(el),
    })
}

/// Classify cursor within element content (between opening and closing tags).
fn classify_in_content(
    offset: u32,
    el: &verter_semantic::analysis::template::TemplateElement,
    all_elements: &[verter_semantic::analysis::template::TemplateElement],
) -> CursorContext {
    // Check text children for interpolations and text
    for segment in &el.text_children {
        match segment {
            verter_semantic::analysis::template::TemplateTextSegment::Interpolation {
                span,
                expression_span,
            } => {
                if offset >= expression_span.start && offset < expression_span.end {
                    return CursorContext::Template(TemplateCursorContext::Interpolation);
                }
                // Inside {{ }} but outside expression (e.g., on the braces themselves)
                if offset >= span.start && offset < span.end {
                    return CursorContext::Template(TemplateCursorContext::Interpolation);
                }
            }
            verter_semantic::analysis::template::TemplateTextSegment::Text { span, .. } => {
                if offset >= span.start && offset < span.end {
                    return CursorContext::Template(TemplateCursorContext::TextContent);
                }
            }
        }
    }

    // Check if cursor might be inside a child element (not directly tracked in text_children)
    // This shouldn't normally happen since find_deepest_element picks the innermost,
    // but handle it gracefully.
    for child in all_elements {
        if let Some(_pi) = child.parent_index {
            // Not a direct comparison — we'd need the element index, but we can check spans
            if offset >= child.span.start && offset < child.span.end {
                // Cursor is inside a child element — shouldn't reach here if find_deepest_element works
                return classify_within_element(offset, "", child, all_elements);
            }
        }
    }

    // Default: text content between children
    CursorContext::Template(TemplateCursorContext::TextContent)
}

/// Convert a directive to an ExpressionKind.
fn directive_to_expression_kind(
    dir: &verter_semantic::analysis::template::TemplateDirective,
) -> ExpressionKind {
    match dir.name.as_str() {
        "if" | "else-if" => ExpressionKind::VIf,
        "for" => ExpressionKind::VFor,
        "show" => ExpressionKind::VShow,
        "slot" => ExpressionKind::VSlot,
        "on" => ExpressionKind::EventHandler {
            event_name: dir.argument.clone().unwrap_or_default(),
        },
        "bind" => ExpressionKind::Prop {
            prop_name: dir.argument.clone().unwrap_or_default(),
        },
        "html" => ExpressionKind::ContentDirective {
            name: "html".to_string(),
        },
        "text" => ExpressionKind::ContentDirective {
            name: "text".to_string(),
        },
        "model" => ExpressionKind::VModel,
        "memo" => ExpressionKind::VMemo,
        other => ExpressionKind::Other {
            directive: other.to_string(),
        },
    }
}

/// Collect existing attribute/directive names on an element for dedup.
fn collect_existing_attrs(
    el: &verter_semantic::analysis::template::TemplateElement,
) -> Vec<String> {
    let mut names = Vec::new();
    for attr in &el.attributes {
        names.push(attr.name.clone());
    }
    for dir in &el.directives {
        names.push(dir.raw_name.clone());
    }
    names
}

/// Extract a partial identifier typed after a specific byte marker.
fn extract_partial_after(offset: u32, source: &str, _marker: u8) -> String {
    // Scan backward from cursor to find start of identifier
    let bytes = source.as_bytes();
    let mut start = offset as usize;
    while start > 0
        && (bytes[start - 1].is_ascii_alphanumeric()
            || bytes[start - 1] == b'-'
            || bytes[start - 1] == b'_')
    {
        start -= 1;
    }
    source.get(start..offset as usize).unwrap_or("").to_string()
}

/// Classify style cursor context.
fn classify_style_context(
    offset: u32,
    blocks: &[SfcBlock],
    analysis: Option<&FileAnalysisSnapshot>,
) -> CursorContext {
    if let Some(analysis) = analysis {
        // Check all style blocks for v-bind() expressions
        for (i, style_block) in blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.tag_name == "style")
        {
            let (cs, ce) = style_block.content_range();
            if offset < cs || offset > ce {
                continue;
            }
            // Find the corresponding style analysis
            // Style blocks are indexed in order they appear
            let style_idx = blocks
                .iter()
                .take(i + 1)
                .filter(|b| b.tag_name == "style")
                .count()
                - 1;
            if let Some(style_analysis) = analysis.styles.get(style_idx) {
                for vb in &style_analysis.v_binds {
                    if offset >= vb.start && offset < vb.end {
                        return CursorContext::Style(StyleCursorContext::VBind);
                    }
                }
            }
        }
    }
    CursorContext::Style(StyleCursorContext::General)
}

/// Text-based fallback for template classification when no analysis is available.
fn classify_template_text_fallback(offset: u32, source: &str) -> TemplateCursorContext {
    let offset = offset as usize;
    let bytes = source.as_bytes();
    if offset > bytes.len() {
        return TemplateCursorContext::TextContent;
    }

    // Check for mustache context: find `{{` before `}}` scanning backward
    {
        let before = &source[..offset];
        let last_open = before.rfind("{{");
        let last_close = before.rfind("}}");
        if let Some(open_pos) = last_open {
            if last_close.is_none_or(|close_pos| open_pos > close_pos) {
                return TemplateCursorContext::Interpolation;
            }
        }
    }

    // Scan backward to determine if inside a tag or text content
    let mut i = offset;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'>' => return TemplateCursorContext::TextContent,
            b'<' => {
                let tag_start = i + 1;
                if tag_start < bytes.len() && bytes[tag_start] == b'/' {
                    return TemplateCursorContext::TextContent;
                }
                // Skip past tag name
                let mut name_end = tag_start;
                while name_end < bytes.len()
                    && (bytes[name_end].is_ascii_alphanumeric()
                        || bytes[name_end] == b'-'
                        || bytes[name_end] == b'_')
                {
                    name_end += 1;
                }
                if offset <= name_end {
                    let partial = source.get(tag_start..offset).unwrap_or("").to_string();
                    return TemplateCursorContext::TagName { partial };
                }
                return TemplateCursorContext::AttributeName {
                    tag_name: source.get(tag_start..name_end).unwrap_or("").to_string(),
                    is_component: source
                        .as_bytes()
                        .get(tag_start)
                        .is_some_and(|b| b.is_ascii_uppercase()),
                    existing_attrs: vec![],
                };
            }
            _ => {}
        }
    }
    TemplateCursorContext::TextContent
}

// =============================================================================
// Layer 2: OXC Expression Sub-Context
// =============================================================================

/// Classify expression sub-context with an optional trigger character shortcut.
pub fn classify_expression_context_with_trigger(
    expr_content: &str,
    cursor_offset: usize,
    trigger_character: Option<&str>,
) -> ExpressionContext {
    // Shortcut: trigger character `.` means member access without needing to parse
    if trigger_character == Some(".") {
        return ExpressionContext::MemberAccess;
    }

    classify_expression_context(expr_content, cursor_offset)
}

/// Classify expression sub-context using OXC AST analysis.
///
/// Parses the expression as TSX and walks the AST to determine what kind of
/// expression position the cursor is in.
pub fn classify_expression_context(tsx_content: &str, tsx_offset: usize) -> ExpressionContext {
    if tsx_content.is_empty() || tsx_offset == 0 {
        return ExpressionContext::IdentifierExpected;
    }

    // Quick heuristic: check for member access pattern (`.` before cursor with identifier chars)
    // This handles incomplete expressions like `foo.` that OXC may not parse correctly.
    if is_member_access_position(tsx_content, tsx_offset) {
        return ExpressionContext::MemberAccess;
    }

    // Try OXC parse for more precise classification
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::tsx();
    // Wrap in an expression statement for parsing
    let wrapped = format!("({})", tsx_content);
    let wrapped_offset = tsx_offset + 1; // account for the wrapping `(`
    let parse_result = oxc_parser::Parser::new(&allocator, &wrapped, source_type).parse();

    if parse_result.panicked {
        return ExpressionContext::Unknown;
    }

    // Walk the AST to find the deepest node containing the offset
    classify_from_ast(&parse_result.program, wrapped_offset)
}

/// Check if cursor is in a member access position using byte scanning.
fn is_member_access_position(content: &str, offset: usize) -> bool {
    if offset == 0 {
        return false;
    }
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut i = offset;
    if i > len {
        return false;
    }
    // Skip backward past partial identifier
    while i > 0
        && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_' || bytes[i - 1] == b'$')
    {
        i -= 1;
    }
    // Skip whitespace
    while i > 0 && bytes[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return false;
    }
    // Check for spread `..`
    if i >= 2 && bytes[i - 2] == b'.' {
        return false;
    }
    i -= 1;
    // Check for optional chaining `?.`
    if i > 0 && bytes[i - 1] == b'?' {
        return true;
    }
    // The char before `.` must be an identifier char, `)`, or `]`
    // BUT NOT a standalone digit — `1.5` is a number literal, not member access
    if i > 0 {
        let c = bytes[i - 1];
        if c.is_ascii_digit() {
            // Could be a number literal (1.5) — check if there are only digits before
            let mut j = i - 1;
            while j > 0 && bytes[j - 1].is_ascii_digit() {
                j -= 1;
            }
            // If we reached start or a non-identifier char, it's a number literal
            if j == 0
                || !(bytes[j - 1].is_ascii_alphanumeric()
                    || bytes[j - 1] == b'_'
                    || bytes[j - 1] == b'$')
            {
                return false;
            }
        }
        c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c == b')' || c == b']'
    } else {
        false
    }
}

/// Walk the OXC AST to classify the expression context at a given offset.
fn classify_from_ast(program: &oxc_ast::ast::Program<'_>, offset: usize) -> ExpressionContext {
    use oxc_ast::ast::*;

    // Walk statements to find the expression
    for stmt in &program.body {
        let span = get_stmt_span(stmt);
        if offset < span.start as usize || offset > span.end as usize {
            continue;
        }

        if let Statement::ExpressionStatement(expr_stmt) = stmt {
            return classify_expression(&expr_stmt.expression, offset);
        }
    }

    ExpressionContext::IdentifierExpected
}

fn classify_expression(expr: &oxc_ast::ast::Expression<'_>, offset: usize) -> ExpressionContext {
    use oxc_ast::ast::Expression;

    match expr {
        // Parenthesized expression — unwrap
        Expression::ParenthesizedExpression(paren) => {
            return classify_expression(&paren.expression, offset);
        }

        // Member access: foo.bar or foo?.bar
        Expression::StaticMemberExpression(member) => {
            // If cursor is at or after the dot (object.span.end), it's member access
            if offset >= member.object.span().end as usize {
                return ExpressionContext::MemberAccess;
            }
            // Cursor is on the object — recurse
            return classify_expression(&member.object, offset);
        }

        Expression::ComputedMemberExpression(member) => {
            if offset >= member.object.span().end as usize {
                // Could be inside [expr] — check if in the expression part
                return classify_expression(&member.expression, offset);
            }
            return classify_expression(&member.object, offset);
        }

        // Literals
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_) => {
            return ExpressionContext::Literal;
        }

        Expression::TemplateLiteral(tl) => {
            // Check if cursor is in a quasi (text) or expression part
            for expr in &tl.expressions {
                let span = expr.span();
                if offset >= span.start as usize && offset <= span.end as usize {
                    return classify_expression(expr, offset);
                }
            }
            return ExpressionContext::Literal;
        }

        // Type assertions
        Expression::TSAsExpression(ts) => {
            if offset >= ts.type_annotation.span().start as usize {
                return ExpressionContext::TypePosition;
            }
            return classify_expression(&ts.expression, offset);
        }
        Expression::TSSatisfiesExpression(ts) => {
            if offset >= ts.type_annotation.span().start as usize {
                return ExpressionContext::TypePosition;
            }
            return classify_expression(&ts.expression, offset);
        }
        Expression::TSNonNullExpression(ts) => {
            return classify_expression(&ts.expression, offset);
        }
        Expression::TSTypeAssertion(ts) => {
            if offset < ts.type_annotation.span().end as usize {
                return ExpressionContext::TypePosition;
            }
            return classify_expression(&ts.expression, offset);
        }

        // Object expression — check if cursor is on a property key
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                match prop {
                    oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) => {
                        let key_span = p.key.span();
                        if offset >= key_span.start as usize
                            && offset <= key_span.end as usize
                            && !p.computed
                        {
                            return ExpressionContext::PropertyKey;
                        }
                        // Check value
                        let val_span = p.value.span();
                        if offset >= val_span.start as usize && offset <= val_span.end as usize {
                            return classify_expression(&p.value, offset);
                        }
                    }
                    oxc_ast::ast::ObjectPropertyKind::SpreadProperty(s) => {
                        let arg_span = s.argument.span();
                        if offset >= arg_span.start as usize && offset <= arg_span.end as usize {
                            return classify_expression(&s.argument, offset);
                        }
                    }
                }
            }
            return ExpressionContext::PropertyKey; // In object but not on specific prop
        }

        // Call expression — recurse into callee or arguments
        Expression::CallExpression(call) => {
            let callee_span = call.callee.span();
            if offset >= callee_span.start as usize && offset <= callee_span.end as usize {
                return classify_expression(&call.callee, offset);
            }
            for arg in &call.arguments {
                let arg_span = arg.span();
                if offset >= arg_span.start as usize && offset <= arg_span.end as usize {
                    if let oxc_ast::ast::Argument::SpreadElement(s) = arg {
                        return classify_expression(&s.argument, offset);
                    }
                    return classify_expression(arg.to_expression(), offset);
                }
            }
        }

        // Conditional (ternary)
        Expression::ConditionalExpression(cond) => {
            for sub in [&cond.test, &cond.consequent, &cond.alternate] {
                let span = sub.span();
                if offset >= span.start as usize && offset <= span.end as usize {
                    return classify_expression(sub, offset);
                }
            }
        }

        // Binary/Logical — recurse into left/right
        Expression::BinaryExpression(bin) => {
            if offset <= bin.left.span().end as usize {
                return classify_expression(&bin.left, offset);
            }
            return classify_expression(&bin.right, offset);
        }
        Expression::LogicalExpression(log) => {
            if offset <= log.left.span().end as usize {
                return classify_expression(&log.left, offset);
            }
            return classify_expression(&log.right, offset);
        }

        // Unary
        Expression::UnaryExpression(u) => {
            return classify_expression(&u.argument, offset);
        }

        // Array
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                let span = elem.span();
                if offset >= span.start as usize && offset <= span.end as usize {
                    match elem {
                        oxc_ast::ast::ArrayExpressionElement::SpreadElement(s) => {
                            return classify_expression(&s.argument, offset);
                        }
                        oxc_ast::ast::ArrayExpressionElement::Elision(_) => {}
                        _ => {
                            return classify_expression(elem.to_expression(), offset);
                        }
                    }
                }
            }
        }

        // Arrow function body
        Expression::ArrowFunctionExpression(arrow) => {
            // Check if it has an expression body (single expression, no braces)
            if !arrow.expression {
                return ExpressionContext::IdentifierExpected;
            }
            if let Some(oxc_ast::ast::Statement::ExpressionStatement(expr_stmt)) =
                arrow.body.statements.first()
            {
                let span = expr_stmt.expression.span();
                if offset >= span.start as usize && offset <= span.end as usize {
                    return classify_expression(&expr_stmt.expression, offset);
                }
            }
        }

        // Identifier
        Expression::Identifier(_) => {
            return ExpressionContext::IdentifierExpected;
        }

        // Assignment
        Expression::AssignmentExpression(assign) => {
            let right_span = assign.right.span();
            if offset >= right_span.start as usize {
                return classify_expression(&assign.right, offset);
            }
        }

        // Sequence (comma-separated)
        Expression::SequenceExpression(seq) => {
            for expr in &seq.expressions {
                let span = expr.span();
                if offset >= span.start as usize && offset <= span.end as usize {
                    return classify_expression(expr, offset);
                }
            }
        }

        _ => {}
    }

    ExpressionContext::IdentifierExpected
}

fn get_stmt_span(stmt: &oxc_ast::ast::Statement<'_>) -> oxc_span::Span {
    use oxc_ast::ast::Statement;
    match stmt {
        Statement::ExpressionStatement(s) => s.span,
        Statement::BlockStatement(s) => s.span,
        Statement::VariableDeclaration(s) => s.span,
        Statement::ReturnStatement(s) => s.span,
        Statement::IfStatement(s) => s.span,
        _ => oxc_span::Span::new(0, 0),
    }
}

#[cfg(test)]
#[path = "cursor_context_tests.rs"]
mod cursor_context_tests;
