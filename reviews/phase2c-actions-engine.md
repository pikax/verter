# Phase 2c: Actions Engine Review

## Overall: GOOD Foundation — 1 Bug Found

Small, well-designed crate (507 lines). Clean trait system, easy extensibility. One significant bug in RemoveUnusedCss provider.

---

## Engine Dispatch: Simple & Extensible
- Flat registry of `Box<dyn ActionProvider>`
- `fixes_for()` and `actions_at()` iterate all providers
- O(n) per diagnostic — fine for <5 providers, index if >10
- `builtin()` factory maintains single source of truth

## ActionProvider Trait: Clean
- `fixes_for_diagnostic()` — keyed to specific diagnostic rules
- `actions_at()` — keyed to cursor position (refactorings)
- Both default to `vec![]` — implement only what's needed
- Supports multiple alternatives per diagnostic

## CodeAction/FileEdit Types: Sound
- Multi-file edits: supported in types (`FileEdit.file_id`)
- Cross-file edits: **silently dropped at LSP boundary** (diagnostics_bridge lines 163-164)
- Cursor positioning after fix: **not supported** (no `cursor_offset` field)
- Position encoding: correct (SFC-absolute byte offsets)

---

## Bug Found

### RemoveUnusedCss: Grouped Selectors
**Severity: MEDIUM**

For grouped selectors like `.used, .unused { }`, if `.unused` is the diagnostic target, the provider removes the **entire rule** including `.used`.

- Provider assumes one selector per rule
- No comma detection or split logic
- **Silently breaks code** when other selectors in the group are valid

**Fix options**:
1. Remove only the unused selector from the group
2. Refuse to act (return empty vec) when comma detected
3. Add test: `assert!(!source.contains(".unused"))` after fix

---

## Other Findings

### LSP Bridge Re-runs Linter
`action_engine_fixes()` re-runs the linter to reconstruct DiagnosticSet. Necessary because LSP codeAction context only includes range diagnostics. Acceptable (linter is fast) but should be documented.

### Stale Diagnostics
If file changed between diagnostic publish and action request, span matching may miss. Current code matches by rule name AND range — safe but could miss shifted spans.

### Missing Test Coverage
- Grouped selectors
- SCSS nesting
- Minified CSS
- Files without trailing newlines

---

## Extensibility: EXCELLENT
Adding a new provider: implement trait, register in `register_builtin_providers()`.

**Would work**: add-missing-import, suppress-lint-rule, fix-prop-type
**Needs changes**: cross-file refactoring (WorkspaceEdit support), cursor positioning
