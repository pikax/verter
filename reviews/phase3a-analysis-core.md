# Phase 3a: Analysis Core Review

## Overall: WELL-ENGINEERED — Production Ready with Caveats

Single-pass O(n) AST analysis, zero-copy lifetimes, comprehensive test coverage (72 tests).

---

## Critical Issues

### HIGH: Destructuring Bindings Not Extracted
`build_script_analysis()` only matches `BindingPattern::BindingIdentifier`. Destructured patterns are skipped:
```typescript
const { count, ref } = useCounter();  // NOT extracted
const [value, setValue] = useState();  // NOT extracted
```
**Impact**: LSP completions miss these bindings, cross-file analysis incomplete.
**Fix**: Implement recursive destructuring walker.

### MEDIUM: Qualified Names in Type Inheritance
`interface Local extends NS.Base {}` — code only matches `Expression::Identifier`, so qualified names silently dropped.
**Impact**: Rare in Vue but possible with namespace types.

### MEDIUM: ProjectIndex Memory Unbounded
No pruning mechanism. Each `add_file()` grows indexes indefinitely. 10k+ files could cause memory pressure.
**Impact**: Typical projects <2k files are fine; component libraries could be affected.

---

## Strengths

### Binding Analysis: CORRECT
- All binding kinds properly categorized (Const, Let, Var, Function, Class, etc.)
- Three-level reactivity classification: boolean, ReactivityKind enum, composable heuristic
- `using`/`awaitUsing` (ES2024) handled
- Vue API detection by imported name, not alias

### Import/Export: COMPREHENSIVE
- Declaration + specifier level type-only imports
- Namespace imports, re-exports, star exports
- Export change detection via SHA-256 hashing
- `safe_slice()` prevents panics on malformed spans
- 13 tests covering all edge cases

### Type References: EXCELLENT
All TypeScript type constructs handled:
- References, generics, unions, intersections, type literals
- Arrays, tuples, conditionals, mapped types, indexed access
- Type operators (keyof, typeof), template literals, function types
- Qualified names (NS.Type)

### Cross-File Type Resolution: SOLID
- Transitive dependency discovery via `derive_macro_type_deps()`
- Interface extends + type alias intersection chains followed
- Cycle detection with visited set
- Diamond inheritance deduplication
- 5 cross-file tests covering deep chains

### Scope Tracking: EXCELLENT
- Fine-grained bitflag system (AnalysisScope)
- Presets for Build, LSP, Linter, Build+Optimize
- No nested scope issues (single O(n) pass over top-level statements)

### OXC AST Usage: CORRECT
- All functions properly bound to allocator lifetime
- Return types are owned (no lifetime escapes)
- Helper functions return `&str` slices valid during parsing
- Arena not exposed to callers

---

## Edge Cases

| Case | Status | Notes |
|------|--------|-------|
| Specifier-level type-only imports | Handled | `import { type Foo, bar }` |
| Re-export chains | Not followed | Caller (verter_host) resolves |
| Star exports | Tracked as `"*"` | Caller responsibility |
| Vue API from @vue/* packages | Not classified | Only `from 'vue'` recognized |
| First await in function bodies | Correctly skipped | Only top-level await detected |
| Circular type inheritance | Protected by visited set | No test but safe |

## Test Coverage: GOOD (72 tests)
- Imports: 8 tests (all edge cases)
- Exports: 5 tests (re-exports well-tested)
- Macros/Types: 5 tests (transitive deps)
- Bindings: 20+ tests (missing destructuring)
- Analysis flags: 10+ tests
- ProjectIndex: ~50 tests (functional, no stress tests)

Missing: destructuring extraction, circular inheritance, qualified names in heritage, large project memory.
