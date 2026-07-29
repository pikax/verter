use verter_parser::tokenizer::byte::vue_sfc_root_block_is_raw_text;

/// Lightweight SFC block scanner for LSP structural features.
///
/// Finds `<script>`, `<template>`, `<style>`, and custom block boundaries
/// by scanning the raw carrier source. Returns byte offsets for each block's
/// opening tag, content, and closing tag.
///
/// This is intentionally simple — it doesn't build a markup tree. Block close
/// tags are depth-balanced for markup-bearing blocks and first-close for
/// raw-text blocks; WHICH custom blocks count as markup is the carrier
/// decision [`CustomBlockContentKind`] encodes (Vue custom blocks are raw
/// text, matching `verter_parser`; Svelte root components are markup whose
/// same-name children nest). `<script>` / `<style>` are always raw text.
/// It's used for document symbols, folding ranges, and determining which
/// block the cursor is in.
///
/// A detected SFC block with its byte positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcBlock {
    /// Block tag name (e.g., "script", "template", "style", "i18n").
    pub tag_name: String,
    /// Byte offset of the '<' in the opening tag.
    pub open_tag_start: u32,
    /// Byte offset just past the '>' of the opening tag.
    pub open_tag_end: u32,
    /// Byte offset of the '<' in the closing tag.
    pub close_tag_start: u32,
    /// Byte offset just past the '>' of the closing tag.
    pub close_tag_end: u32,
    /// Raw attributes string from the opening tag (e.g., ` setup lang="ts"`).
    pub attrs_raw: String,
}

impl SfcBlock {
    /// Byte range of the block's inner content (between open and close tags).
    pub fn content_range(&self) -> (u32, u32) {
        (self.open_tag_end, self.close_tag_start)
    }

    /// Look up an attribute of this block's opening tag by EXACT name.
    ///
    /// Returns `None` when the attribute is absent, `Some(None)` for a present boolean attribute,
    /// and `Some(Some(value))` for a present valued attribute. Backed by the single
    /// [`scan_attr_region`] tokenizer, so a name like `notsetup` / `data-setup` / `mylang` is a
    /// distinct attribute and never satisfies a query for `setup` / `lang` (no substring sniffing).
    fn attr(&self, name: &str) -> Option<Option<&str>> {
        scan_attr_region(&self.attrs_raw)
            .into_iter()
            .find(|ra| self.attrs_raw[ra.name_start..ra.name_end] == *name)
            .map(|ra| {
                ra.value_start
                    .zip(ra.value_end)
                    .map(|(vs, ve)| &self.attrs_raw[vs..ve])
            })
    }

    /// Whether this is a `<script setup>` block (exact boolean `setup` attribute on a `script`).
    pub fn is_setup(&self) -> bool {
        self.tag_name == "script" && self.attr("setup").is_some()
    }

    /// The typed `lang` attribute value, if present.
    pub fn lang(&self) -> Option<&str> {
        self.attr("lang").flatten()
    }

    /// Whether this block has the exact `scoped` attribute.
    pub fn is_scoped(&self) -> bool {
        self.attr("scoped").is_some()
    }

    /// Whether this block has the exact `module` attribute.
    pub fn is_module(&self) -> bool {
        self.attr("module").is_some()
    }

    /// The `attrs` / `attributes` attribute value, if present (long form `attributes` preferred).
    pub fn attrs(&self) -> Option<&str> {
        self.attr("attributes")
            .flatten()
            .or_else(|| self.attr("attrs").flatten())
    }
}

/// One attribute parsed from an opening-tag attribute region. All offsets are RELATIVE to the
/// first byte of the region passed to [`scan_attr_region`].
#[derive(Debug, Clone, Copy)]
struct RawAttr {
    name_start: usize,
    name_end: usize,
    value_start: Option<usize>,
    value_end: Option<usize>,
}

/// The single SFC opening-tag attribute tokenizer.
///
/// `region` is the attribute text of an opening tag — everything AFTER the tag name. It may end
/// at a `>` / `/>` (the full `<tag …>` region minus the tag name, as [`parse_opening_tag`] passes)
/// or simply run to the end of the string ([`SfcBlock::attrs_raw`], which already excludes the
/// `>`). Attributes are matched by exact name boundaries (whitespace / `=` / `>` / `/` delimited),
/// so a name like `notsetup`, `data-setup`, or `mylang` is a single distinct attribute. Both
/// [`parse_opening_tag`] (which rebases the offsets to document-absolute) and [`SfcBlock`]'s typed
/// accessors read from this one tokenizer — there is no second substring-based attribute reader.
fn scan_attr_region(region: &str) -> Vec<RawAttr> {
    let bytes = region.as_bytes();
    let len = bytes.len();
    let mut attrs = Vec::new();
    let mut i = 0;

    while i < len && bytes[i] != b'>' {
        // Skip whitespace before the next attribute.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len || bytes[i] == b'>' || bytes[i] == b'/' {
            break;
        }

        // Read the attribute name.
        let name_start = i;
        while i < len
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name_end = i;
        if name_start == name_end {
            i += 1;
            continue;
        }

        // Skip whitespace between the name and a possible '='.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        if i < len && bytes[i] == b'=' {
            i += 1;
            // Skip whitespace after '='.
            while i < len && bytes[i].is_ascii_whitespace() {
                i += 1;
            }

            if i < len && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let val_start = i;
                while i < len && bytes[i] != quote {
                    i += 1;
                }
                let val_end = i;
                if i < len {
                    i += 1; // skip the closing quote
                }
                attrs.push(RawAttr {
                    name_start,
                    name_end,
                    value_start: Some(val_start),
                    value_end: Some(val_end),
                });
            } else {
                // Unquoted value.
                let val_start = i;
                while i < len
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'>'
                    && bytes[i] != b'/'
                {
                    i += 1;
                }
                let val_end = i;
                attrs.push(RawAttr {
                    name_start,
                    name_end,
                    value_start: Some(val_start),
                    value_end: Some(val_end),
                });
            }
        } else {
            // Boolean attribute (no value).
            attrs.push(RawAttr {
                name_start,
                name_end,
                value_start: None,
                value_end: None,
            });
        }
    }

    attrs
}

// ── Cursor Context ──────────────────────────────────────────────────────────

/// Where the cursor is in the SFC structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfcCursorContext {
    /// Inside a block's content (between open and close tags).
    BlockContent { block_index: usize },
    /// On the opening tag of a block (from `<` to `>`).
    OpeningTag { block_index: usize },
    /// On the closing tag of a block (from `</` to `>`).
    ClosingTag { block_index: usize },
    /// Outside all blocks (root level of the SFC).
    RootLevel,
}

/// Classify the cursor position within the SFC structure.
pub fn classify_cursor(offset: u32, blocks: &[SfcBlock]) -> SfcCursorContext {
    for (i, block) in blocks.iter().enumerate() {
        if offset >= block.open_tag_start && offset < block.open_tag_end {
            return SfcCursorContext::OpeningTag { block_index: i };
        }
        if offset >= block.close_tag_start && offset < block.close_tag_end {
            return SfcCursorContext::ClosingTag { block_index: i };
        }
        let (content_start, content_end) = block.content_range();
        if offset >= content_start && offset < content_end {
            return SfcCursorContext::BlockContent { block_index: i };
        }
    }
    SfcCursorContext::RootLevel
}

