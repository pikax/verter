use crate::{
    common::Span,
    syntax_kai::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::{
            CompiledRootStyleEnd, CompiledRootStyleStart, CssModuleClassMapping, CssModuleInfo,
            Event, ProcessedCssVBind, ProcessedStyleBlock,
        },
    },
};

/// CSS style plugin for the syntax_kai pipeline.
///
/// Processes `CompiledStyleStart`/`CompiledStyleEnd` events to handle:
/// - **Scoped CSS**: Transforms selectors with `[data-v-{scope_id}]` attribute selectors
/// - **CSS v-bind()**: Extracts expressions and replaces with CSS custom properties
/// - **CSS Modules**: Hashes class names and builds runtime mappings
pub struct CssStylePlugin<'alloc> {
    /// Scope ID for scoped styles, pre-computed by builder from component name hash.
    scope_id: Option<[u8; 8]>,
    /// Component ID for CSS module class hashing.
    component_id: Option<[u8; 8]>,
    /// Buffered CompiledStyleStart (set on Start, consumed on End).
    current_start: Option<CompiledRootStyleStart<'alloc>>,
    /// Counter for CSS module hash uniqueness across multiple module blocks.
    module_class_counter: usize,
}

impl<'alloc> Default for CssStylePlugin<'alloc> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'alloc> CssStylePlugin<'alloc> {
    pub fn new() -> Self {
        Self {
            scope_id: None,
            component_id: None,
            current_start: None,
            module_class_counter: 0,
        }
    }

    pub fn set_scope_id(&mut self, scope_id: [u8; 8]) {
        self.scope_id = Some(scope_id);
    }

    pub fn set_component_id(&mut self, component_id: [u8; 8]) {
        self.component_id = Some(component_id);
    }

    fn process_style_block(
        &mut self,
        start: CompiledRootStyleStart<'alloc>,
        end: CompiledRootStyleEnd,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> ProcessedStyleBlock<'alloc> {
        let css_content = end
            .content
            .map(|c| &ctx.bytes[c.start as usize..c.end as usize]);
        let content_offset = end.content.map_or(0, |c| c.start);

        let mut transformed_css: Option<Vec<u8>> = None;
        let mut v_bind_expressions: Vec<ProcessedCssVBind> = Vec::new();
        let mut module_info: Option<CssModuleInfo> = None;

        // 1. Extract v-bind() expressions
        if let Some(css) = css_content {
            v_bind_expressions = extract_v_bind(css, content_offset, &self.scope_id);
        }

        // 2. Scoped CSS transformation
        if start.scoped {
            if let (Some(css), Some(scope_id)) = (css_content, &self.scope_id) {
                let mut output = Vec::with_capacity(css.len() + css.len() / 4);
                transform_scoped_css(css, scope_id, &mut output);

                // Apply v-bind replacements
                if !v_bind_expressions.is_empty() {
                    output = apply_v_bind_replacements(
                        &output,
                        css,
                        content_offset,
                        &v_bind_expressions,
                    );
                }

                transformed_css = Some(output);
            }
        }

        // 3. CSS Modules transformation
        if start.module.is_some() {
            if let Some(css) = css_content {
                let component_id = self.component_id.unwrap_or([b'0'; 8]);
                let (module, hashed_css) = transform_css_modules(
                    css,
                    content_offset,
                    &start.module,
                    &component_id,
                    &mut self.module_class_counter,
                );
                module_info = Some(module);

                let base = transformed_css.take().unwrap_or_else(|| css.to_vec());
                transformed_css = Some(hashed_css.unwrap_or(base));
            }
        }

        // 4. If only v-bind (no scoped, no modules), apply replacements
        if transformed_css.is_none() && !v_bind_expressions.is_empty() {
            if let Some(css) = css_content {
                let output =
                    apply_v_bind_replacements_raw(css, content_offset, &v_bind_expressions);
                transformed_css = Some(output);
            }
        }

        ProcessedStyleBlock {
            lang: start.lang,
            scoped: start.scoped,
            module: module_info,
            transformed_css,
            v_bind_expressions,
            compiled_start: start,
            compiled_end: end,
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
            Event::CompiledStyleStart(start) => {
                self.current_start = Some(start);
                SyntaxResult::Drop
            }
            Event::CompiledStyleEnd(end) => {
                if let Some(start) = self.current_start.take() {
                    let processed = self.process_style_block(start, end, ctx);
                    SyntaxResult::Replace(Event::ProcessedStyle(processed))
                } else {
                    SyntaxResult::Keep(Event::CompiledStyleEnd(end))
                }
            }
            other => SyntaxResult::Keep(other),
        }
    }
}

// --- v-bind extraction ---

