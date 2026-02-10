use crate::{
    code_transform::CodeTransform,
    common::Span,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{
            CssModuleClassMapping, CssModuleInfo, CssParsedSpecialPseudoKind, CssParsedStyleBlock,
            CssParsedVBind, Event, ProcessedCssVBind, ProcessedStyleBlock,
        },
    },
};

/// CSS style plugin for the syntax_kai pipeline.
///
/// Consumes `CssParsedStyle` events (from css_parser) and applies transformations:
/// - **Scoped CSS**: Inserts `[data-v-{scope_id}]` attribute selectors
/// - **CSS v-bind()**: Replaces `v-bind(expr)` with `var(--{scope_id}-{sanitized})`
/// - **CSS Modules**: Hashes class names and builds runtime mappings
///
/// Uses `CodeTransform` for all modifications, preserving source positions.
pub struct CssStylePlugin<'alloc> {
    alloc: &'alloc oxc_allocator::Allocator,
    /// Scope ID for scoped styles, pre-computed by builder from component name hash.
    scope_id: Option<[u8; 8]>,
    /// Component ID for CSS module class hashing.
    component_id: Option<[u8; 8]>,
    /// Counter for CSS module hash uniqueness across multiple module blocks.
    module_class_counter: usize,
}

impl<'alloc> CssStylePlugin<'alloc> {
    pub fn new(alloc: &'alloc oxc_allocator::Allocator) -> Self {
        Self {
            alloc,
            scope_id: None,
            component_id: None,
            module_class_counter: 0,
        }
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    pub fn set_component_id(&mut self, component_id: [u8; 8]) {
        self.component_id = Some(component_id);
    }

    fn process_parsed_style(
        &mut self,
        parsed: CssParsedStyleBlock,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> ProcessedStyleBlock {
        let content = parsed.content;
        let mut transformed_css: Option<Vec<u8>> = None;
        let mut v_bind_expressions: Vec<ProcessedCssVBind> = Vec::new();
        let mut module_info: Option<CssModuleInfo> = None;

        if let Some(content_span) = content {
            let css_str = &ctx.input[content_span.start as usize..content_span.end as usize];
            let content_offset = content_span.start;

            // Create a CodeTransform for the CSS content
            let mut ct = CodeTransform::new(css_str, self.alloc);
            let mut modified = false;

            // 1. Scoped CSS: insert [data-v-{scope_id}] in selectors
            if parsed.scoped {
                if let Some(scope_id) = &self.scope_id {
                    let scope_attr = format!(
                        "[data-v-{}]",
                        std::str::from_utf8(scope_id).unwrap_or("00000000")
                    );

                    for rule in &parsed.rules {
                        for selector in &rule.selectors {
                            apply_scoped_selector(
                                &mut ct,
                                selector,
                                content_offset,
                                &scope_attr,
                                ctx.bytes,
                            );
                        }
                    }
                    modified = true;
                }
            }

            // 2. v-bind(): replace with var(--{scope_id}-{sanitized})
            if !parsed.v_binds.is_empty() {
                for vb in &parsed.v_binds {
                    let processed = apply_v_bind_replacement(
                        &mut ct,
                        vb,
                        content_offset,
                        &self.scope_id,
                        ctx.bytes,
                    );
                    v_bind_expressions.push(processed);
                }
                modified = true;
            }

            // 3. CSS Modules: hash class names
            if parsed.module.is_some() {
                let component_id = self.component_id.unwrap_or([b'0'; 8]);
                let (info, did_modify) = apply_css_modules(
                    &mut ct,
                    &parsed.classes,
                    content_offset,
                    &parsed.module,
                    &component_id,
                    &mut self.module_class_counter,
                    ctx.bytes,
                );
                module_info = Some(info);
                if did_modify {
                    modified = true;
                }
            }

            if modified {
                transformed_css = Some(ct.to_string().into_bytes());
            }
        }

        ProcessedStyleBlock {
            lang: parsed.lang,
            scoped: parsed.scoped,
            module: module_info,
            transformed_css,
            v_bind_expressions,
            compiled_start: parsed.compiled_start,
            compiled_end: parsed.compiled_end,
        }
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for CssStylePlugin<'alloc> {
    fn name(&self) -> &str {
        "css_style"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::CssParsedStyle(parsed) => {
                let processed = self.process_parsed_style(parsed, ctx);
                SyntaxResult::Replace(Event::ProcessedStyle(processed))
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

// =============================================================================
// Scoped CSS transformation
// =============================================================================

/// Apply scoped attribute to a single parsed selector using CodeTransform.
fn apply_scoped_selector(
    ct: &mut CodeTransform<'_>,
    selector: &crate::syntax_kai::types::CssParsedSelector,
    content_offset: u32,
    scope_attr: &str,
    bytes: &[u8],
) {
    let sel_text = &bytes[selector.span.start as usize..selector.span.end as usize];

    // Check for special pseudos first — only the first special pseudo is relevant
    if let Some(special) = selector.specials.first() {
        match special.kind {
            CssParsedSpecialPseudoKind::Global => {
                // :global(.class) → .class — remove the :global() wrapper
                if let Some(inner) = special.inner {
                    let inner_text = &bytes[inner.start as usize..inner.end as usize];
                    let inner_str = std::str::from_utf8(inner_text).unwrap_or("");
                    ct.overwrite(
                        special.span.start - content_offset,
                        special.span.end - content_offset,
                        inner_str,
                    );
                }
                return; // No scoping for :global
            }
            CssParsedSpecialPseudoKind::Deep => {
                // :deep(.inner) → [scope] .inner
                // .parent :deep(.inner) → .parent[scope] .inner
                let deep_local_start = special.span.start - content_offset;
                let deep_local_end = special.span.end - content_offset;

                // Check if there's a parent selector before :deep
                let sel_local_start = selector.span.start - content_offset;
                let before_deep = &bytes[selector.span.start as usize..special.span.start as usize];
                let before_trimmed = trim_bytes(before_deep);

                if before_trimmed.is_empty() {
                    // :deep(.inner) at start → [scope] .inner
                    if let Some(inner) = special.inner {
                        let inner_text = &bytes[inner.start as usize..inner.end as usize];
                        let inner_str = std::str::from_utf8(inner_text).unwrap_or("");
                        let replacement = format!("{} {}", scope_attr, inner_str);
                        ct.overwrite(deep_local_start, deep_local_end, &replacement);
                    }
                } else {
                    // .parent :deep(.inner) → .parent[scope] .inner
                    // Find end of parent selector (before whitespace + :deep)
                    let before_str = std::str::from_utf8(before_trimmed).unwrap_or("");
                    if let Some(inner) = special.inner {
                        let inner_text = &bytes[inner.start as usize..inner.end as usize];
                        let inner_str = std::str::from_utf8(inner_text).unwrap_or("");
                        let replacement = format!("{}{} {}", before_str, scope_attr, inner_str);
                        ct.overwrite(sel_local_start, deep_local_end, &replacement);
                    }
                }
                return;
            }
            CssParsedSpecialPseudoKind::Slotted => {
                // :slotted(.slot) → .slot[scope-s]
                if let Some(inner) = special.inner {
                    let inner_text = &bytes[inner.start as usize..inner.end as usize];
                    let inner_str = std::str::from_utf8(inner_text).unwrap_or("");
                    let slotted_scope = scope_attr.replace(']', "-s]");
                    let replacement = format!("{}{}", inner_str, slotted_scope);
                    ct.overwrite(
                        special.span.start - content_offset,
                        special.span.end - content_offset,
                        &replacement,
                    );
                }
                return;
            }
        }
    }

    // Normal selector: scope each compound selector
    // Split by combinators and spaces, append [scope] to each compound selector
    scope_selector_with_ct(
        ct,
        sel_text,
        selector.span.start - content_offset,
        scope_attr,
    );
}

/// Scope a normal selector (no special pseudos) using CodeTransform.
fn scope_selector_with_ct(
    ct: &mut CodeTransform<'_>,
    sel_text: &[u8],
    sel_offset: u32,
    scope_attr: &str,
) {
    let parts = split_by_combinators(sel_text);

    for part in &parts {
        if let SelectorPart::SimpleSelector(sel, local_start) = part {
            let (base, _pseudo) = split_pseudo(sel);

            // Insert [scope] after the base part, before the pseudo part
            let insert_pos = sel_offset + *local_start as u32 + base.len() as u32;
            ct.append_left(insert_pos, scope_attr);
        }
    }
}

/// Split a trimmed selector into parts: simple selectors, combinators, spaces.
fn split_by_combinators(selector: &[u8]) -> Vec<SelectorPart<'_>> {
    let mut parts = Vec::new();
    let mut i = 0;
    let len = selector.len();

    while i < len {
        // Skip whitespace
        while i < len && is_ws(selector[i]) {
            i += 1;
        }
        if i >= len {
            break;
        }

        // Combinator
        if selector[i] == b'>' || selector[i] == b'+' || selector[i] == b'~' {
            parts.push(SelectorPart::Combinator(selector[i]));
            i += 1;
            continue;
        }

        // Simple selector
        let start = i;
        while i < len && !is_ws(selector[i]) && !is_combinator(selector[i]) {
            if selector[i] == b'(' {
                let mut depth = 1u32;
                i += 1;
                while i < len && depth > 0 {
                    match selector[i] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                continue;
            }
            if selector[i] == b'[' {
                while i < len && selector[i] != b']' {
                    i += 1;
                }
                if i < len {
                    i += 1;
                }
                continue;
            }
            i += 1;
        }

        if i > start {
            if !parts.is_empty() && matches!(parts.last(), Some(SelectorPart::SimpleSelector(..))) {
                parts.push(SelectorPart::Space);
            }
            parts.push(SelectorPart::SimpleSelector(&selector[start..i], start));
        }
    }

    parts
}

#[derive(Debug)]
enum SelectorPart<'a> {
    SimpleSelector(&'a [u8], usize), // (bytes, local_start_offset)
    Combinator(#[allow(dead_code)] u8),
    Space,
}

/// Split a simple selector into base and pseudo parts.
/// E.g., `.btn:hover` → (`.btn`, `:hover`)
fn split_pseudo(selector: &[u8]) -> (&[u8], &[u8]) {
    let mut i = selector.len();
    while i > 0 {
        i -= 1;
        if selector[i] == b':' {
            let pseudo_start = if i > 0 && selector[i - 1] == b':' {
                i - 1
            } else {
                i
            };
            let before = &selector[..pseudo_start];
            if !before.is_empty() {
                return (before, &selector[pseudo_start..]);
            }
        }
    }
    (selector, b"")
}

// =============================================================================
// v-bind() transformation
// =============================================================================

/// Replace a single v-bind() expression with var(--{scope_id}-{sanitized}).
fn apply_v_bind_replacement(
    ct: &mut CodeTransform<'_>,
    vb: &CssParsedVBind,
    content_offset: u32,
    scope_id: &Option<[u8; 8]>,
    bytes: &[u8],
) -> ProcessedCssVBind {
    let expr_bytes = &bytes[vb.expression.start as usize..vb.expression.end as usize];

    let scope = scope_id.unwrap_or([b'0'; 8]);
    let mut var_name = Vec::with_capacity(2 + 8 + 1 + expr_bytes.len());
    var_name.extend_from_slice(b"--");
    var_name.extend_from_slice(&scope);
    var_name.push(b'-');
    for &b in expr_bytes {
        match b {
            b'.' | b' ' => var_name.push(b'-'),
            b'\'' | b'"' => {}
            _ => var_name.push(b),
        }
    }

    let var_name_str = std::str::from_utf8(&var_name).unwrap_or("--unknown");
    let replacement = format!("var({})", var_name_str);

    ct.overwrite(
        vb.full_span.start - content_offset,
        vb.full_span.end - content_offset,
        &replacement,
    );

    ProcessedCssVBind {
        expression: vb.expression,
        var_name,
        css_start: vb.full_span.start,
        css_end: vb.full_span.end,
    }
}

// =============================================================================
// CSS Modules transformation
// =============================================================================

/// Hash class names for CSS Modules.
fn apply_css_modules(
    ct: &mut CodeTransform<'_>,
    classes: &[crate::syntax_kai::types::CssParsedClass],
    content_offset: u32,
    module_attr: &Option<Span>,
    component_id: &[u8; 8],
    counter: &mut usize,
    bytes: &[u8],
) -> (CssModuleInfo, bool) {
    let mut mappings: Vec<CssModuleClassMapping> = Vec::new();
    let mut did_modify = false;

    for cls in classes {
        let class_name = &bytes[cls.name_span.start as usize..cls.name_span.end as usize];

        let mut hashed = Vec::with_capacity(1 + class_name.len() + 1 + 8 + 4);
        hashed.push(b'_');
        hashed.extend_from_slice(class_name);
        hashed.push(b'_');
        hashed.extend_from_slice(component_id);
        hashed.extend_from_slice(counter.to_string().as_bytes());
        *counter += 1;

        let hashed_str = std::str::from_utf8(&hashed).unwrap_or("_unknown_");

        // Overwrite the class name (just the name after the `.`)
        ct.overwrite(
            cls.name_span.start - content_offset,
            cls.name_span.end - content_offset,
            hashed_str,
        );

        mappings.push(CssModuleClassMapping {
            original: cls.name_span,
            hashed,
        });

        did_modify = true;
    }

    let custom_name = module_attr.and_then(|span| {
        if span.start == 0 && span.end == 0 {
            None
        } else {
            Some(span)
        }
    });

    (
        CssModuleInfo {
            custom_name,
            classes: mappings,
        },
        did_modify,
    )
}

// =============================================================================
// Helpers
// =============================================================================

fn trim_bytes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| !is_ws(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| !is_ws(b))
        .map_or(start, |e| e + 1);
    &bytes[start..end]
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn is_combinator(b: u8) -> bool {
    b == b'>' || b == b'+' || b == b'~'
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::css_parser::css_parser::CssParserPlugin;
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;

    /// Run input through tokenizer → syntax → element_compiler → css_parser → css_style pipeline.
    fn process_style_events(
        input: &str,
        scope_id: Option<[u8; 8]>,
        component_id: Option<[u8; 8]>,
    ) -> Vec<String> {
        let allocator = oxc_allocator::Allocator::new();

        let mut tokenizer_events = Vec::new();
        tokenize(input.as_bytes(), |event| tokenizer_events.push(event));

        let options = SyntaxPluginOptions::default();
        let mut ctx = SyntaxPluginContext {
            input,
            bytes: input.as_bytes(),
            options: &options,
        };

        let mut events_storage: Vec<Event<'_>> = Vec::new();
        let ptr = &mut events_storage as *mut Vec<Event<'_>>;
        {
            let mut syntax = Syntax::new(unsafe { &mut *ptr }, false);
            for event in &tokenizer_events {
                syntax.handle(event, &mut ctx);
            }
        }

        // element_compiler
        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // css_parser
        let mut parser = CssParserPlugin::new();
        let mut parsed = Vec::new();
        for event in compiled {
            match parser.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => parsed.push(e),
                SyntaxResult::Drop => {}
            }
        }

        // css_style
        let mut css = CssStylePlugin::new(&allocator);
        if let Some(sid) = scope_id {
            css.set_scope_id(sid);
        }
        if let Some(cid) = component_id {
            css.set_component_id(cid);
        }

        let mut result = Vec::new();
        for event in parsed {
            match css.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => result.push(e),
                SyntaxResult::Drop => {}
            }
        }

        result
            .iter()
            .map(|e| match e {
                Event::ProcessedStyle(ps) => {
                    let css_str = ps
                        .transformed_css
                        .as_ref()
                        .map(|c| String::from_utf8_lossy(c).to_string())
                        .unwrap_or_else(|| "None".to_string());
                    format!(
                        "ProcessedStyle(scoped={}, module={}, vbinds={}, css={})",
                        ps.scoped,
                        ps.module.is_some(),
                        ps.v_bind_expressions.len(),
                        css_str
                    )
                }
                Event::Text(_) => "Text".to_string(),
                _ => format!("{:?}", std::mem::discriminant(e)),
            })
            .collect()
    }

    #[test]
    fn test_plain_style_produces_processed_style() {
        let events = process_style_events("<style>.box { color: red; }</style>", None, None);
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .expect("Expected ProcessedStyle");
        assert!(ps.contains("scoped=false"));
        assert!(ps.contains("css=None"));
    }

    #[test]
    fn test_non_style_events_pass_through() {
        let events = process_style_events("<template>hello</template>", None, None);
        assert!(events.iter().any(|e| e == "Text"));
    }

    #[test]
    fn test_scoped_class_selector() {
        let events = process_style_events(
            "<style scoped>.box { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("[data-v-a1b2c3d4]"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_element_selector() {
        let events = process_style_events(
            "<style scoped>div { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("div[data-v-a1b2c3d4]"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_pseudo_class_ordering() {
        let events = process_style_events(
            "<style scoped>.btn:hover { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains(".btn[data-v-a1b2c3d4]:hover"), "got: {}", ps);
    }

    #[test]
    fn test_scoped_pseudo_element_ordering() {
        let events = process_style_events(
            "<style scoped>.text::before { content: ''; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains(".text[data-v-a1b2c3d4]::before"), "got: {}", ps);
    }

    #[test]
    fn test_no_scope_id_no_transform() {
        let events = process_style_events("<style scoped>.box { color: red; }</style>", None, None);
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("css=None"), "got: {}", ps);
    }

    #[test]
    fn test_v_bind_simple() {
        let events = process_style_events(
            "<style scoped>.box { color: v-bind(color); }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("vbinds=1"), "got: {}", ps);
        assert!(ps.contains("var(--a1b2c3d4-color)"), "got: {}", ps);
    }

    #[test]
    fn test_v_bind_dotted() {
        let events = process_style_events(
            "<style scoped>.box { color: v-bind('theme.color'); }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("var(--a1b2c3d4-theme-color)"), "got: {}", ps);
    }

    #[test]
    fn test_module_default_name() {
        let events = process_style_events(
            "<style module>.btn { color: red; }</style>",
            None,
            Some(*b"comp1234"),
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("module=true"), "got: {}", ps);
    }

    #[test]
    fn test_module_class_hashing() {
        let events = process_style_events(
            "<style module>.btn { color: red; }</style>",
            None,
            Some(*b"comp1234"),
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        assert!(ps.contains("._btn_comp1234"), "got: {}", ps);
    }

    #[test]
    fn test_empty_style_block() {
        let events = process_style_events("<style scoped></style>", Some(*b"a1b2c3d4"), None);
        assert!(
            events.iter().any(|e| e.starts_with("ProcessedStyle(")),
            "{:?}",
            events
        );
    }

    #[test]
    fn test_scoped_selector_list() {
        let events = process_style_events(
            "<style scoped>.a, .b { color: red; }</style>",
            Some(*b"a1b2c3d4"),
            None,
        );
        let ps = events
            .iter()
            .find(|e| e.starts_with("ProcessedStyle("))
            .unwrap();
        let scope_count = ps.matches("[data-v-a1b2c3d4]").count();
        assert!(
            scope_count >= 2,
            "Both selectors should be scoped, found {} in: {}",
            scope_count,
            ps
        );
    }

    #[test]
    fn test_split_pseudo() {
        assert_eq!(
            split_pseudo(b".btn:hover"),
            (b".btn" as &[u8], b":hover" as &[u8])
        );
        assert_eq!(
            split_pseudo(b".text::before"),
            (b".text" as &[u8], b"::before" as &[u8])
        );
        assert_eq!(split_pseudo(b"div"), (b"div" as &[u8], b"" as &[u8]));
    }
}
