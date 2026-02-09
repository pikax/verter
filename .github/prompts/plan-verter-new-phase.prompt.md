# Revised Plan: Verter Core Vue Compiler - Critical Bug Fixes

## Investigation Summary

After thorough exploration, **many items in the original plan are already correctly implemented**. This revised plan focuses only on the **critical bugs that produce invalid JavaScript output**.

### Already Working (Original Plan Was Incorrect)

| Feature | Status | Evidence |
|---------|--------|----------|
| Multi-root Fragment wrapping | ✅ Working | `element.rs:1072-1183` |
| Block root tracking (openBlock) | ✅ Working | `element.rs:174-183` |
| Key props for v-if/v-else | ✅ Working | `element.rs:137-172` |
| Patch flags with comments | ✅ Working | `element.rs:1656-1706` |
| Async context __temp/__restore | ✅ Working | `script.rs:245-259` |
| Component name from filename | ✅ Working | `codegen.rs:66-87` |
| Event modifiers | ✅ Working | All modifiers work |
| Key modifiers | ✅ Working | `_withKeys()` correct |

---

## Critical Bugs to Fix (Invalid JS Output)

---

## Complete Syntax Error Inventory

### builtin-components.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 54 | Negation in wrong place | `disabled: _ctx.!showModal` |
| 56 | Bracket mismatch | `"Conditional teleport"])` |
| 64, 77 | Bracket mismatch | `"Fade transition"])` |
| 69 | Bracket in wrong place | `_toDisplayString(_ctx.currentView)])}` |
| 75 | Unquoted hyphenated props | `onBefore-enter:`, `onAfter-enter:` |
| 85 | Bracket mismatch | `_toDisplayString(item.name)])}` |
| 96 | Invalid array/number syntax | `include: _ctx.['CompA', 'CompB'], max: _ctx.10` |
| 111-112 | Dangling null | `null))` random appearance |

### dynamic-component.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 48-50 | Missing closing paren | Unclosed `_createBlock(...)` |
| 63 | Double comma, empty arg | `null, , [` |
| 67, 86 | Dangling null | `null))` |
| 76 | Unquoted hyphenated prop | `shared-prop:` |

### patch-flags.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 62 | Missing string concat | `" Static prefix: "_toDisplayString(...)` |
| 69 | Object after _ctx. | `class: _ctx.{ active: isActive }` |
| 71 | Array after _ctx. | `class: _ctx.[baseClass, conditionalClass]` |
| 78 | Object after _ctx. | `style: _ctx.{ color: textColor, ... }` |
| 80 | Array after _ctx. | `style: _ctx.[baseStyle, overrideStyle]` |
| 87 | Unquoted hyphenated prop | `data-value: _ctx.dataValue` |
| 93 | Empty property name | `{ : _ctx.allProps }` |
| 95 | Double brackets | `[[dynamicPropName]]:` |

### custom-directives.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 18-26 | Hyphenated prop names | `{ v-focus: "" }`, `{ v-tooltip: "..." }` |

### v-bind-spread.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 56, 61, 66, 68, 81, 86, 91, 96 | Empty property name | `{ : _ctx.attrs }` |
| 73 | Object after _ctx. | `{ : _ctx.{ type: 'text', ... } }` |

### v-once.js
| Line | Error | Invalid Code |
|------|-------|--------------|
| 28, 33, 42, 46 | Broken on prop | `{ on:  }` |
| 28, 46 | Missing string concat | `" Static: "_toDisplayString(...)` |

### key-modifiers.js (Minor)
| Line | Error | Invalid Code |
|------|-------|--------------|
| 33-37, 41-44 | Missing commas | Between sibling `_createElementVNode` calls |

### v-model-modifiers.js (Minor)
| Line | Error | Invalid Code |
|------|-------|--------------|
| 31-48 | Missing commas | Between sibling elements |

---

## Root Cause Categories