/// A parsed attribute from an SFC opening tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttr {
    /// Attribute name (e.g., "lang", "setup", "scoped").
    pub name: String,
    /// Attribute value, if present (e.g., "ts", "scss"). `None` for boolean attrs.
    pub value: Option<String>,
    /// Absolute byte offset of the attribute name start.
    pub name_start: u32,
    /// Absolute byte offset past the attribute name end.
    pub name_end: u32,
    /// Absolute byte offset of the value start (inside quotes), if present.
    pub value_start: Option<u32>,
    /// Absolute byte offset past the value end (inside quotes), if present.
    pub value_end: Option<u32>,
}

/// Context for the opening tag region of an SFC block.
#[derive(Debug, Clone)]
pub struct OpeningTagContext {
    /// Tag name (e.g., "script", "template", "style").
    pub tag_name: String,
    /// Absolute byte offset of the tag name start (after `<`).
    pub tag_name_start: u32,
    /// Absolute byte offset past the tag name end.
    pub tag_name_end: u32,
    /// Parsed attributes on the opening tag.
    pub attrs: Vec<ParsedAttr>,
}

/// Parse the opening tag region of an SFC block to extract tag name and attributes
/// with absolute byte offsets.
pub fn parse_opening_tag(source: &str, block: &SfcBlock) -> OpeningTagContext {
    let start = block.open_tag_start as usize;
    let end = block.open_tag_end as usize;
    let region = &source[start..end];
    let bytes = region.as_bytes();

    // Skip '<'
    let mut i = 1;

    // Read tag name
    let tag_name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    let tag_name_end = i;
    let tag_name = region[tag_name_start..tag_name_end].to_string();

    // Parse the attribute region (everything after the tag name) with the shared tokenizer,
    // rebasing its region-relative offsets to document-absolute ones.
    let attrs_base = start + tag_name_end;
    let attrs = scan_attr_region(&region[tag_name_end..])
        .into_iter()
        .map(|ra| ParsedAttr {
            name: region[tag_name_end + ra.name_start..tag_name_end + ra.name_end].to_string(),
            value: ra
                .value_start
                .zip(ra.value_end)
                .map(|(vs, ve)| region[tag_name_end + vs..tag_name_end + ve].to_string()),
            name_start: (attrs_base + ra.name_start) as u32,
            name_end: (attrs_base + ra.name_end) as u32,
            value_start: ra.value_start.map(|vs| (attrs_base + vs) as u32),
            value_end: ra.value_end.map(|ve| (attrs_base + ve) as u32),
        })
        .collect();

    OpeningTagContext {
        tag_name,
        tag_name_start: (start + tag_name_start) as u32,
        tag_name_end: (start + tag_name_end) as u32,
        attrs,
    }
}

/// How a CUSTOM (non-`script`/`template`/`style`) top-level block's content is
/// interpreted when finding its close tag. The two variants are the two carrier
/// semantics this scanner serves:
///
/// * [`RawText`](Self::RawText) — the Vue SFC rule: at the root of a Vue SFC
///   only `<template>` hosts markup; every custom block (`<docs>`, `<i18n>`, …)
///   is raw text (RCDATA) whose FIRST same-name close ends the block. The
///   per-tag decision is the SHARED parser predicate
///   [`vue_sfc_root_block_is_raw_text`] — the exact rule `verter_parser`'s SFC
///   tokenizer applies when entering RCDATA — so the scanner and the parser
///   cannot diverge on which Vue blocks nest.
/// * [`Markup`](Self::Markup) — the Svelte rule: root markup lives at the SFC
///   root, so a non-`script`/`style` paired tag (`<Card>`) is a component whose
///   same-name children NEST; its close is depth-balanced.
///
/// `<script>` / `<style>` are raw text under BOTH kinds, and `<template>` is
/// depth-balanced under both (it is the Vue markup host, and an ordinary
/// element for Svelte).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomBlockContentKind {
    /// Vue custom-block semantics: raw text, first same-name close wins.
    RawText,
    /// Svelte root-markup semantics: components nest, close is depth-balanced.
    Markup,
}

/// Resolve the [`CustomBlockContentKind`] for a document.
///
/// Mirrors the resolution `DocumentRegistry::document_file_language` performs:
/// the editor `language_id` is authoritative for a framework carrier when
/// available (an in-memory carrier document may not carry a `.vue` / `.svelte`
/// path); otherwise the canonical path classifies through the host's static
/// classifier. Svelte maps to [`Markup`](CustomBlockContentKind::Markup);
/// every other language — Vue, plain scripts, unknown carriers — maps to
/// [`RawText`](CustomBlockContentKind::RawText), the fail-conservative Vue rule
/// (raw text never swallows a following block, so an unknown carrier can lose
/// span fidelity but never block discovery).
pub fn custom_block_content_kind(
    language_id: Option<&str>,
    canonical_id: &str,
) -> CustomBlockContentKind {
    let registry = verter_session::LanguageRegistry::global();
    let language = language_id
        .and_then(|id| registry.carrier_for_editor_language_id(id))
        .unwrap_or_else(|| registry.classify_static(canonical_id).static_resolution());
    if language.is_svelte() {
        CustomBlockContentKind::Markup
    } else {
        CustomBlockContentKind::RawText
    }
}

/// Scan an open document's SFC blocks with its carrier-resolved
/// [`CustomBlockContentKind`] ([`custom_block_content_kind`] over the
/// document's `language_id` + `canonical_id`).
///
/// This is the entry point LSP feature handlers use for an open document, so a
/// Svelte carrier gets depth-balanced component blocks while a Vue carrier
/// keeps parser-faithful raw-text custom blocks.
pub fn scan_sfc_blocks_for_document(doc: &super::DocumentState) -> Vec<SfcBlock> {
    scan_sfc_blocks_with(
        &doc.source,
        custom_block_content_kind(Some(&doc.language_id), &doc.canonical_id),
    )
}

/// Scan SFC source text and return all top-level blocks found, treating custom
/// blocks as Vue raw text ([`CustomBlockContentKind::RawText`]).
///
/// Blocks are returned in source order. Self-closing tags (e.g., `<template />`)
/// are not treated as blocks since they have no content.
///
/// Callers with a resolved document use [`scan_sfc_blocks_for_document`];
/// callers that KNOW the source is Svelte root markup pass
/// [`CustomBlockContentKind::Markup`] to [`scan_sfc_blocks_with`]. This
/// carrier-blind default uses the Vue rule because it is discovery-safe: raw
/// text can mis-span a Svelte component block, but balancing a Vue custom
/// block can swallow the `<script setup>` that follows its first close.
pub fn scan_sfc_blocks(source: &str) -> Vec<SfcBlock> {
    scan_sfc_blocks_with(source, CustomBlockContentKind::RawText)
}

