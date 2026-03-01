# Phase 5c: LSP Display Features + TSGO Integration Review

## Overall: STRONG — Well-Architected TSGO Integration, Minor Gaps

---

## Completion Features

**Status: Well-implemented with minor concerns**

**Strengths**:
- Structured approach: separates class_attribute, script, template, and component_prop completions
- Smart trigger logic: `is_incomplete: true` for CSS class completions enables live filtering
- Cross-file component prop resolution via optional `resolve_component` callback
- Reactivity context indicators (ref, computed, reactive, maybe-ref) in templates
- Strips `___VERTER___` prefixed symbols from output

**Issues**:
- **P2**: Dynamic `:class` string parsing is fragile — basic byte scanning, no balanced brace/quote validation
- **P2**: Component name detection uses heuristic backward scan — could match partial names
- **P3**: No CSS custom property completions in template
- **P3**: CSS class completions don't indicate scoped vs unscoped source

---

## Hover Features

**Status: Strong with CSS selector matching excellence**

**Strengths**:
- Vue API hover shows category, sync requirements, import source
- CSS element hover matches selectors with three-valued logic, sorts by specificity
- Binding reactivity hints (ref needs `.value`, computed is read-only)
- Binding initializer info (function call source, literal kind)
- Excludes HTML comments from hover

**Issues**:
- **P2**: CSS selector matching spans depend on `content_offset` accuracy — no validation
- **P2**: No hover for `@event` listeners in template (could show handler type)
- **P3**: MaybeMatches doesn't distinguish confidence levels

---

## Inlay Hints

**Status: Good with correct span handling**

**Strengths**:
- DOM query matching via structured selector parser → template elements
- Accurate hint positioning via `script_offset` conversion
- useTemplateRef integration with proper error messaging
- Dynamic class handling shows "possible match"

**Issues**:
- **P2**: Script offset assumes single script block — companion script calls get wrong positions
- **P3**: No fallback for complex selectors when structured parse returns None
- **P3**: Element descriptions truncate without indication for long class lists

---

## Diagnostics Mapping

**Status: Correct but minimal**

- Clean severity mapping (Error/Warning/Info)
- Graceful fallback for missing spans to (0,0)

**Issues**:
- **P2**: No deduplication between verter and TSGO diagnostics
- **P2**: All diagnostics source="verter" — no way to distinguish TSGO origin in UI
- **P3**: Zero-width range for equal start/end spans — some clients won't highlight

---

## CSS Diagnostics

**Status: GAP — No unused CSS detection module found**

- No `css_diagnostics.rs` exists in the features directory
- CSS analysis exists in verter_analysis (selector parsing, matching) but no unused CSS warnings
- Significant gap compared to Volar

---

## Other Features

| Feature | Quality | Notes |
|---------|---------|-------|
| Folding Range | Excellent | Multi-line check correct, self-closing skip working |
| Code Lens | Good | Counts accurate, but Range fallback (0,0) makes position wrong |
| Color Info | Strong | Hex/RGB/HSL parsing robust, CSS ID false positive prevention |
| Document Links | Good | URI conversion handles Windows paths |
| Workspace Symbol | Functional | Basic filtering, but Range always default |
| Document Drop Edit | Minimal | Simplistic relative path computation |
| Document Symbols | Solid | Hierarchical SFC structure, but binding spans often zero (fallback) |

---

## TSGO IPC: WELL-ARCHITECTED

**Strengths**:
- LSP JSON-RPC transport with correct Content-Length framing
- 10-second request timeout prevents hangs
- Async response routing by request ID
- Auto-responds to server→client requests (workspace/configuration, registerCapability)
- EOF detection drains pending requests on crash
- Diagnostic caching for publishDiagnostics

**Issues**:
- **P1**: Position encoding assumes ASCII-only TSX — non-ASCII would corrupt offsets
- **P2**: `pack_position()` uses 16-bit line/col — files >65535 lines silently wrap
- **P3**: URI-to-path conversion doesn't validate file:// scheme
- **P3**: No request ID collision detection (theoretical at i64::MAX)

---

## TSGO Merge: EXCELLENT Position Mapping

**Strengths**:
- Round-trip validation prevents queries in synthetic TSX regions
- Graceful fallback for all merge functions
- Diagnostic filtering excludes unmapped positions
- TSGO type signature takes priority in hover, verter context appended
- Completion deduplication strips `___VERTER___` and `$V_` prefixed items

**Issues**:
- **P2**: PositionMapper round-trip checks same line but not same character
- **P2**: Definition mapping hardcodes `.vue.tsx` suffix — `.vue.js`/`.vue.ts` would fail
- **P3**: References deduplication by (uri, range.start) only — could duplicate with different end
- **P3**: Windows file path normalization incomplete

---

## TSGO Resilience: EXCELLENT Auto-Recovery

**Strengths**:
- Crash detection via EOF signal
- Exponential backoff (1s, 2s, 4s) between restarts
- Max 3 restarts with state recovery (re-opens cached files)
- Pending requests immediately error on crash
- User notification via LSP client warnings

**Issues**:
- **P2**: After 3 crashes, stays in verter-only mode permanently — no time-based reset
- **P2**: No timeout for TSGO startup (could hang on broken binary)
- **P3**: Concurrent restart race — requests error without waiting for restart completion

---

## Summary

### P1 (High)
1. CSS diagnostics missing entirely (no unused CSS detection)
2. Position encoding ASCII assumption fragile for future changes
3. File path normalization incomplete on Windows

### P2 (Medium)
1. Dynamic `:class` parsing fragile for complex expressions
2. Symbol ranges often fallback to block-wide (imprecise navigation)
3. TSGO restart counter never resets
4. No TSGO startup timeout
5. Diagnostic deduplication missing
6. PositionMapper round-trip validation incomplete

### P3 (Low)
1. No CSS module/custom property completions in template
2. Workspace symbols lack precise positions
3. Document drop edit uses simplistic path calculation