| Category | Description | Files Affected |
|----------|-------------|----------------|
| **Missing commas** | Sibling elements not separated | All files |
| **Empty prop names** | `v-bind="obj"` → `{ : value }` | v-bind-spread, patch-flags |
| **Object/array literal after _ctx.** | `:class="{...}"` → `_ctx.{...}` | patch-flags |
| **Unquoted hyphenated props** | `data-value:` instead of `"data-value":` | patch-flags, builtin, dynamic |
| **Missing string concat** | `"text"_toDisplayString()` | patch-flags, v-once |
| **Directive as prop** | `v-focus: ""` | custom-directives |
| **v-once broken** | `{ on: }` | v-once |
| **Bracket mismatches** | Unclosed/misplaced brackets | builtin, dynamic |
| **Negation placement** | `_ctx.!value` | builtin |

---

## Bug #1: v-bind Spread (Highest Priority)

**Symptom:** Outputs `{ : _ctx.attrs }` - empty property name is invalid JS.

**Root Cause Analysis:**
- **Detection:** [directives.rs:253-257](crates/verter_core/src/codegen/vue/template/directives.rs#L253-L257) - When `v-bind="obj"` has no argument, `event.arg` is `None`, creating `Span::new(0, 0)`
- **Output:** [element.rs:1228-1241](crates/verter_core/src/codegen/vue/template/element.rs#L1228-L241) - `write_props()` writes empty string + ": " for `PropKind::Bind`
- **Missing helpers:** `NORMALIZE_PROPS` and `MERGE_PROPS` defined in types.rs:339-340 but never used

**Fix Steps:**

1. **Add `PropKind::BindSpread` variant** in [types.rs](crates/verter_core/src/codegen/vue/template/types.rs):
   ```rust
   pub enum PropKind {
       Static,
       Bind,
       BindSpread,  // NEW: v-bind="obj" without attribute name
       On,
       Model,
       // ...
   }
   ```

2. **Detect spread in** [directives.rs:249-271](crates/verter_core/src/codegen/vue/template/directives.rs#L249-L271):
   ```rust
   if prop_name.starts_with(':') || prop_name.starts_with("v-bind") {
       if event.arg.is_none() {
           // v-bind="obj" spread
           (PropKind::BindSpread, Span::new(0, 0), value, false)
       } else {
           // :prop="val" regular bind
           (PropKind::Bind, name, value, is_dynamic)
       }
   }
   ```

3. **Handle spread in** [element.rs:write_props()](crates/verter_core/src/codegen/vue/template/element.rs#L1200):
   - If element has ONLY spread: wrap entire props with `_normalizeProps(_guardReactiveProps(...))`
   - If spread + other props: use `_mergeProps({static props}, spread)`
   - Set `NORMALIZE_PROPS`, `GUARD_REACTIVE_PROPS`, and/or `MERGE_PROPS` helper flags

4. **Add helper imports** to [types.rs](crates/verter_core/src/codegen/vue/template/types.rs):
   ```rust
   pub const GUARD_REACTIVE_PROPS: u32 = 1 << 15;  // Add if missing

   // In HelperFlags::to_imports():
   if self.contains(Self::GUARD_REACTIVE_PROPS) {
       imports.push("guardReactiveProps as _guardReactiveProps");
   }
   ```

---

## Bug #2: Custom Directives

**Symptom:** Outputs `{ v-focus: "" }` - hyphenated unquoted property names are invalid JS.

**Root Cause Analysis:**
- **Detection:** [directives.rs:291-295](crates/verter_core/src/codegen/vue/template/directives.rs#L291-L295) - Custom directives (`v-focus`, `v-tooltip`) fall through to `else` clause and become `PropKind::Static`
- **Output:** [element.rs:1219-1226](crates/verter_core/src/codegen/vue/template/element.rs#L1219-L226) - Static props output literally: `v-focus: ""`
- **Missing infrastructure:** `WITH_DIRECTIVES` helper exists but is never set; `RESOLVE_DIRECTIVE` helper missing

**Fix Steps:**

1. **Add `PropKind::CustomDirective` variant** in [types.rs](crates/verter_core/src/codegen/vue/template/types.rs):
   ```rust
   pub enum PropKind {
       // ...existing...
       CustomDirective,  // v-custom, v-focus, v-tooltip, etc.
   }
   ```

2. **Add directive tracking** to `CurrentElement` in [types.rs:209-231](crates/verter_core/src/codegen/vue/template/types.rs#L209-L231):
   ```rust
   pub struct CurrentElement {
       // ...existing...
       pub custom_directives: Vec<CustomDirectiveEntry>,
   }

   pub struct CustomDirectiveEntry {
       pub name: String,      // "focus", "tooltip"
       pub arg: Option<Span>, // :arg
       pub modifiers: Vec<String>, // .mod1.mod2
       pub value: Option<Span>,
   }
   ```

3. **Add `RESOLVE_DIRECTIVE` helper** in [types.rs:320-348](crates/verter_core/src/codegen/vue/template/types.rs#L320-L348):
   ```rust
   pub const RESOLVE_DIRECTIVE: u32 = 1 << 16;

   // In to_imports():
   if self.contains(Self::RESOLVE_DIRECTIVE) {
       imports.push("resolveDirective as _resolveDirective");
   }
   ```

4. **Detect custom directives** in [directives.rs:291](crates/verter_core/src/codegen/vue/template/directives.rs#L291) (before `else`):
   ```rust
   } else if prop_name.starts_with("v-") && !is_builtin_directive(prop_name) {
       // Custom directive
       let directive_name = &prop_name[2..]; // Remove "v-"
       (PropKind::CustomDirective, name, value, false)
   } else {
       // Static attribute
       (PropKind::Static, name, value, false)
   }
   ```

5. **Generate `_withDirectives` wrapper** in [element.rs](crates/verter_core/src/codegen/vue/template/element.rs):
   - Collect custom directives during prop processing
   - Wrap element with `_withDirectives(element, [[directive1], [directive2, value, arg, modifiers]])`
   - Set `WITH_DIRECTIVES` and `RESOLVE_DIRECTIVE` helpers

---

## Bug #3: v-once Directive

**Symptom:** Outputs `{ on: }` - malformed property.

**Root Cause Analysis:**
- **Detection:** [directives.rs:1256](crates/verter_core/src/codegen/vue/template/directives.rs#L1256) - v-once is only recognized as event modifier (like `.once` on `@click`), not as standalone directive
- **Hoisting bug:** [element.rs:1721-1751](crates/verter_core/src/codegen/vue/template/element.rs#L1721-L751) - `generate_props_code()` doesn't handle `PropKind::On`, outputs `on: ""`
- **Missing v-once handling:** No code path for `v-once` as standalone directive on elements

**Fix Steps:**

1. **Detect v-once as directive** in [directives.rs](crates/verter_core/src/codegen/vue/template/directives.rs) prop detection:
   ```rust
   if prop_name == "v-once" {
       // Mark element for caching, don't add to props
       return; // Or set a flag on CurrentElement
   }
   ```

2. **Add v_once flag** to `CurrentElement` in [types.rs](crates/verter_core/src/codegen/vue/template/types.rs):
   ```rust
   pub struct CurrentElement {
       // ...existing...
       pub v_once: bool,
   }
   ```

3. **Generate cache wrapper** in [element.rs](crates/verter_core/src/codegen/vue/template/element.rs):
   ```rust
   if elem.v_once {
       let cache_idx = state.cache_index;
       state.cache_index += 1;
       code = format!("_cache[{}] || (_cache[{}] = {})", cache_idx, cache_idx, code);
   }
   ```

4. **Fix `generate_props_code()`** at [element.rs:1721-1751](crates/verter_core/src/codegen/vue/template/element.rs#L1721-L751):
   - Skip `PropKind::On` props (they shouldn't reach this function)
   - Or add proper handling for event handlers

---

## Implementation Order

| Priority | Bug | Impact | Complexity |
|----------|-----|--------|------------|
| 1 | v-bind spread | Breaks any `v-bind="obj"` usage | Medium |
| 2 | Custom directives | Breaks all custom directives | High |
| 3 | v-once | Breaks v-once elements | Medium |

---

## Testing Strategy (Critical Gap)

### Current Testing Problem

The existing tests only check for **presence of strings** (e.g., `code.contains("_withKeys")`), but they don't validate that the output is **syntactically valid JavaScript**. This means broken output passes all tests.

**Examples of invalid JS that passes tests:**

| File | Invalid Syntax | Line |
|------|----------------|------|
| patch-flags.js | `class: _ctx.{ active: isActive }` | 69 |
| patch-flags.js | `" Static prefix: "_toDisplayString(...)` (missing `+`) | 62 |
| patch-flags.js | `data-value: _ctx.dataValue` (unquoted hyphen) | 87 |
| builtin-components.js | `disabled: _ctx.!showModal` | 54 |
| builtin-components.js | `onBefore-enter:` (hyphen without quotes) | 75 |
| builtin-components.js | `include: _ctx.['CompA', 'CompB']` | 96 |
| dynamic-component.js | `null, , [` (double comma) | 63 |
| custom-directives.js | `{ v-focus: "" }` (hyphenated unquoted prop) | 18 |
| v-bind-spread.js | `{ : _ctx.attrs }` (empty property name) | 56 |

### New Testing Approach

#### Layer 1: JS Syntax Validation (Add First)

Create a test helper that validates generated code is parseable JavaScript:

```rust
// In crates/verter_core/src/builder/codegen.rs or new test_utils.rs

use oxc::parser::Parser;
use oxc::span::SourceType;
use oxc_allocator::Allocator;

/// Validates that generated code is syntactically valid JavaScript
fn assert_valid_js(code: &str, context: &str) {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path("test.js").unwrap();
    let parser_result = Parser::new(&allocator, code, source_type).parse();

    assert!(
        parser_result.errors.is_empty(),
        "Generated code is not valid JavaScript!\n\
         Context: {}\n\
         Errors: {:?}\n\
         Generated:\n{}",
        context,
        parser_result.errors,
        code
    );
}

/// Test helper that generates and validates
fn gen_and_validate(source: &str) -> String {
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue".to_string());
    let result = generate(source, &options, &allocator);

    assert_valid_js(&result.code, source);
    result.code
}
```

#### Layer 2: Pattern-Based Negative Tests

Add tests that explicitly check for known invalid patterns:

```rust
#[test]
fn test_no_invalid_patterns() {
    let test_cases = vec![
        (r#"<template><div v-bind="obj"/></template><script setup>const obj={}</script>"#, "v-bind spread"),
        (r#"<template><div :class="{active}"/></template><script setup>const active=true</script>"#, "object class"),
        (r#"<template><div v-focus/></template><script setup></script>"#, "custom directive"),
    ];

    let invalid_patterns = [
        ("{ :", "empty property name"),
        ("_ctx.{", "object literal after _ctx."),
        ("_ctx.[", "array literal after _ctx."),
        ("{ v-", "hyphenated directive as prop"),
        (": _ctx.!", "negation after colon"),
        ("null))", "dangling null closing"),
        (", ,", "double comma"),
        ("\"_toDisplayString", "missing concatenation operator"),
    ];

    for (source, context) in test_cases {
        let code = gen_and_validate(source);

        for (pattern, desc) in &invalid_patterns {
            assert!(
                !code.contains(pattern),
                "Found invalid pattern '{}' ({}) in {}.\nGenerated:\n{}",
                pattern, desc, context, code
            );
        }
    }
}
```

#### Layer 3: AST Comparison with Vue's Official Compiler (PRIMARY METHOD)

**This is the safest way to ensure correctness** - generate the source of truth at test time using Vue's official compiler, then compare ASTs.

**Workflow for Each Test:**
1. Create a temp `.vue` file: `examples/codegen/source/TEMP_FILE.vue`
2. Run `node codegen.js` to compile with Vue's official compiler
3. Copy the generated content (`TEMP_FILE.vue.js`) to the test as a **static string** - this is the SOURCE OF TRUTH
4. Parse the Vue output with oxc to get the reference AST
5. Run verter's generator on the same Vue source
6. Parse our output with oxc
7. Compare the ASTs structurally (ignoring offsets/spans due to indentation)

**What to Compare:**
- ✅ Node types (CallExpression, Identifier, etc.)
- ✅ Property names and values
- ✅ Function call names (`_createElementVNode`, `_withDirectives`, etc.)
- ✅ Import/export declarations
- ✅ Array element count and order
- ✅ Conditional structure (ternary nesting)

**What to Ignore:**
- ❌ Span/offset positions (will differ due to indentation)
- ❌ Whitespace and formatting
- ❌ Comment content
- ❌ String quote style (single vs double)

```rust
#[test]
fn e2e_simple_template_ast_matches_vue() {
    // Step 1: Vue source (same as what you put in TEMP_FILE.vue)
    let vue_source = r#"<template>
  <div class="hello">Hello</div>
</template>
<script setup>
</script>"#;

    // Step 2: Vue compiler output (copy from TEMP_FILE.vue.js after `node codegen.js`)
    // THIS IS THE SOURCE OF TRUTH
    let vue_output = r#"import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue";
export default {
  __name: "anonymous",
  setup(__props) {
    return (_ctx, _cache) => {
      return _openBlock(), _createElementBlock("div", { class: "hello" }, "Hello");
    };
  }
};"#;

    // Step 3: Run verter generator
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue".to_string());
    let our_result = generate(vue_source, &options, &allocator);

    // Step 4: Validate our output is valid JS
    assert_valid_js(&our_result.code, "verter output");

    // Step 5: Compare AST structures
    let diffs = compare_ast_structure(&our_result.code, vue_output, "simple_template");

    assert!(
        diffs.is_empty(),
        "AST differs from Vue:\n{}\n\nOurs:\n{}\n\nVue:\n{}",
        diffs.join("\n"), our_result.code, vue_output
    );
}
```

**How to Generate Vue Source of Truth:**
```bash
# 1. Create temp Vue file
echo '<template><div>Test</div></template><script setup></script>' > examples/codegen/source/TEMP_FILE.vue

# 2. Run Vue compiler
node codegen.js

# 3. Copy output to your test
cat examples/codegen/source/TEMP_FILE.vue.js

# 4. Clean up
rm examples/codegen/source/TEMP_FILE.vue examples/codegen/source/TEMP_FILE.vue.js
```

#### Layer 4: Specific Bug Tests

For each bug, write tests that both validate syntax AND check correct output:

```rust
#[test]
fn test_vbind_spread_produces_valid_js() {
    let source = r#"<template><div v-bind="attrs">Hi</div></template>
<script setup>const attrs = {}</script>"#;

    let code = gen_and_validate(source);  // Validates JS syntax

    // Check correct output
    assert!(
        code.contains("_normalizeProps") || code.contains("_guardReactiveProps"),
        "v-bind spread should use normalization helpers. Generated:\n{}",
        code
    );
    assert!(
        !code.contains("{ :"),
        "Should not have empty property name. Generated:\n{}",
        code
    );
}

#[test]
fn test_custom_directive_produces_valid_js() {
    let source = r#"<template><input v-focus/></template>
<script setup></script>"#;

    let code = gen_and_validate(source);  // Validates JS syntax

    assert!(
        code.contains("_withDirectives") && code.contains("_resolveDirective"),
        "Custom directive should use directive helpers. Generated:\n{}",
        code
    );
    assert!(
        !code.contains("v-focus:"),
        "Should not output directive as literal prop. Generated:\n{}",
        code
    );
}
```

---

## Implementation Order (Revised)

### Phase 0: Add Validation Infrastructure (Do First!)

**Note: CLAUDE_IMPLEMENTATION_GUIDE.md has been updated with mandatory JS validation and AST comparison requirements.**

**Step 0.1: JS Syntax Validation Helpers**
1. Add `assert_valid_js()` helper using oxc parser
2. Add `gen_and_validate()` helper
3. Add `INVALID_PATTERNS` constant
4. Add `assert_no_invalid_patterns()` helper

**Step 0.2: AST Comparison Infrastructure (PRIMARY METHOD)**
5. Add `compare_ast_structure()` helper that:
   - Takes our generated code and Vue's official output as strings
   - Parses both with oxc
   - Compares AST structure (ignoring spans/offsets/indentation)
   - Returns list of structural differences

6. Create initial AST comparison tests using this workflow:
   - Create `examples/codegen/source/TEMP_FILE.vue` with test template
   - Run `node codegen.js` to get Vue's official output
   - Copy the content of `TEMP_FILE.vue.js` as a **static string** in the test
   - Clean up temp files
   - The test compares verter output AST with the static Vue output

7. **Run tests - some WILL fail** (confirms structural differences exist)

8. **Mark failing tests with `#[ignore]`** - Initially, some tests will fail due to existing bugs. Use `#[ignore]` attribute to skip them temporarily:
   ```rust
   #[test]
   #[ignore] // TODO: Fix v-bind spread codegen
   fn e2e_vbind_spread_ast_matches_vue() { ... }
   ```

9. Create a tracking list of ignored tests to fix in Phase 1

10. Run `cargo test` - all non-ignored tests should pass
11. Run `cargo test --ignored` - shows what's left to fix

**Why AST Comparison (with this workflow)?**
- String comparison is fragile (formatting differences cause false failures)
- Syntax validation only checks if code parses, not if it's correct
- AST comparison verifies structural correctness
- Using static strings from Vue's compiler output makes tests self-contained
- The source of truth is always generated fresh from Vue's official compiler
- Tests are deterministic and don't depend on external files at runtime

### Phase 1: Fix Critical Syntax Bugs

For each bug, the workflow is:
1. Find the ignored test for this bug
2. Remove `#[ignore]` attribute
3. Run the test - it should fail
4. Fix the bug
5. Run the test - it should pass
6. Run `cargo test` - all other tests should still pass
7. Commit

Fix bugs in this order:

1. **String concatenation** - Missing `+` between static text and `_toDisplayString`
2. **Property name quoting** - Hyphenated props need quotes
3. **v-bind spread** - Empty property names
4. **Object/array class/style** - `_ctx.{...}` invalid syntax
5. **Custom directives** - `v-focus:` invalid prop names
6. **Missing commas** - Between sibling elements

### Phase 2: Verify All Tests Pass

After all bugs are fixed:
1. Run `cargo test` - all tests should pass
2. Run `cargo test --ignored` - should show 0 tests (all fixed)
3. AST comparison tests verify structural correctness against Vue's compiler

---

## TDD Workflow (Revised)

For each bug fix:

### Step 1: Write Failing Test (with validation)
```rust
#[test]
fn test_feature_name() {
    let source = r#"..."#;
    let code = gen_and_validate(source);  // MUST parse as valid JS

    assert!(code.contains("expected"), "...");
    assert!(!code.contains("invalid_pattern"), "...");
}
```

### Step 2: Verify Test Fails
```bash
cargo test --package verter_core test_feature_name 2>&1 | tail -30
```

### Step 3: Implement Fix

### Step 4: Verify Test Passes + Full Suite
```bash
cargo test --package verter_core 2>&1 | tail -60
```

---

## Files to Modify

| File | Changes |
|------|---------|
| [codegen.rs](crates/verter_core/src/builder/codegen.rs) | Add `assert_valid_js()`, validation tests |
| [types.rs](crates/verter_core/src/codegen/vue/template/types.rs) | Add `PropKind` variants, helper flags |
| [directives.rs](crates/verter_core/src/codegen/vue/template/directives.rs) | Fix detection logic |
| [element.rs](crates/verter_core/src/codegen/vue/template/element.rs) | Fix code generation |
| [interpolation.rs](crates/verter_core/src/codegen/vue/template/interpolation.rs) | Fix string concatenation |