/// Extract v-bind() expressions from CSS content.
fn extract_v_bind(css: &[u8], offset: u32, scope_id: &Option<[u8; 8]>) -> Vec<ProcessedCssVBind> {
    let mut results = Vec::new();
    let mut i = 0;

    while i < css.len() {
        if i + 7 <= css.len() && &css[i..i + 7] == b"v-bind(" {
            let start_pos = i;
            let expr_start = i + 7;
            let mut depth = 1u32;
            let mut j = expr_start;
            while j < css.len() && depth > 0 {
                match css[j] {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    _ => {}
                }
                if depth > 0 {
                    j += 1;
                }
            }

            if depth == 0 {
                let expr_end = j;
                let full_end = j + 1;

                let expr_bytes = &css[expr_start..expr_end];
                let trimmed = trim_bytes(expr_bytes);

                // Strip surrounding quotes
                let unquoted = if trimmed.len() >= 2
                    && ((trimmed[0] == b'\'' && trimmed[trimmed.len() - 1] == b'\'')
                        || (trimmed[0] == b'"' && trimmed[trimmed.len() - 1] == b'"'))
                {
                    &trimmed[1..trimmed.len() - 1]
                } else {
                    trimmed
                };

                let scope = scope_id.unwrap_or([b'0'; 8]);
                let mut var_name = Vec::with_capacity(2 + 8 + 1 + unquoted.len());
                var_name.extend_from_slice(b"--");
                var_name.extend_from_slice(&scope);
                var_name.push(b'-');
                for &b in unquoted {
                    match b {
                        b'.' | b' ' => var_name.push(b'-'),
                        b'\'' | b'"' => {}
                        _ => var_name.push(b),
                    }
                }

                results.push(ProcessedCssVBind {
                    expression: Span::new(offset + expr_start as u32, offset + expr_end as u32),
                    var_name,
                    css_start: offset + start_pos as u32,
                    css_end: offset + full_end as u32,
                });

                i = full_end;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }

    results
}

/// Apply v-bind() → var(--name) replacements to CSS.
fn apply_v_bind_replacements(
    _transformed: &[u8],
    original: &[u8],
    offset: u32,
    v_binds: &[ProcessedCssVBind],
) -> Vec<u8> {
    apply_v_bind_replacements_raw(original, offset, v_binds)
}

fn apply_v_bind_replacements_raw(
    css: &[u8],
    offset: u32,
    v_binds: &[ProcessedCssVBind],
) -> Vec<u8> {
    if v_binds.is_empty() {
        return css.to_vec();
    }

    let mut output = Vec::with_capacity(css.len());
    let mut last_end = 0usize;

    for vb in v_binds {
        let local_start = (vb.css_start - offset) as usize;
        let local_end = (vb.css_end - offset) as usize;
        output.extend_from_slice(&css[last_end..local_start]);
        output.extend_from_slice(b"var(");
        output.extend_from_slice(&vb.var_name);
        output.push(b')');
        last_end = local_end;
    }

    output.extend_from_slice(&css[last_end..]);
    output
}

// --- Scoped CSS transformation ---

fn transform_scoped_css(css: &[u8], scope_id: &[u8; 8], output: &mut Vec<u8>) {
    let scope_attr = format!(
        "[data-v-{}]",
        std::str::from_utf8(scope_id).unwrap_or("00000000")
    );
    let scope_bytes = scope_attr.as_bytes();
    let mut i = 0;
    let len = css.len();

    while i < len {
        // Skip CSS comments
        if i + 1 < len && css[i] == b'/' && css[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(css[i] == b'*' && css[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < len {
                i += 2;
            }
            output.extend_from_slice(&css[start..i]);
            continue;
        }

        // Skip strings
        if css[i] == b'"' || css[i] == b'\'' {
            let quote = css[i];
            output.push(css[i]);
            i += 1;
            while i < len && css[i] != quote {
                if css[i] == b'\\' && i + 1 < len {
                    output.push(css[i]);
                    i += 1;
                }
                output.push(css[i]);
                i += 1;
            }
            if i < len {
                output.push(css[i]);
                i += 1;
            }
            continue;
        }

        // At-rules
        if css[i] == b'@' {
            while i < len && css[i] != b'{' && css[i] != b';' {
                output.push(css[i]);
                i += 1;
            }
            if i < len && css[i] == b';' {
                output.push(css[i]);
                i += 1;
            }
            continue;
        }

        // Collect selector up to '{'
        if is_selector_start_char(css[i]) {
            let selector_start = i;
            while i < len && css[i] != b'{' {
                i += 1;
            }

            if i > selector_start {
                let selector_bytes = &css[selector_start..i];
                let scoped = scope_selectors(selector_bytes, scope_bytes);
                output.extend_from_slice(&scoped);
            }

            if i < len {
                // Push '{' and skip to matching '}'
                output.push(css[i]);
                i += 1;
                let mut depth = 1u32;
                while i < len && depth > 0 {
                    if css[i] == b'{' {
                        depth += 1;
                    } else if css[i] == b'}' {
                        depth -= 1;
                    }
                    output.push(css[i]);
                    i += 1;
                }
            }
            continue;
        }

        output.push(css[i]);
        i += 1;
    }
}

/// Scope a selector list.
fn scope_selectors(selector_list: &[u8], scope_attr: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(selector_list.len() + scope_attr.len() * 2);

    for (idx, selector) in selector_list.split(|&b| b == b',').enumerate() {
        if idx > 0 {
            output.push(b',');
        }

        let trimmed = trim_bytes(selector);
        if trimmed.is_empty() {
            output.extend_from_slice(selector);
            continue;
        }

        // Check for :deep(), :global(), :slotted()
        if let Some(result) = handle_special_pseudo(trimmed, scope_attr) {
            let leading = leading_whitespace(selector);
            output.extend_from_slice(leading);
            output.extend_from_slice(&result);
        } else {
            scope_single_selector(selector, scope_attr, &mut output);
        }
    }

    output
}

fn handle_special_pseudo(selector: &[u8], scope_attr: &[u8]) -> Option<Vec<u8>> {
    // :global(.class) → .class
    if selector.starts_with(b":global(") {
        return extract_pseudo_inner(selector, b":global(").map(|inner| inner.to_vec());
    }

    // :deep(.inner) → [scope] .inner
    if selector.starts_with(b":deep(") {
        if let Some(inner) = extract_pseudo_inner(selector, b":deep(") {
            let mut result = Vec::new();
            result.extend_from_slice(scope_attr);
            result.push(b' ');
            result.extend_from_slice(inner);
            return Some(result);
        }
    }

    // .parent :deep(.inner) → .parent[scope] .inner
    if let Some(deep_pos) = find_subsequence(selector, b":deep(") {
        let before = trim_bytes(&selector[..deep_pos]);
        if !before.is_empty() {
            if let Some(inner) = extract_pseudo_inner(&selector[deep_pos..], b":deep(") {
                let mut result = Vec::new();
                result.extend_from_slice(before);
                result.extend_from_slice(scope_attr);
                result.push(b' ');
                result.extend_from_slice(inner);
                return Some(result);
            }
        }
    }

    // :slotted(.slot) → .slot[scope-s]
    if selector.starts_with(b":slotted(") {
        if let Some(inner) = extract_pseudo_inner(selector, b":slotted(") {
            let scope_str = std::str::from_utf8(scope_attr).unwrap_or("");
            // Replace last ] with -s]
            let slotted_scope = scope_str.replace(']', "-s]");
            let mut result = Vec::new();
            result.extend_from_slice(inner);
            result.extend_from_slice(slotted_scope.as_bytes());
            return Some(result);
        }
    }

    None
}

fn scope_single_selector(selector: &[u8], scope_attr: &[u8], output: &mut Vec<u8>) {
    let leading = leading_whitespace(selector);
    let trimmed = trim_bytes(selector);
    if trimmed.is_empty() {
        output.extend_from_slice(selector);
        return;
    }

    output.extend_from_slice(leading);

    let parts = split_by_combinators(trimmed);
    for part in &parts {
        match part {
            SelectorPart::Combinator(c) => {
                output.push(b' ');
                output.push(*c);
                output.push(b' ');
            }
            SelectorPart::Space => {
                output.push(b' ');
            }
            SelectorPart::SimpleSelector(sel) => {
                let (base, pseudo) = split_pseudo(sel);
                output.extend_from_slice(base);
                output.extend_from_slice(scope_attr);
                output.extend_from_slice(pseudo);
            }
        }
    }
}

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

#[derive(Debug)]
enum SelectorPart<'a> {
    SimpleSelector(&'a [u8]),
    Combinator(u8),
    Space,
}

fn split_by_combinators(selector: &[u8]) -> Vec<SelectorPart<'_>> {
    let mut parts = Vec::new();
    let mut i = 0;
    let len = selector.len();

    while i < len {
        while i < len && is_ws(selector[i]) {
            i += 1;
        }
        if i >= len {
            break;
        }

        if selector[i] == b'>' || selector[i] == b'+' || selector[i] == b'~' {
            parts.push(SelectorPart::Combinator(selector[i]));
            i += 1;
            continue;
        }

        let start = i;
        while i < len && !is_ws(selector[i]) && !is_combinator(selector[i]) {
            if selector[i] == b'(' {
                let mut depth = 1;
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
            i += 1;
        }

        if i > start {
            if !parts.is_empty() && matches!(parts.last(), Some(SelectorPart::SimpleSelector(_))) {
                parts.push(SelectorPart::Space);
            }
            parts.push(SelectorPart::SimpleSelector(&selector[start..i]));
        }
    }

    parts
}

// --- CSS Modules transformation ---

fn transform_css_modules(
    css: &[u8],
    offset: u32,
    module_attr: &Option<Span>,
    component_id: &[u8; 8],
    counter: &mut usize,
) -> (CssModuleInfo, Option<Vec<u8>>) {
    let mut classes: Vec<CssModuleClassMapping> = Vec::new();
    let mut output = Vec::with_capacity(css.len());
    let mut i = 0;

    while i < css.len() {
        if css[i] == b'.' && (i == 0 || is_selector_context_char(css[i - 1])) {
            let class_start = i + 1;
            let mut class_end = class_start;
            while class_end < css.len() && is_css_ident_char(css[class_end]) {
                class_end += 1;
            }

            if class_end > class_start {
                let class_name = &css[class_start..class_end];

                let mut hashed = Vec::with_capacity(1 + class_name.len() + 1 + 8 + 4);
                hashed.push(b'_');
                hashed.extend_from_slice(class_name);
                hashed.push(b'_');
                hashed.extend_from_slice(component_id);
                hashed.extend_from_slice(counter.to_string().as_bytes());
                *counter += 1;

                classes.push(CssModuleClassMapping {
                    original: Span::new(offset + class_start as u32, offset + class_end as u32),
                    hashed: hashed.clone(),
                });

                output.push(b'.');
                output.extend_from_slice(&hashed);
                i = class_end;
                continue;
            }
        }

        output.push(css[i]);
        i += 1;
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
            classes,
        },
        Some(output),
    )
}

// --- Helpers ---

fn extract_pseudo_inner<'a>(selector: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if !selector.starts_with(prefix) {
        return None;
    }
    let inner_start = prefix.len();
    let mut depth = 1u32;
    let mut i = inner_start;
    while i < selector.len() && depth > 0 {
        match selector[i] {
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            i += 1;
        }
    }
    if depth == 0 {
        Some(&selector[inner_start..i])
    } else {
        None
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn trim_bytes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| !is_ws(b)).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| !is_ws(b))
        .map_or(start, |e| e + 1);
    &bytes[start..end]
}