/// [`scan_sfc_blocks`] with an explicit custom-block content interpretation.
pub fn scan_sfc_blocks_with(source: &str, custom_blocks: CustomBlockContentKind) -> Vec<SfcBlock> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < len {
        // Look for '<' that starts a tag (skip comments, text, etc.)
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        // Skip comments <!-- ... -->
        if i + 3 < len && &bytes[i..i + 4] == b"<!--" {
            if let Some(end) = find_bytes(bytes, i + 4, b"-->") {
                i = end + 3;
            } else {
                i += 4;
            }
            continue;
        }

        // Skip closing tags (we handle them when matching open tags)
        if i + 1 < len && bytes[i + 1] == b'/' {
            i += 1;
            continue;
        }

        // Try to parse an opening tag
        let tag_start = i;
        i += 1; // skip '<'

        // Read tag name (letters, digits, hyphens)
        let name_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
            i += 1;
        }
        if i == name_start {
            continue; // empty tag name
        }
        let raw_tag_name = &source[name_start..i];

        // Only match known top-level SFC tags and custom blocks
        // Skip DOCTYPE, html, head, body, etc.
        if !is_sfc_tag(raw_tag_name) {
            continue;
        }

        // Normalize the built-in SFC tags (`script` / `template` / `style`) to
        // their canonical lowercase form so a case-variant `<SCRIPT>` stores
        // `tag_name == "script"` and downstream lowercase matches (the
        // auto-close markup-region gate) recognize it. Custom blocks keep their
        // authored spelling.
        let tag_name: String = canonical_builtin_sfc_tag(raw_tag_name)
            .map(|c| c.to_string())
            .unwrap_or_else(|| raw_tag_name.to_string());

        // Read attributes until '>' or '/>'
        let attrs_start = i;
        let mut self_closing = false;
        while i < len {
            if bytes[i] == b'>' {
                if i > 0 && bytes[i - 1] == b'/' {
                    self_closing = true;
                }
                break;
            }
            // Skip quoted attribute values
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    i += 1;
                }
            }
            i += 1;
        }
        if i >= len {
            break; // unclosed tag
        }

        let attrs_end = if self_closing { i - 1 } else { i };
        let attrs_raw = source[attrs_start..attrs_end].to_string();
        let open_tag_end = (i + 1) as u32; // past the '>'
        i += 1;

        if self_closing {
            continue; // self-closing blocks have no content
        }

        // Find the matching closing tag. Markup-bearing blocks may nest
        // same-name tags (a Vue slot `<template #row>`, a nested Svelte
        // component), so their close is DEPTH-BALANCED
        // ([`find_balanced_close_tag`]); raw-text blocks end at the FIRST
        // same-name close (HTML raw-text semantics — a `</script>` in a JS
        // string ends the block, and a stray `<script>` in a string must not
        // open a nesting level). WHICH blocks are raw text is the carrier
        // decision [`CustomBlockContentKind`] encodes: under `RawText` (Vue)
        // the shared parser predicate [`vue_sfc_root_block_is_raw_text`] makes
        // everything but `<template>` raw text — a Vue custom block like
        // `<docs>` must end at its first close or it swallows the
        // `<script setup>` after it; under `Markup` (Svelte) only
        // `<script>` / `<style>` are raw text and components balance.
        // Unbalanced or torn-opaque content fails closed to the historical
        // first-close boundary so the blocks after it stay discoverable. The
        // pattern carries the canonical lowercase tag name; both searches match
        // the close case-insensitively, so a `</SCRIPT>` still resolves.
        let close_pattern = format!("</{tag_name}");
        let raw_text = match custom_blocks {
            CustomBlockContentKind::RawText => vue_sfc_root_block_is_raw_text(tag_name.as_bytes()),
            CustomBlockContentKind::Markup => is_raw_text_sfc_tag(&tag_name),
        };
        let close_start = if raw_text {
            find_close_tag(source, i, &close_pattern)
        } else {
            match find_balanced_close_tag(source, i, &tag_name) {
                BalancedClose::Found(pos) => Some(pos),
                BalancedClose::UnterminatedOpaque | BalancedClose::Unbalanced => {
                    find_close_tag(source, i, &close_pattern)
                }
            }
        };
        match close_start {
            Some(close_start) => {
                let close_end = match source[close_start..].find('>') {
                    Some(offset) => close_start + offset + 1,
                    None => continue,
                };

                blocks.push(SfcBlock {
                    tag_name,
                    open_tag_start: tag_start as u32,
                    open_tag_end,
                    close_tag_start: close_start as u32,
                    close_tag_end: close_end as u32,
                    attrs_raw,
                });

                i = close_end;
            }
            None => {
                // No closing tag yet (the user is mid-typing the block). The
                // block must STILL establish a region from its open tag to EOF
                // so its content classifies as inside-block (non-markup), not
                // RootLevel — otherwise a generic `Box<Foo>` typed before the
                // closing tag exists is misread as root markup. The close-tag
                // positions collapse to the source end (an empty, open-ended
                // close span). Everything after the open tag is inside this
                // block, so scanning stops.
                blocks.push(SfcBlock {
                    tag_name,
                    open_tag_start: tag_start as u32,
                    open_tag_end,
                    close_tag_start: len as u32,
                    close_tag_end: len as u32,
                    attrs_raw,
                });
                break;
            }
        }
    }

    blocks
}

/// Check if a tag name is an SFC top-level block.
///
/// The built-in `script` / `template` / `style` tags are matched
/// CASE-INSENSITIVELY (HTML tag names are case-insensitive, and
/// [`find_close_tag`] already matches the closing tag with
/// `eq_ignore_ascii_case`), so `<SCRIPT>` / `<Style>` classify as their
/// canonical block, not as a custom block.
fn is_sfc_tag(name: &str) -> bool {
    is_builtin_sfc_tag(name) || is_custom_block_tag(name)
}

/// Whether `name` is one of the built-in SFC block tags (`script` / `template`
/// / `style`), case-insensitively. Returns the canonical lowercase form so the
/// stored [`SfcBlock::tag_name`] is normalized and downstream lowercase matches
/// (e.g. the auto-close markup-region gate) recognize a case-variant tag.
fn canonical_builtin_sfc_tag(name: &str) -> Option<&'static str> {
    ["script", "template", "style"]
        .into_iter()
        .find(|canonical| name.eq_ignore_ascii_case(canonical))
}

fn is_builtin_sfc_tag(name: &str) -> bool {
    canonical_builtin_sfc_tag(name).is_some()
}

/// Check if a tag name could be a custom block.
/// Custom blocks are any tag name that isn't a standard HTML element
/// and isn't script/template/style.
fn is_custom_block_tag(name: &str) -> bool {
    // Custom blocks typically have short, known names like "i18n", "docs", etc.
    // We accept any tag name that:
    // 1. Isn't a standard HTML tag
    // 2. Contains only lowercase letters, digits, hyphens
    // This is a heuristic — the real SFC parser is the source of truth.
    !is_standard_html_tag(name)
}

/// Whether `name` is a standard HTML element, case-insensitively (HTML tag
/// names are case-insensitive). The match table is lowercase, so the input is
/// folded to lowercase first — `<DIV>` / `<Br>` are still standard elements and
/// never misclassified as custom SFC blocks.
fn is_standard_html_tag(name: &str) -> bool {
    is_standard_html_tag_lower(&name.to_ascii_lowercase())
}

fn is_standard_html_tag_lower(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "head"
            | "body"
            | "div"
            | "span"
            | "p"
            | "a"
            | "img"
            | "input"
            | "button"
            | "form"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "ul"
            | "ol"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "footer"
            | "nav"
            | "main"
            | "section"
            | "article"
            | "aside"
            | "details"
            | "summary"
            | "dialog"
            | "data"
            | "time"
            | "code"
            | "pre"
            | "blockquote"
            | "hr"
            | "br"
            | "em"
            | "strong"
            | "small"
            | "sub"
            | "sup"
            | "mark"
            | "del"
            | "ins"
            | "abbr"
            | "cite"
            | "dfn"
            | "kbd"
            | "samp"
            | "var"
            | "meta"
            | "link"
            | "base"
            | "title"
            | "noscript"
            | "canvas"
            | "svg"
            | "video"
            | "audio"
            | "source"
            | "track"
            | "embed"
            | "object"
            | "param"
            | "iframe"
            | "map"
            | "area"
            | "select"
            | "textarea"
            | "label"
            | "fieldset"
            | "legend"
            | "output"
            | "progress"
            | "meter"
            | "datalist"
            | "option"
            | "optgroup"
            | "picture"
            | "figure"
            | "figcaption"
            | "colgroup"
            | "col"
            | "thead"
            | "tbody"
            | "tfoot"
            | "caption"
            | "slot"
    )
}

