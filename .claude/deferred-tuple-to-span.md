# Deferred Tuple-to-Span Changes

These fields currently use tuples (`Vec<(u32, u32)>` or `Vec<(String, u32, u32)>`) for span data. Converting them to use `Span` or `Vec<Span>` would change the JSON serialization format (since `Span` has custom serde that emits flat `spanStart`/`spanEnd` keys, while tuples serialize as JSON arrays).

**Review needed:** Decide whether to change the wire format or keep backward compatibility.

---

## 1. `TemplateAnalysisSnapshot.v_if_v_for_conflicts`

**File:** `crates/verter_analysis/src/template.rs`

```rust
// Current:
pub v_if_v_for_conflicts: Vec<(u32, u32)>,

// Proposed:
pub v_if_v_for_conflicts: Vec<Span>,
```

**JSON impact:**
- Current: `"vIfVForConflicts": [[10, 30], [50, 70]]`
- After: `"vIfVForConflicts": [{"spanStart": 10, "spanEnd": 30}, {"spanStart": 50, "spanEnd": 70}]`

**Notes:** Each tuple is `(span_start, span_end)` of an element that has both `v-if` and `v-for`. Used by diagnostics (no_use_v_if_with_v_for rule) and MCP/FFI consumers.

---

## 2. `AnalyzedEmitDefinition.emit_locations`

**File:** `crates/verter_analysis/src/template.rs`

```rust
// Current:
pub emit_locations: Vec<(u32, u32)>,

// Proposed:
pub emit_locations: Vec<Span>,
```

**JSON impact:**
- Current: `"emitLocations": [[100, 120], [200, 220]]`
- After: `"emitLocations": [{"spanStart": 100, "spanEnd": 120}, {"spanStart": 200, "spanEnd": 220}]`

**Notes:** Each tuple is `(span_start, span_end)` of `$emit('eventName')` call sites in the template. Used by LSP references/definition features.

---

## 3. `IfChain.conditions`

**File:** `crates/verter_analysis/src/template.rs`

```rust
// Current:
pub conditions: Vec<(String, u32, u32)>,

// Proposed — Option A (named struct):
pub conditions: Vec<IfCondition>,

pub struct IfCondition {
    pub expression: String,
    pub span: Span,
}

// Proposed — Option B (keep mixed):
// No change — the String makes it harder to use Span directly
```

**JSON impact (Option A):**
- Current: `"conditions": [["show", 10, 30], ["!hidden", 50, 70]]`
- After: `"conditions": [{"expression": "show", "spanStart": 10, "spanEnd": 30}, {"expression": "!hidden", "spanStart": 50, "spanEnd": 70}]`

**Notes:** Each tuple is `(expression_text, span_start, span_end)` for if/else-if conditions in a chain. Used by diagnostics and template analysis snapshot. Option A is cleaner but changes wire format more significantly. Option B defers the change.

---

## Decision Criteria

- If these fields are only consumed internally (Rust crate to Rust crate), the JSON format change is fine
- If TS/VS Code extension or MCP clients parse these fields, the format change needs a version bump or migration
- `v_if_v_for_conflicts` and `emit_locations` are exposed via MCP `analyze_file` and FFI `compile()`
- `IfChain.conditions` is part of `TemplateAnalysisSnapshot` which is serialized in custom protocol data