fn leading_whitespace(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| !is_ws(b)).unwrap_or(bytes.len());
    &bytes[..end]
}

fn is_ws(b: u8) -> bool {
    b == b' ' || b == b'\t' || b == b'\n' || b == b'\r'
}

fn is_combinator(b: u8) -> bool {
    b == b'>' || b == b'+' || b == b'~'
}

fn is_css_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn is_selector_start_char(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || b == b'.'
        || b == b'#'
        || b == b':'
        || b == b'['
        || b == b'*'
        || b == b'&'
}

fn is_selector_context_char(b: u8) -> bool {
    is_ws(b)
        || b == b','
        || b == b'{'
        || b == b'}'
        || b == b';'
        || b == b'>'
        || b == b'+'
        || b == b'~'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_kai::plugin::{SyntaxPluginContext, SyntaxPluginOptions};
    use crate::syntax_kai::plugins::element_compiler::element_compiler::ElementCompilerPlugin;
    use crate::syntax_kai::syntax::Syntax;
    use crate::tokenizer::byte::tokenize;

    fn process_style_events(
        input: &str,
        scope_id: Option<[u8; 8]>,
        component_id: Option<[u8; 8]>,
    ) -> Vec<String> {
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

        let mut ec = ElementCompilerPlugin::new();
        let mut compiled = Vec::new();
        for event in events_storage {
            match ec.process_event(event, &mut ctx) {
                SyntaxResult::Keep(e) | SyntaxResult::Replace(e) => compiled.push(e),
                SyntaxResult::Drop => {}
            }
        }

        let mut css = CssStylePlugin::new();
        if let Some(sid) = scope_id {
            css.set_scope_id(sid);
        }
        if let Some(cid) = component_id {
            css.set_component_id(cid);
        }

        let mut result = Vec::new();
        for event in compiled {
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
    fn test_compiled_style_start_is_dropped() {
        let events = process_style_events("<style>.box { }</style>", None, None);
        assert!(!events.iter().any(|e| e.contains("CompiledStyleStart")));
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
    fn test_trim_bytes() {
        assert_eq!(trim_bytes(b"  hello  "), b"hello");
        assert_eq!(trim_bytes(b"hello"), b"hello");
        assert_eq!(trim_bytes(b"  "), b"" as &[u8]);
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
}