/// The blocks that are raw text under EVERY [`CustomBlockContentKind`]:
/// `<script>` / `<style>` content is never parsed as markup, so the FIRST
/// matching close tag ends the block (HTML raw-text element semantics, which
/// is how both Vue's SFC parser and Svelte treat them). This is the
/// [`Markup`](CustomBlockContentKind::Markup)-kind raw-text set; the
/// [`RawText`](CustomBlockContentKind::RawText) (Vue) kind instead derives its
/// per-tag decision from the shared parser predicate
/// [`vue_sfc_root_block_is_raw_text`], under which custom blocks are raw text
/// too and only `<template>` balances.
fn is_raw_text_sfc_tag(name: &str) -> bool {
    matches!(name, "script" | "style")
}

/// Outcome of a depth-balanced close-tag search ([`find_balanced_close_tag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BalancedClose {
    /// Byte offset of the `<` of the depth-matched closing tag.
    Found(usize),
    /// An opaque region (`<!--` comment or `<![CDATA[` section) opened after
    /// the content start and never closes; everything after it reads as
    /// opaque character data, so no close judgement is possible.
    UnterminatedOpaque,
    /// Tag depth never returned to zero before EOF (missing outer close, or a
    /// stray same-name open inside raw text).
    Unbalanced,
}

/// Find the depth-matched `</{tag_name}>` for a block whose content starts at
/// `content_start` (just past the opening tag's `>`), balancing nested
/// same-name opens so a nested `<template #slot>` / `<Card>` does not truncate
/// the enclosing block.
///
/// The walk skips `<!-- … -->` comments and `<![CDATA[ … ]]>` sections
/// wholesale (same-name tokens inside them never shift depth; CDATA is
/// treated as opaque wherever it appears — the scanner is not a tree parser,
/// so it does not model the foreign-content-only rule under which HTML grants
/// CDATA its meaning), scans every tag-like span — OPENING (`<name …>`) and
/// CLOSING (`</name …>`) alike — to its `>` honoring quoted attribute values
/// (a `>` or a same-name tag inside a quote never leaks into the walk, even
/// inside a malformed attribute-bearing close tag like
/// `</template data-x="<template>">`), and treats a malformed tag-like `<`
/// (no `>` before EOF or a raw `<`) as plain text. Quotes only matter INSIDE
/// tag spans — an apostrophe in text (`Bob's`) never desyncs the walk.
/// Tag-name matching is case-insensitive with proper name boundaries,
/// consistent with [`find_close_tag`]. Self-closing same-name opens
/// (`<template #row />`) do not open a nesting level.
pub(crate) fn find_balanced_close_tag(
    source: &str,
    content_start: usize,
    tag_name: &str,
) -> BalancedClose {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let tag = tag_name.as_bytes();
    let mut depth = 1usize;
    let mut i = content_start;

    while i < len {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // Comments never contribute tags.
        if i + 4 <= len && &bytes[i..i + 4] == b"<!--" {
            match find_bytes(bytes, i + 4, b"-->") {
                Some(end) => {
                    i = end + 3;
                    continue;
                }
                None => return BalancedClose::UnterminatedOpaque,
            }
        }
        // CDATA sections are opaque character data: a same-name open or close
        // inside `<![CDATA[ … ]]>` never shifts depth.
        if i + 9 <= len && &bytes[i..i + 9] == b"<![CDATA[" {
            match find_bytes(bytes, i + 9, b"]]>") {
                Some(end) => {
                    i = end + 3;
                    continue;
                }
                None => return BalancedClose::UnterminatedOpaque,
            }
        }
        // `</name …>` — a close tag. Only a same-name close changes depth.
        // The span is scanned with the SAME quote-awareness as open tags so a
        // quoted attribute value on a (malformed, mid-edit) close tag —
        // `</template data-x="<template>">` — never leaks an open into the
        // walk and desyncs the depth.
        if i + 1 < len && bytes[i + 1] == b'/' {
            if close_tag_name_matches(bytes, i, tag) {
                depth -= 1;
                if depth == 0 {
                    return BalancedClose::Found(i);
                }
            }
            match scan_tag_span(bytes, i) {
                Some((gt, _)) => i = gt + 1,
                // Malformed (no `>` before EOF or a raw `<`): treat the `</`
                // as plain text and keep walking.
                None => i += 2,
            }
            continue;
        }
        // `<name …>` — an open tag (any name). Scan its span so quoted
        // attribute values are honored; only a same-name, non-self-closing
        // open increases depth.
        if i + 1 < len && bytes[i + 1].is_ascii_alphanumeric() {
            match scan_tag_span(bytes, i) {
                Some((gt, self_closing)) => {
                    if !self_closing && open_tag_name_matches(bytes, i, tag) {
                        depth += 1;
                    }
                    i = gt + 1;
                }
                // Malformed (no `>` before EOF or a raw `<`): treat this `<`
                // as plain text and keep walking.
                None => i += 1,
            }
            continue;
        }
        i += 1;
    }
    BalancedClose::Unbalanced
}

