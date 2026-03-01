# Phase 2b: Diagnostics Rules Audit

## Summary: 28 rules reviewed — 21 OK, 4 stubs, 3 issues

---

## Rules Table

| Rule | Category | Verdict | Notes |
|------|----------|---------|-------|
| require-v-for-key | Vue | OK | Checks `vfor.has_key` flag. Simple and accurate. |
| valid-v-for | Vue | OK | Validates non-empty variable and iterable. |
| no-duplicate-attributes | Vue | OK | FxHashSet tracking, O(n). |
| no-dupe-v-else-if | Vue | OK | Pre-computed by analysis, just consumes data. |
| no-template-key | Vue | OK | Correctly allows key on `<template v-for>`. |
| no-textarea-mustache | Vue | OK | Span containment within textarea. |
| no-unused-components | Vue | STUB | No-op implementation. Needs script+template cross-ref. |
| no-unused-props | Vue | OK | Checks used_in_template OR used_in_script. |
| no-use-v-if-with-v-for | Vue | OK | Redundant checks (element + pre-computed). Safe. |
| alt-text | A11Y | OK | Covers img, area, input[type=image]. Skips components. |
| aria-role | A11Y | OK | 82 WAI-ARIA 1.2 roles. Case-sensitive. |
| form-control-has-label | A11Y | OK | Skips type="hidden". |
| click-events-have-key-events | A11Y | OK | Detects @click without @keydown/@keyup/@keypress. |
| anchor-has-content | A11Y | **ISSUE** | Only checks self-closing. Misses `<a href="/"></a>`. |
| heading-has-content | A11Y | **ISSUE** | Only checks self-closing. Misses `<h1></h1>`. |
| no-autofocus | A11Y | OK | Simple attribute check. |
| unused-css-selector | CSS | OK | Selector matching via analysis crate. Skips :deep/:global. |
| scoped-css-cascade | CSS | OK | Static + dynamic class cascade detection. |
| undefined-css-class | CSS | **ISSUE** | Span-based class extraction may be fragile with UTF-8/UTF-16 misalignment. Tests only basic cases. |
| no-v-html | Security | OK | Simple flag check. |
| provide-inject-validation | CrossFile | OK | Consumes pre-computed missing/unused providers. |
| no-duplicate-vue | CrossFile | OK | Checks duplicate_vue_versions. Includes npm ls hint. |
| deep-composable-tracking | CrossFile | OK | Effect list from lifecycle + watchers + provide/inject. |
| max-template-depth | Performance | OK | Configurable limit (default 10). |
| prefer-static-class | Performance | OK | Detects :class with string literal. |
| no-inline-lifecycle | Script | OK | Checks vue_api_calls. |
| no-lifecycle-after-await | Script | OK | After first_await_offset + requires_sync_context(). |
| no-ref-as-operand | Reactivity | STUB | Metadata-only, no diagnostic emitted. |
| no-setup-props-reactivity-loss | Reactivity | STUB | Detection incomplete, no diagnostics emitted. |

---

## Critical Issues

### 1. anchor-has-content — False Negative
Only checks self-closing anchors. Empty regular anchors like `<a href="/"></a>` are missed.
**Fix**: Check all anchors for content or aria-label.

### 2. heading-has-content — False Negative
Same issue: only checks self-closing headings. `<h1></h1>` not caught.
**Fix**: Check all headings for content or aria-label.

### 3. undefined-css-class — Potential False Positives
Extracts class names via source substring using span_start/span_end. Risk of false positives if offsets are UTF-8/UTF-16 misaligned. Multi-word class handling may be fragile. Tests only cover basic cases.
**Fix**: Validate offset safety or use more robust parser. Add edge case tests.

## Acknowledged Stubs
- no-unused-components: No-op (needs cross-reference)
- no-ref-as-operand: Metadata-only (needs AST analysis)
- no-setup-props-reactivity-loss: Incomplete (needs destructuring pattern analysis)
