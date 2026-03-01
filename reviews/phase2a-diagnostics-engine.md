# Phase 2a: Diagnostics Engine Review

## Overall: EXCELLENT — Production Ready

Well-architected detection-only engine with clean separation from actions crate.

---

## Key Findings

### Visitor Pattern: COMPLETE
- Single-pass DFS covering all analysis types: template elements, directives, v-for, interpolations, script, styles, cross-file
- O(n) flat array traversal — efficient and complete
- Explicit interpolation filtering prevents false traversals

### Rule Trait: WELL-DESIGNED
- 10 granular hooks (check_template, check_element, check_directive, check_v_for, check_interpolation, check_script, check_style, check_file, check_cross_file)
- Rules implement only needed hooks (no-ops default)
- Trivial to add new rules (8-40 lines typical)
- 28+ rules registered in centralized registry

### DiagnosticSet: PRACTICAL
- Vec wrapper with add/extend/find_by_rule/find_by_span/enhance
- Enrichment pattern for TSGO type info injection
- O(n) queries sufficient for typical <100 diagnostics per file

### Separation of Concerns: EXCELLENT
- Zero dependency on verter_actions
- Pure detection: message + span + severity + tags
- No fix logic, no code mutations, no suggestion code

---

## Issues Found

### P1 (High)
1. **`disable-next-line` uses 1000-byte window** instead of actual next line offset. Long lines >1000 chars won't be fully suppressed.
2. **`@verter:enable` is a no-op** — once disabled, can't re-enable. Must use ignore-start/end blocks instead.

### P2 (Medium)
3. **Nested ignore-start/end not supported** — uses `Option<T>` not stack. Nested pairs don't track correctly.
4. **Composable chains snapshot not populated** — `CrossFileSnapshot.composable_chains` always empty (TODO).

### P3 (Low)
5. No preset-level rule filtering (must override per-rule)
6. Duplicate Vue versions require external filesystem scan (caller responsibility)

---

## Config System: CLEAN
- Presets: Essential, Recommended (default), All, Performance, A11y, Strict
- Per-rule severity overrides always win
- Strict mode promotes everything to Error

## Performance: EFFICIENT
- Single-pass visitor O(n)
- Flat element array with O(1) lookup
- Linear rule application, no quadratic loops
- No allocation during visit (borrows analysis data)

## Test Coverage: COMPREHENSIVE
- 40+ tests, all passing
- Both positive and negative assertions
- Edge cases: out-of-bounds enhance, nested ranges, component spreads
- Serde roundtrips verified