/// Whether `bytes[at..]` is `</{tag}` (case-insensitive) with a proper name
/// boundary (the byte after the name is `>` or whitespace), consistent with
/// [`find_close_tag`].
fn close_tag_name_matches(bytes: &[u8], at: usize, tag: &[u8]) -> bool {
    let name_at = at + 2;
    if name_at + tag.len() >= bytes.len() {
        return false;
    }
    if !bytes[name_at..name_at + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    let after = bytes[name_at + tag.len()];
    after == b'>' || after.is_ascii_whitespace()
}

/// Whether `bytes[at..]` is `<{tag}` (case-insensitive) with a proper name
/// boundary (whitespace, `>`, or `/`), so `<templatefoo` never matches.
fn open_tag_name_matches(bytes: &[u8], at: usize, tag: &[u8]) -> bool {
    let name_at = at + 1;
    if name_at + tag.len() > bytes.len() {
        return false;
    }
    if !bytes[name_at..name_at + tag.len()].eq_ignore_ascii_case(tag) {
        return false;
    }
    match bytes.get(name_at + tag.len()) {
        None => true,
        Some(&b) => b.is_ascii_whitespace() || b == b'>' || b == b'/',
    }
}

/// Scan an open tag's span from its `<` at `lt` to its closing `>`, honoring
/// quoted attribute values. Returns `(gt_index, self_closing)`; `None` when
/// the tag never closes (EOF inside the tag or its attribute value) or a raw
/// `<` appears first (malformed — the caller treats the original `<` as
/// text).
fn scan_tag_span(bytes: &[u8], lt: usize) -> Option<(usize, bool)> {
    let len = bytes.len();
    let mut j = lt + 1;
    while j < len {
        match bytes[j] {
            b'>' => return Some((j, bytes[j - 1] == b'/')),
            b'"' | b'\'' => {
                let quote = bytes[j];
                j += 1;
                while j < len && bytes[j] != quote {
                    j += 1;
                }
                if j >= len {
                    return None; // unterminated attribute value
                }
            }
            b'<' => return None,
            _ => {}
        }
        j += 1;
    }
    None
}

/// Find the position of a closing tag pattern (case-insensitive for the tag name).
fn find_close_tag(source: &str, start: usize, pattern: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let pat_bytes = pattern.as_bytes();
    let pat_len = pat_bytes.len();
    let mut i = start;

    while i + pat_len <= bytes.len() {
        if bytes[i] == b'<'
            && bytes.get(i + 1) == Some(&b'/')
            && source[i..i + pat_len].eq_ignore_ascii_case(pattern)
        {
            // Verify the next char after tag name is '>' or whitespace
            let after = i + pat_len;
            if after < bytes.len() && (bytes[after] == b'>' || bytes[after].is_ascii_whitespace()) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Find a byte pattern in `bytes` starting at `start`.
fn find_bytes(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    let pat_len = pattern.len();
    if pat_len == 0 || start + pat_len > bytes.len() {
        return None;
    }
    (start..=bytes.len() - pat_len).find(|&i| &bytes[i..i + pat_len] == pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Basic scanning
    // ========================================================================

    #[test]
    fn test_basic_sfc() {
        let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup lang=\"ts\">\nconst x = 1;\n</script>\n\n<style scoped>\n.foo { color: red }\n</style>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(blocks[1].tag_name, "script");
        assert_eq!(blocks[2].tag_name, "style");
    }

    #[test]
    fn test_script_setup_detection() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_setup());
        assert_eq!(blocks[0].lang(), Some("ts"));
    }

    #[test]
    fn is_setup_requires_exact_setup_attribute_not_substring() {
        // `notsetup` and `data-setup` both CONTAIN the substring "setup" but are NOT the boolean
        // `setup` attribute. Classification must be by exact attribute name, never substring.
        for attrs in ["notsetup", "data-setup", "setupx", "data-setup=\"false\""] {
            let source = format!("<script {attrs} lang=\"ts\">\nconst x = 1;\n</script>");
            let blocks = scan_sfc_blocks(&source);
            assert!(
                !blocks[0].is_setup(),
                "`{attrs}` must NOT be treated as <script setup>"
            );
        }
        // A real boolean `setup` attribute IS setup, in any position.
        for attrs in ["setup", "lang=\"ts\" setup", "setup lang=\"ts\""] {
            let source = format!("<script {attrs}>\nconst x = 1;\n</script>");
            let blocks = scan_sfc_blocks(&source);
            assert!(blocks[0].is_setup(), "`{attrs}` is a real <script setup>");
        }
    }

    #[test]
    fn lang_reads_exact_attribute_name_not_substring() {
        // `mylang` CONTAINS "lang" but is not the `lang` attribute.
        let source = "<script setup mylang=\"x\">\n</script>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(
            blocks[0].lang(),
            None,
            "`mylang` must not satisfy a `lang` query"
        );

        // The real `lang` attribute resolves to its typed value.
        let source = "<script setup lang=\"tsx\">\n</script>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].lang(), Some("tsx"));
    }

    #[test]
    fn scoped_and_module_require_exact_attribute_names() {
        // `data-scoped` / `unscoped` / `modulexyz` contain the substrings but are not the attrs.
        let source = "<style data-scoped modulexyz>\n.a{}\n</style>";
        let blocks = scan_sfc_blocks(source);
        assert!(!blocks[0].is_scoped(), "`data-scoped` is not `scoped`");
        assert!(!blocks[0].is_module(), "`modulexyz` is not `module`");

        let source = "<style scoped module>\n.a{}\n</style>";
        let blocks = scan_sfc_blocks(source);
        assert!(blocks[0].is_scoped());
        assert!(blocks[0].is_module());
    }

    #[test]
    fn test_style_attributes() {
        let source = "<style scoped lang=\"scss\" module>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_scoped());
        assert!(blocks[0].is_module());
        assert_eq!(blocks[0].lang(), Some("scss"));
    }

    #[test]
    fn test_custom_block() {
        let source = "<template>\n  <div/>\n</template>\n\n<i18n lang=\"json\">\n{\"en\": {\"hello\": \"Hello\"}}\n</i18n>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(blocks[1].tag_name, "i18n");
        assert_eq!(blocks[1].lang(), Some("json"));
    }

    #[test]
    fn test_multiple_style_blocks() {
        let source = "<style>\n.a {}\n</style>\n<style scoped>\n.b {}\n</style>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "style");
        assert!(!blocks[0].is_scoped());
        assert_eq!(blocks[1].tag_name, "style");
        assert!(blocks[1].is_scoped());
    }

    // ========================================================================
    // Content ranges
    // ========================================================================

    #[test]
    fn test_content_range() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let (start, end) = blocks[0].content_range();
        assert_eq!(&source[start as usize..end as usize], "\nconst x = 1;\n");
    }

    #[test]
    fn test_open_close_tag_positions() {
        let source = "<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(
            &source[b.open_tag_start as usize..b.open_tag_end as usize],
            "<template>"
        );
        assert_eq!(
            &source[b.close_tag_start as usize..b.close_tag_end as usize],
            "</template>"
        );
    }

    // ========================================================================
    // Edge cases
    // ========================================================================

    #[test]
    fn test_empty_source() {
        let blocks = scan_sfc_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_no_blocks() {
        let blocks = scan_sfc_blocks("just plain text\nno blocks here");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_html_comment_ignored() {
        let source = "<!-- this is a comment -->\n<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag_name, "template");
    }

    #[test]
    fn test_self_closing_ignored() {
        let source = "<template />\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Self-closing <template /> is skipped
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag_name, "script");
    }

    #[test]
    fn test_script_with_companion() {
        // Two script blocks: one <script> and one <script setup>
        let source =
            "<script>\nexport default { name: 'Foo' }\n</script>\n\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "script");
        assert!(!blocks[0].is_setup());
        assert_eq!(blocks[1].tag_name, "script");
        assert!(blocks[1].is_setup());
    }

    // ========================================================================
    // Real-world SFC
    // ========================================================================

    #[test]
    fn test_realistic_sfc() {
        let source = r#"<template>
  <div class="container">
    <h1>{{ title }}</h1>
    <p v-if="show">Content</p>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const title = ref('Hello')
const show = ref(true)
</script>

<style scoped>
.container {
  padding: 16px;
}
</style>
"#;
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 3);

        // Template
        assert_eq!(blocks[0].tag_name, "template");
        let (cs, ce) = blocks[0].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains("<div class=\"container\">"));
        assert!(content.contains("{{ title }}"));

        // Script
        assert_eq!(blocks[1].tag_name, "script");
        assert!(blocks[1].is_setup());
        assert_eq!(blocks[1].lang(), Some("ts"));
        let (cs, ce) = blocks[1].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains("import { ref } from 'vue'"));

        // Style
        assert_eq!(blocks[2].tag_name, "style");
        assert!(blocks[2].is_scoped());
        let (cs, ce) = blocks[2].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains(".container"));
    }

    // ========================================================================
    // attrs() helper (B8)
    // ========================================================================

    #[test]
    fn test_attrs_short_form() {
        let source = r#"<script setup attrs="{ class?: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ class?: string }"));
    }

    #[test]
    fn test_attrs_long_form() {
        let source = r#"<script setup attributes="{ class?: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ class?: string }"));
    }

    #[test]
    fn test_attrs_not_present() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), None);
    }

    #[test]
    fn test_attrs_long_form_preferred_over_short() {
        // When `attributes` is present, it should win even if there's also `attrs`
        // In practice only one would be used, but test the precedence.
        let source = r#"<script setup attributes="{ a: number }" attrs="{ b: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ a: number }"));
    }

    // ========================================================================
    // Cursor context classification (A1)
    // ========================================================================

    #[test]
    fn test_classify_cursor_in_opening_tag() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Cursor on '<' of opening tag
        assert_eq!(
            classify_cursor(0, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
        // Cursor on 's' of 'setup'
        assert_eq!(
            classify_cursor(8, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_in_content() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Cursor in content area
        let (cs, _) = blocks[0].content_range();
        assert_eq!(
            classify_cursor(cs + 1, &blocks),
            SfcCursorContext::BlockContent { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_in_closing_tag() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(
            classify_cursor(blocks[0].close_tag_start, &blocks),
            SfcCursorContext::ClosingTag { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_root_level() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Between blocks (the newlines between </template> and <script>)
        let between_offset = blocks[0].close_tag_end;
        assert_eq!(
            classify_cursor(between_offset, &blocks),
            SfcCursorContext::RootLevel
        );
    }

    #[test]
    fn test_classify_cursor_empty_file() {
        assert_eq!(classify_cursor(0, &[]), SfcCursorContext::RootLevel);
    }

    #[test]
    fn test_classify_cursor_multi_block() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup lang=\"ts\">\nconst x = 1;\n</script>\n\n<style scoped>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks.len(), 3);

        // Template opening tag
        assert_eq!(
            classify_cursor(0, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
        // Script opening tag
        assert_eq!(
            classify_cursor(blocks[1].open_tag_start, &blocks),
            SfcCursorContext::OpeningTag { block_index: 1 }
        );
        // Style content
        let (style_cs, _) = blocks[2].content_range();
        assert_eq!(
            classify_cursor(style_cs + 1, &blocks),
            SfcCursorContext::BlockContent { block_index: 2 }
        );
    }

    // ========================================================================
    // Opening tag parser (A1)
    // ========================================================================

    #[test]
    fn test_parse_opening_tag_basic() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.tag_name, "script");
        assert_eq!(ctx.attrs.len(), 2);

        // setup is boolean
        assert_eq!(ctx.attrs[0].name, "setup");
        assert_eq!(ctx.attrs[0].value, None);

        // lang has value
        assert_eq!(ctx.attrs[1].name, "lang");
        assert_eq!(ctx.attrs[1].value, Some("ts".to_string()));
    }

    #[test]
    fn test_parse_opening_tag_no_attrs() {
        let source = "<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.tag_name, "template");
        assert!(ctx.attrs.is_empty());
    }

    #[test]
    fn test_parse_opening_tag_attrs_value() {
        let source = r#"<script setup attrs="{ class?: string }" lang="ts">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.attrs.len(), 3);
        assert_eq!(ctx.attrs[0].name, "setup");
        assert_eq!(ctx.attrs[1].name, "attrs");
        assert_eq!(ctx.attrs[1].value, Some("{ class?: string }".to_string()));
        assert_eq!(ctx.attrs[2].name, "lang");
        assert_eq!(ctx.attrs[2].value, Some("ts".to_string()));

        // Verify value offsets point to correct source positions
        let vs = ctx.attrs[1].value_start.unwrap() as usize;
        let ve = ctx.attrs[1].value_end.unwrap() as usize;
        assert_eq!(&source[vs..ve], "{ class?: string }");
    }

    #[test]
    fn test_parse_opening_tag_single_quotes() {
        let source = "<style lang='scss' scoped>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.attrs.len(), 2);
        assert_eq!(ctx.attrs[0].name, "lang");
        assert_eq!(ctx.attrs[0].value, Some("scss".to_string()));
        assert_eq!(ctx.attrs[1].name, "scoped");
        assert_eq!(ctx.attrs[1].value, None);
    }

    // ========================================================================
    // Case-insensitive SFC tag classification (F4)
    //
    // HTML tag names are case-insensitive, so `<SCRIPT>` / `<Script>` /
    // `<STYLE>` / `<Template>` are the SAME block tags as their lowercase
    // forms. `find_close_tag` already matches the closing tag with
    // `eq_ignore_ascii_case`, so the OPEN-tag classifier must agree, otherwise
    // a case-variant `<SCRIPT>` produces no block and its content (e.g. a TS
    // generic `Box<Foo>`) leaks into RootLevel markup classification.
    // ========================================================================

    #[test]
    fn case_variant_script_tag_is_recognized_as_script_block() {
        // `<SCRIPT>` (uppercase) must classify as a `script` block with a
        // NORMALIZED lowercase `tag_name`, so downstream lowercase matches
        // (the Svelte markup-region gate's `matches!(tag, "script" | "style")`)
        // still recognize it. Pre-fix, a lowercase-only matcher mis-routes
        // `<SCRIPT>` into a CUSTOM block whose `tag_name` stays `"SCRIPT"`.
        let source = "<SCRIPT lang=\"ts\">\nconst x: Box<Foo> = mk();\n</SCRIPT>\n<div></div>";
        let blocks = scan_sfc_blocks(source);
        let script = blocks
            .iter()
            .find(|b| b.tag_name.eq_ignore_ascii_case("script"))
            .expect("<SCRIPT> must be recognized as a script block");
        assert_eq!(
            script.tag_name, "script",
            "a case-variant <SCRIPT> must normalize its tag_name to lowercase `script`, got `{}`",
            script.tag_name
        );
        let (cs, ce) = script.content_range();
        assert!(
            (cs as usize..ce as usize).contains(&(source.find("Box<Foo>").unwrap())),
            "the script block's content must enclose the `Box<Foo>` generic",
        );
    }

    #[test]
    fn case_variant_style_and_template_tags_are_recognized() {
        let source = "<Template>\n<div></div>\n</Template>\n<STYLE>\n.a{}\n</STYLE>";
        let blocks = scan_sfc_blocks(source);
        let template = blocks
            .iter()
            .find(|b| b.tag_name.eq_ignore_ascii_case("template"))
            .expect("<Template> must be recognized as a template block");
        assert_eq!(
            template.tag_name, "template",
            "<Template> must normalize its tag_name to lowercase `template`"
        );
        let style = blocks
            .iter()
            .find(|b| b.tag_name.eq_ignore_ascii_case("style"))
            .expect("<STYLE> must be recognized as a style block");
        assert_eq!(
            style.tag_name, "style",
            "<STYLE> must normalize its tag_name to lowercase `style`"
        );
    }

    // ========================================================================
    // Unclosed SFC block establishes an open-ended non-markup region (F5)
    //
    // While the user is mid-typing a `<script>` / `<style>` block there is no
    // closing tag yet. The block must STILL span from its open tag to EOF so
    // its content classifies as inside-block (non-markup) content rather than
    // RootLevel — otherwise a generic `Box<Foo>` typed before `</script>`
    // exists is misread as root markup and auto-closed.
    // ========================================================================

    #[test]
    fn unclosed_script_block_spans_to_eof() {
        let source = "<script lang=\"ts\">\nconst x: Box<Foo> = mk();";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(
            blocks.len(),
            1,
            "the unclosed <script> must still be a block"
        );
        let b = &blocks[0];
        assert_eq!(b.tag_name, "script");
        // The block spans to EOF: close tag positions sit at the source end.
        assert_eq!(b.close_tag_start as usize, source.len());
        assert_eq!(b.close_tag_end as usize, source.len());
        // The generic is inside the block's content, so the cursor right after
        // `Box<Foo` classifies as BlockContent, NOT RootLevel.
        let off = source.find("Box<Foo").unwrap() as u32 + "Box<Foo".len() as u32;
        assert_eq!(
            classify_cursor(off, &blocks),
            SfcCursorContext::BlockContent { block_index: 0 },
            "an offset inside the unclosed script must classify as BlockContent"
        );
    }

    #[test]
    fn unclosed_style_block_spans_to_eof() {
        let source = "<div></div>\n<style>\n.a > .b { color: red";
        let blocks = scan_sfc_blocks(source);
        let style = blocks
            .iter()
            .position(|b| b.tag_name == "style")
            .expect("unclosed <style> must still be a block");
        assert_eq!(
            blocks[style].close_tag_end as usize,
            source.len(),
            "the unclosed style block must span to EOF"
        );
        let off = source.find(".a >").unwrap() as u32 + ".a >".len() as u32;
        assert_eq!(
            classify_cursor(off, &blocks),
            SfcCursorContext::BlockContent { block_index: style },
            "an offset inside the unclosed style must classify as BlockContent"
        );
    }

    // ========================================================================
    // Nested same-name tags — depth-balanced close matching (W20)
    //
    // `<template>` and custom/component blocks contain real markup where
    // same-name tags nest (a Vue slot `<template #row>`, a nested Svelte
    // component). The block's close must be the DEPTH-MATCHED close tag, not
    // the first one, or everything after the inner close falls out of the
    // block (folding, symbols, cursor classification all die there).
    // ========================================================================

    #[test]
    fn nested_slot_template_does_not_truncate_the_outer_template_block() {
        let source = "<template>\n  <Card>\n    <template #header>\n      <h1>title</h1>\n    </template>\n    <button>after</button>\n  </Card>\n</template>\n\n<script setup lang=\"ts\">\nconst x = 1;\n</script>\n\n<style scoped>\n.a {}\n</style>\n";
        let blocks = scan_sfc_blocks(source);

        // The nested slot template is NOT a top-level block; script/style after
        // the outer close are still discovered.
        assert_eq!(
            blocks.len(),
            3,
            "expected exactly template + script + style, got: {:?}",
            blocks.iter().map(|b| &b.tag_name).collect::<Vec<_>>()
        );
        assert_eq!(blocks[0].tag_name, "template");
        let outer_close = source.rfind("</template>").unwrap();
        assert_eq!(
            blocks[0].close_tag_start as usize, outer_close,
            "the outer template block must close at the OUTER </template>, not the nested slot template's close"
        );
        let (cs, ce) = blocks[0].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(
            content.contains("<button>after</button>"),
            "markup after the nested </template> must stay inside the outer block's content"
        );
        // A cursor on markup after the inner close classifies as template
        // content, not RootLevel dead zone.
        let after_off = source.find("after<").unwrap() as u32;
        assert_eq!(
            classify_cursor(after_off, &blocks),
            SfcCursorContext::BlockContent { block_index: 0 }
        );
        assert_eq!(blocks[1].tag_name, "script");
        assert!(blocks[1].is_setup());
        assert_eq!(blocks[2].tag_name, "style");
    }

    #[test]
    fn nested_same_name_component_does_not_truncate_the_outer_block_svelte() {
        // Svelte carrier sources reach the scanner through the carrier-resolved
        // routing (`scan_sfc_blocks_for_document` → `custom_block_content_kind`),
        // and Svelte markup nests same-name components at the root. This pins
        // the PRODUCTION composition: the svelte editor language id resolves to
        // `Markup`, under which the original W20 balance fix holds.
        let source = "<script lang=\"ts\">\n  let n = 1;\n</script>\n\n<Card>\n  <Card>inner</Card>\n  <p>after</p>\n</Card>\n\n<style>\n  .a {}\n</style>\n";
        let kind = custom_block_content_kind(Some("svelte"), "/proj/src/App.svelte");
        assert_eq!(kind, CustomBlockContentKind::Markup);
        let blocks = scan_sfc_blocks_with(source, kind);

        let card = blocks
            .iter()
            .find(|b| b.tag_name == "Card")
            .expect("the outer <Card> must be scanned as a block");
        let outer_close = source.rfind("</Card>").unwrap();
        assert_eq!(
            card.close_tag_start as usize, outer_close,
            "the outer <Card> block must close at the OUTER </Card>, not the nested component's close"
        );
        let (cs, ce) = card.content_range();
        assert!(
            source[cs as usize..ce as usize].contains("<p>after</p>"),
            "markup after the nested </Card> must stay inside the outer block's content"
        );
        assert!(blocks.iter().any(|b| b.tag_name == "script"));
        assert!(
            blocks.iter().any(|b| b.tag_name == "style"),
            "the style block after the outer close must still be discovered"
        );
    }

    #[test]
    fn custom_block_content_kind_maps_svelte_to_markup_everything_else_raw_text() {
        // Editor language id is authoritative when it names a carrier
        // (mirrors `DocumentRegistry::document_file_language`)…
        assert_eq!(
            custom_block_content_kind(Some("svelte"), "/proj/src/App.svelte"),
            CustomBlockContentKind::Markup
        );
        assert_eq!(
            custom_block_content_kind(Some("vue"), "/proj/src/App.vue"),
            CustomBlockContentKind::RawText
        );
        // …a non-carrier editor id falls back to the canonical path…
        assert_eq!(
            custom_block_content_kind(Some("plaintext"), "/proj/src/App.svelte"),
            CustomBlockContentKind::Markup
        );
        assert_eq!(
            custom_block_content_kind(None, "/proj/src/App.svelte"),
            CustomBlockContentKind::Markup
        );
        // …and everything that is not Svelte gets the fail-conservative Vue
        // raw-text rule (discovery-safe: never swallows a following block).
        assert_eq!(
            custom_block_content_kind(None, "/proj/src/App.vue"),
            CustomBlockContentKind::RawText
        );
        assert_eq!(
            custom_block_content_kind(Some("typescript"), "/proj/src/main.ts"),
            CustomBlockContentKind::RawText
        );
    }

    #[test]
    fn self_closing_same_name_tag_does_not_open_a_nesting_level() {
        // `<template #row />` is self-closing: it must NOT increment depth.
        // If it did, the walk would end unbalanced and fail back to the
        // first-close boundary (the #cell close), truncating the outer block.
        let source = "<template>\n  <template #row />\n  <template #cell>x</template>\n  <div>after</div>\n</template>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let outer_close = source.rfind("</template>").unwrap();
        assert_eq!(blocks[0].close_tag_start as usize, outer_close);
        let (cs, ce) = blocks[0].content_range();
        assert!(source[cs as usize..ce as usize].contains("<div>after</div>"));
    }

    #[test]
    fn comment_embedded_same_name_tags_do_not_shift_the_block_boundary() {
        // A commented-out `</template>` must not close the block, and a
        // commented-out `<template>` must not open a nesting level.
        let source = "<template>\n  <!-- </template> -->\n  <!-- <template> -->\n  <template #a>x</template>\n  <p>after</p>\n</template>\n<script setup>\nconst y = 2;\n</script>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(
            blocks.len(),
            2,
            "expected template + script, got: {:?}",
            blocks.iter().map(|b| &b.tag_name).collect::<Vec<_>>()
        );
        assert_eq!(blocks[0].tag_name, "template");
        let outer_close = source.rfind("</template>").unwrap();
        assert_eq!(
            blocks[0].close_tag_start as usize, outer_close,
            "comment-embedded tags must not shift the close boundary"
        );
        let (cs, ce) = blocks[0].content_range();
        assert!(source[cs as usize..ce as usize].contains("<p>after</p>"));
        assert_eq!(blocks[1].tag_name, "script");
    }

    #[test]
    fn quoted_attr_value_close_tag_text_does_not_close_the_block() {
        // `</template>` inside a quoted attribute value of a nested element is
        // data, not a close tag.
        let source =
            "<template>\n  <div data-x=\"</template>\">\n    <p>after</p>\n  </div>\n</template>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let outer_close = source.rfind("</template>").unwrap();
        assert_eq!(
            blocks[0].close_tag_start as usize, outer_close,
            "a quoted `</template>` attribute value must not close the block"
        );
        let (cs, ce) = blocks[0].content_range();
        assert!(source[cs as usize..ce as usize].contains("<p>after</p>"));
    }

    #[test]
    fn unbalanced_nested_open_falls_back_to_first_close_without_panic() {
        // The outer close is missing, so depth never balances. Fail closed to
        // the historical first-close boundary so the blocks after it stay
        // discoverable (extending to EOF would swallow the script block).
        let source = "<template>\n  <template #a>x</template>\n  <p>stranded</p>\n<script setup>\nconst z = 3;\n</script>\n";
        let blocks = scan_sfc_blocks(source);

        let first_close = source.find("</template>").unwrap();
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(
            blocks[0].close_tag_start as usize, first_close,
            "unbalanced input must fail closed to the first-close boundary"
        );
        assert!(
            blocks
                .iter()
                .any(|b| b.tag_name == "script" && b.is_setup()),
            "the script block after the unbalanced template must still be discovered"
        );
    }

    #[test]
    fn vue_custom_block_is_raw_text_so_a_nested_same_name_pair_never_swallows_the_script() {
        // Vue custom blocks are RCDATA (raw text) in Vue's own parser and in
        // `verter_parser` (`check_and_setup_rcdata`): the FIRST `</docs>` ends
        // the block, so the `<script setup>` that follows it is a real root
        // block. Balancing `<docs>` instead would span to the SECOND `</docs>`
        // and swallow the script — hiding it from imports, code lens, document
        // symbols, and edits.
        let source = "<docs><docs>example</docs><script setup>\nconst x = 1;\n</script></docs>";
        let blocks = scan_sfc_blocks(source);

        let script = blocks
            .iter()
            .find(|b| b.tag_name == "script")
            .expect("the <script setup> after the first </docs> must be discovered");
        assert!(script.is_setup());
        let (cs, ce) = script.content_range();
        assert_eq!(
            &source[cs as usize..ce as usize],
            "\nconst x = 1;\n",
            "the script block must carry its own content, not sit inside <docs>"
        );

        let docs = blocks
            .iter()
            .find(|b| b.tag_name == "docs")
            .expect("<docs> must still be a block");
        assert_eq!(
            docs.close_tag_start as usize,
            source.find("</docs>").unwrap(),
            "the raw-text <docs> block must end at the FIRST </docs>, matching verter_parser"
        );
        assert_eq!(
            blocks.len(),
            2,
            "expected exactly docs + script, got: {:?}",
            blocks.iter().map(|b| &b.tag_name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn svelte_markup_kind_still_balances_nested_same_name_components() {
        // The Svelte routing (CustomBlockContentKind::Markup) must keep the
        // depth-balanced close for root components — the original W20 bug.
        let source =
            "<Card>\n  <Card>inner</Card>\n  <p>after</p>\n</Card>\n<style>\n.a {}\n</style>\n";
        let blocks = scan_sfc_blocks_with(source, CustomBlockContentKind::Markup);

        let card = blocks
            .iter()
            .find(|b| b.tag_name == "Card")
            .expect("the outer <Card> must be scanned as a block");
        assert_eq!(
            card.close_tag_start as usize,
            source.rfind("</Card>").unwrap(),
            "under Markup custom blocks the outer <Card> must close at the OUTER </Card>"
        );
        let (cs, ce) = card.content_range();
        assert!(source[cs as usize..ce as usize].contains("<p>after</p>"));
        assert!(
            blocks.iter().any(|b| b.tag_name == "style"),
            "the style block after the outer close must still be discovered"
        );
    }

    #[test]
    fn close_tag_attr_quoted_same_name_open_does_not_unbalance_the_walk() {
        // A close tag may carry (malformed, mid-edit) attributes. The walker
        // must scan the CLOSE tag's span with the same quote-awareness as open
        // tags: the quoted `<template>` inside `</template data-x="...">` is
        // data, not an open — otherwise depth never rebalances, the scanner
        // falls back to the truncating first-close boundary, and the
        // auto-close markup window extends to EOF.
        let source = "<template>\n  <template #a>x</template data-x=\"<template>\">\n  <p>after</p>\n</template>\n<script setup lang=\"ts\">\nconst b: Box<Foo> = mk();\n</script>\n";

        let content_start = source.find('>').unwrap() + 1;
        let outer_close = source.rfind("</template>").unwrap();
        assert_eq!(
            find_balanced_close_tag(source, content_start, "template"),
            BalancedClose::Found(outer_close),
            "the quoted <template> inside the close tag's attribute must not count as an open"
        );

        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(
            blocks[0].close_tag_start as usize, outer_close,
            "the outer template must close at the OUTER </template>, not the attr-bearing inner close"
        );
        let (cs, ce) = blocks[0].content_range();
        assert!(source[cs as usize..ce as usize].contains("<p>after</p>"));
        assert!(
            blocks
                .iter()
                .any(|b| b.tag_name == "script" && b.is_setup()),
            "the script block after the template must still be discovered"
        );
    }

    #[test]
    fn cdata_section_same_name_tokens_are_opaque_to_the_balanced_walk() {
        // `<![CDATA[ … ]]>` is opaque character data (a foreign-content
        // island): a same-name close or open inside it must not shift depth.
        let source = "<template>\n  <svg><![CDATA[ </template> <template> ]]></svg>\n  <p>after</p>\n</template>\n<script setup>\nconst x = 1;\n</script>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(
            blocks[0].close_tag_start as usize,
            source.rfind("</template>").unwrap(),
            "CDATA-interior same-name tokens must not close (or open) the block"
        );
        let (cs, ce) = blocks[0].content_range();
        assert!(source[cs as usize..ce as usize].contains("<p>after</p>"));
        assert!(
            blocks
                .iter()
                .any(|b| b.tag_name == "script" && b.is_setup()),
            "the script block after the template must still be discovered"
        );
    }

    #[test]
    fn script_and_style_stay_raw_text_first_close_wins() {
        // A `<script>` open inside a JS string must NOT open a nesting level:
        // script/style are raw-text blocks — the first close ends the block
        // (HTML raw-text semantics), and the style block after must be found.
        let source = "<script>\nconst s = \"<script>\";\n</script>\n<style>\n.a {}\n</style>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(
            blocks.len(),
            2,
            "expected script + style, got: {:?}",
            blocks.iter().map(|b| &b.tag_name).collect::<Vec<_>>()
        );
        assert_eq!(blocks[0].tag_name, "script");
        assert_eq!(
            blocks[0].close_tag_start as usize,
            source.find("</script>").unwrap(),
            "a stray `<script>` in a JS string must not defer the raw-text close"
        );
        assert_eq!(blocks[1].tag_name, "style");
    }
}
