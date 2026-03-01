# Phase 3b: CSS & Template Analysis Review

## Overall: WELL-DESIGNED — Mostly Correct with CSS Syntax Gaps

Three-valued selector matching is sound. Main limitation: incomplete CSS syntax support (escape sequences, comments in selectors).

---

## Critical Issues

### HIGH: CSS Comments in Selectors Cause Parse Failure
Scanner does NOT strip comments before passing selector text to `parse_selector()`. Example:
```css
.a /* comment */ > .b { }
```
Parser encounters `/`, skips it as unknown, produces malformed parse. Selector structure is `None` (fails silently).
**Fix**: Strip comments in `scan_css()` before selector extraction.

### MEDIUM: CSS Escape Sequences Not Recognized
`is_css_ident_start()` only checks ASCII letters, underscore, hyphen, or bytes > 0x7F. CSS escapes like `.\2d-foo` are not parsed.
**Impact**: Classes/IDs with escape sequences won't match in selector matching.

---

## CSS Scanner: SOLID with Edge Cases

**Strengths**:
- Comment handling (`/* */`): skips at all nesting levels
- String escaping: quotes properly handled
- At-rule detection: @media, @keyframes, @supports, @layer, @container, @import, @font-face, @property, @scope
- Nested at-rules tracked via depth stack
- Brace-depth tracking for nested blocks

**Gaps**:
- Escape sequences in identifiers
- Nested rules in @layer/@supports not fully handled
- Custom property `var()` references not separately extracted
- Comments inside selectors pass through to parser

## Selector Parser: INCOMPLETE for Modern CSS

**Supported**:
- Element, class, ID, universal selectors
- Attribute selectors (all operators: ~=, |=, ^=, $=, *=)
- Pseudo-classes: :hover, :focus, :first-child, :not(), :is(), :where()
- Pseudo-elements: ::before, ::after
- All combinators: descendant, child, adjacent, general sibling

**Not Supported**:
- `:has()` — explicitly rejected as "too complex"
- Attribute case-sensitivity flag (`[i]`)
- CSS escape sequences in identifiers

## Specificity: CORRECT per CSS Spec

- IDs (a), Classes/Attributes (b), Types (c) — all correct
- `:where()` → 0 specificity
- `:is()` → max of arguments
- Hex color collision check (#fff vs #id) — handled
- Minor edge: `:is(:where(.a))` in text-based version doesn't fully track inner `:where()` depth

## Selector Matching: SOUND Three-Valued Logic

**MaybeMatches returned for**:
- Component elements with type selectors (might render as any element)
- Dynamic `:class` bindings (conditional classes)
- Dynamic `:id` or attribute values

**Minor issues**:
- Sibling combinator chains break after first match (non-issue in practice)
- Descendant combinator conservatively returns MaybeMatches (correct behavior)

## Template Analysis: COMPREHENSIVE

### Dynamic Class Extraction Handles:
- Object syntax: `{ 'my-class': condition }` → `["my-class"]`
- Array syntax: `['foo', { bar: cond }]` → `["foo", "bar"]`
- Ternary: `isActive ? 'active' : 'inactive'` → `["active", "inactive"]`
- Logical: `cond && 'active'` → `["active"]`
- Template literal keys: `` { `test-${foo}`: cond } `` → `["test-"]` (partial, marked)
- Nested ternaries, mixed arrays/objects

### Not Extracted (Correctly):
- Plain variables (`myClasses`)
- Function calls (`getClasses()`)
- Computed property names (`{ [key]: value }`)

## Span Handling: CORRECT
- CSS selector spans relative to style block content, converted via `content_offset`
- Class/ID offsets include selector position
- Template expression offsets verified by tests
- No off-by-one errors detected

## Recommendations
1. **High**: Strip comments before selector parsing
2. **High**: Document escape sequence limitation
3. **Medium**: Log warnings for selectors that fail to parse
4. **Low**: Consider `:has()` support if needed for lint rules
