You are operating in a tight **fix → test → commit → continue** loop.
# Vue vs Verter Output Comparison Routine (Autopilot)

> **Primary entrypoint**: Use `scripts/verter-compare-matrix.mjs` for automated 4-mode matrix comparison.
> This document describes the manual/legacy single-mode comparison workflow.
> See also: `cargo run -p verter_core --example check_matrix` for the Rust comparator.

This document instructs an AI agent to **systematically** compare Verter's compiled output against Vue's official compiler output, fix differences via **TDD**, and **loop until every file is processed**.

Vue's `@vue/compiler-sfc` output is the **source of truth**.

---

## Autopilot Contract (Read First)

You are operating in a tight **fix → test → commit → continue** loop.

### Mandatory loop behavior (per iteration)

For **every** issue you attempt:

1. **Write a failing test** (new test only).
2. **Make the smallest possible fix**.
3. Run **full tests**: `cargo test`.
4. If tests pass → **commit**.
5. If tests fail and you cannot fix quickly → **rollback to last commit** and move to the next issue.

**Do not ask questions or request feedback.** Keep moving forward until all files are processed (matched / fixed / cosmetic / skip / deferred then later processed).

### Scope sizing rule (small issues first)

Prefer fixes that are:
- localized to one module
- a single root cause
- Category **A**, **D**, or small **C**
- quick to validate with a single E2E test

If a file has **too many distinct differences** (see “Too many differences” rule), log them in:
- `.results/differences_compare.md`
…and **defer** the file for a later pass.

---

## Hard Rules

1. **Do NOT modify existing tests** unless a fix directly requires it. Prefer adding new tests.
2. **Do NOT modify code unrelated** to the specific difference you're fixing.
3. **No background refactors.** Fix the bug, not the architecture.
4. **All new tests MUST validate JS syntax** using `assert_valid_js(...)`.
5. **Always keep `main` (or your working branch) green**:
   - If you break tests and can’t fix quickly → rollback and move on.
6. If you cannot solve an issue, mark it and continue:
   - `.match` file: `{"status":"skip","reason":"...","date":"YYYY-MM-DD"}`
7. **Do not stop working** until all files are processed (including deferred items in later passes).

---

## Project Context

**Verter** is a Vue compiler written in Rust. Both compilers process the same `.vue` inputs with the same parameters (filename, component ID, is_production, ssr) and produce per-block JS output. **Using the same parameters, they should match.** Differences are Verter bugs.

### Directory Layout

```
crates/verter_core/examples/check/
├── source/                      # .vue source files (numbered 1_ to N_)
├── generated/                   # Compiled output from both compilers
│   ├── {name}.{block}.{mode}.vue.js        # Vue output (SOURCE OF TRUTH)
│   ├── {name}.{block}.{mode}.verter.js     # Verter output (WHAT WE'RE FIXING)
│   ├── {name}.{block}.{mode}.vue.js.match  # Progress markers (JSON)
│   └── {name}.verter.js                    # Verter monolithic output (ignore)
├── summary.verter.json          # Stats including AST comparison results
├── .results/
│   └── differences_compare.md   # Manual logs for “too many differences” files
└── COMPARE_ROUTINE.md           # This file
```

- **block** = `script` | `render` | `style0`, `style1`, ... | custom (e.g. `route`)
- **mode**  = `dev` | `prod` | `ssr`

---

## Key Source Files

| File | Purpose |
|------|---------|
| `crates/verter_core/src/builder/codegen.rs` | `generate_for_vite()`, options, test helpers, and all E2E tests |
| `crates/verter_core/src/codegen/vue/template_plugin.rs` | Template codegen plugin (render output) |
| `crates/verter_core/src/codegen/vue/script_plugin.rs` | Script codegen plugin (script output) |
| `crates/verter_core/src/codegen/vue/style_plugin.rs` | Style codegen plugin |
| `crates/verter_core/src/codegen/vue/template/element.rs` | Element processing, patch flags, dynamic props |
| `crates/verter_core/src/codegen/vue/template/types.rs` | `TemplateCodegenState` + helper flags |
| `crates/verter_core/examples/check.rs` | Generates Vue & Verter outputs + runs AST comparison |
| `crates/verter_core/examples/check.js` | Vue compiler reference (Node) |
| `CLAUDE_IMPLEMENTATION_GUIDE.md` | TDD patterns, conventions |

---

## Verter API for Tests

```rust
use verter_core::builder::codegen::{generate_for_vite, ViteCodegenOptions};

let allocator = oxc_allocator::Allocator::new();
let options = ViteCodegenOptions {
    filename: Some("test.vue".to_string()),
    is_production: false,  // dev mode
    ssr: false,
    component_id: None,
    sourcemap: false,
};
let result = generate_for_vite(source, &options, &allocator);

// result.script   → Option<BlockOutput>
// result.template → Option<BlockOutput>
// result.styles   → Vec<StyleBlock>
// result.custom   → Vec<CustomBlock>
```

### Existing Test Helpers (`codegen.rs` `#[cfg(test)]`)

- `assert_valid_js(code, context)` — validates JS syntax via OXC parser (**MANDATORY**)
- `gen_and_validate(source) -> String` — generates monolithic output + validates JS
- `compare_ast_structure(our_code, vue_code, context) -> Vec<String>` — AST-level comparison
- `assert_no_invalid_patterns(code, context)` — checks known broken patterns
- `INVALID_PATTERNS` — list of known broken codegen indicators

---

## Status Files (.match) and Processing States

Each Vue reference output file may have a sibling `.match` JSON file.

### Completion statuses (considered DONE)
- `auto_match` — created by AST compare in `cargo run --example check`
- `match` — identical or equivalent
- `cosmetic` — formatting-only differences
- `fixed` — difference fixed by a committed change
- `skip` — blocked/unsolved (document reason)

### Non-completion status (considered NOT DONE)
- `defer` — deferred because file has too many differences or is “large”; must be revisited later

> Important: Your “pick next file” logic must treat `defer` as still pending.

Schema (extended):
```json
{
  "status": "auto_match" | "match" | "fixed" | "cosmetic" | "skip" | "defer",
  "date": "YYYY-MM-DD",
  "test": "e2e_test_name",
  "category": "A" | "B" | "C" | "D" | "E",
  "reason": "description",
  "note": "optional"
}
```

---

## Step 0: Baseline + Hygiene (Run Often)

### 0a. Ensure clean working tree
Before starting and before moving to a new file:

```bash
git status --porcelain
```

If not clean and you are not about to commit, rollback:

```bash
git reset --hard HEAD
git clean -fd
```

### 0b. Generate output + auto-compare
```bash
cargo run --example check
```

### 0c. Read summary
```bash
cat summary.verter.json | python3 -m json.tool
```

---

## Step 1: Priority Order (Tiering)

Process in this order (highest value first):

| Priority | Tier | Description |
|---:|---|---|
| 1 | `render.dev` | Dev render functions |
| 2 | `script.dev` | Dev script blocks |
| 3 | `render.ssr` | SSR render functions |
| 4 | `script.prod` | Prod script blocks |
| 5 | `script.ssr` | SSR script blocks |
| 6 | `style*.{mode}` | Styles |
| 7 | `custom` | Custom blocks |

---

## Step 2: Pick Next File (Autopilot-Safe)

### 2a. Tier selection
Pick the highest-priority tier that still has pending work.

### 2b. Choose next file in numeric order (treat `defer` as pending)

Use this helper (Python; no jq dependency):

```bash
python3 - <<'PY'
import glob, json
from pathlib import Path

# Change this glob per tier, e.g. '*.render.dev.vue.js'
pattern = "generated/*.render.dev.vue.js"

def status(match_path: Path):
    if not match_path.exists():
        return None
    try:
        return json.loads(match_path.read_text()).get("status")
    except Exception:
        return "corrupt"

def is_done(st):
    return st in {"auto_match","match","cosmetic","fixed","skip"}

files = sorted(glob.glob(pattern), key=lambda p: int(Path(p).name.split("_",1)[0]))
for f in files:
    st = status(Path(f + ".match"))
    if st == "corrupt":
        print("CORRUPT_MATCH_FILE:", f + ".match")
        continue
    if st is None or (not is_done(st)) or st == "defer":
        print(f)
        break
PY
```

(Repeat with the correct pattern for the current tier.)

---

## Step 3: Compare (Per File)

For `{name}.{block}.{mode}.vue.js`:

### 3a. Read both outputs
- Vue: `generated/{name}.{block}.{mode}.vue.js`
- Verter: `generated/{name}.{block}.{mode}.verter.js`

### 3b. Edge cases
- Verter file missing → write `.match` as `skip` with reason `verter_missing`
- Vue file empty/errored → `.match` as `skip` with reason `vue_empty_or_error`
- Files identical → `.match` as `match` and continue

### 3c. Classify differences (all differences are bugs)
| Category | Meaning | Typical action |
|---|---|---|
| **A** | Invalid JS (does not parse) | Fix immediately |
| **B** | Missing feature | Often “large” — may defer unless tiny |
| **C** | Wrong structure | TDD fix (often medium) |
| **D** | Wrong values | TDD fix (small) |
| **E** | Cosmetic only | Mark `cosmetic` (done) |

### 3d. Too many differences rule (defer + log)
If the file shows **multiple distinct root causes**, or any of:
- > 10 AST structural diff lines from `compare_ast_structure(...)`
- mixes Categories (e.g., C + D + B together)
- requires implementing a known batch feature (caching / static vnode collapsing / binding prefix policy)

…then:

1. Append a section to `.results/differences_compare.md` (see template below).
2. Create `.match` with `{"status":"defer", ...}`.
3. Move on to the next file.

> “Defer” is not “skip”. Deferred items must be revisited in a later pass.

Template for `.results/differences_compare.md` entry:
```md
## {name}.{block}.{mode}

Date: YYYY-MM-DD

### Summary
- Category guess: B/C/D
- Why deferred: (e.g. multiple root causes, too many diffs)
- Suspected modules: (file paths)

### Vue output snippet
```js
// ...
```

### Verter output snippet
```js
// ...
```

### Observed diffs
- ...
- ...
```

---

## Step 4: TDD Fix Cycle (Per Issue)

### 4a. Minimize reproduction
Read `source/{name}.vue` and reduce to the smallest `.vue` source that reproduces the specific difference.

### 4b. Add a NEW failing test (mandatory)
Add a test to `crates/verter_core/src/builder/codegen.rs` (`#[cfg(test)]` module). Do not edit existing tests.

Render example:
```rust
#[test]
fn e2e_{feature}_{detail}_render_matches_vue() {
    let vue_source = r#"<template>...</template><script setup>...</script>"#;

    let vue_render = r#"import { ... } from "vue"

export function render(_ctx, _cache) {
  // ...
}"#;

    let allocator = oxc_allocator::Allocator::new();
    let options = ViteCodegenOptions { filename: Some("test.vue".into()), ..Default::default() };
    let result = generate_for_vite(vue_source, &options, &allocator);
    let our_render = result.template.expect("should have template block").code;

    assert_valid_js(&our_render, "verter render output");

    let diffs = compare_ast_structure(&our_render, vue_render, "render_block");
    assert!(
        diffs.is_empty(),
        "Render output differs from Vue:\n{}\n\nVerter:\n{}\n\nVue:\n{}",
        diffs.join("\n"),
        our_render,
        vue_render
    );
}
```

Script example:
```rust
#[test]
fn e2e_{feature}_{detail}_script_matches_vue() {
    let vue_source = r#"..."#;
    let vue_script = r#"..."#;

    let allocator = oxc_allocator::Allocator::new();
    let options = ViteCodegenOptions { filename: Some("test.vue".into()), ..Default::default() };
    let result = generate_for_vite(vue_source, &options, &allocator);
    let our_script = result.script.expect("should have script block").code;

    assert_valid_js(&our_script, "verter script output");

    let diffs = compare_ast_structure(&our_script, vue_script, "script_block");
    assert!(
        diffs.is_empty(),
        "Script output differs from Vue:\n{}\n\nVerter:\n{}\n\nVue:\n{}",
        diffs.join("\n"),
        our_script,
        vue_script
    );
}
```

### 4c. Verify the test fails
```bash
cargo test -p verter_core e2e_{name} 2>&1 | tail -50
```

### 4d. Implement the smallest fix
Identify likely module:

| Symptom | Likely location |
|---|---|
| Custom blocks in script | `src/codegen/vue/script_plugin.rs` |
| Render function signature | `src/codegen/vue/template_plugin.rs` |
| Wrong element output | `src/codegen/vue/template/element.rs` |
| Missing helper imports | `src/codegen/vue/template/types.rs` |
| Patch flags wrong | `src/codegen/vue/template/element.rs` |
| Binding prefix wrong | `src/codegen/vue/template/*` |
| Missing caching | `template/element.rs` or `template_plugin.rs` |
| Script wrapping wrong | `script_plugin.rs` |
| Style scoping wrong | `style_plugin.rs` |

### 4e. Run tests (mandatory gating)
1) Targeted test:
```bash
cargo test -p verter_core e2e_{name}
```

2) **Full tests** (required):
```bash
cargo test
```

### 4f. Commit or rollback

#### If `cargo test` passes
1. Regenerate output + auto-compare:
   ```bash
   cargo run --example check
   ```
2. Create/update `.match` for the file (if not auto-matched):
   ```json
   {"status":"fixed","date":"YYYY-MM-DD","test":"e2e_...","category":"C"}
   ```
3. Commit:
   ```bash
   git add -A
   git commit -m "fix(check): {tier} {name} {category} (e2e_...)"
   ```

#### If `cargo test` fails
- Attempt to fix quickly (still minimal changes).
- If you cannot restore green quickly:
  ```bash
  git reset --hard HEAD
  git clean -fd
  ```
- Mark the file:
  - If blocked/unsolved → `skip`
  - If too large → `defer` + log to `.results/differences_compare.md`
- Move to the next file.

---

## Known High-Impact Root Causes (Batch Later)

These may affect many files; prefer to fix small issues first, but log occurrences.

1. Custom blocks embedded into script output (invalid JS)
2. Render function signature mismatch
3. Cache patterns `_cache[n] || (_cache[n] = ...)`
4. `_createStaticVNode(...)` collapsing
5. Binding prefix policy (`_ctx.foo` vs `$setup[...]` / `$props...`)
6. Import position (cosmetic; generally auto-matchable)

---

## Completion Condition: “All files processed”

You are done only when, for every tier, every Vue output file has a `.match` whose status is one of:
- `auto_match`, `match`, `cosmetic`, `fixed`, `skip`

If any `defer` remain, start a **deferred pass**:
- treat `defer` as pending
- select deferred files first (or lowest-numbered deferred in each tier)
- repeat the same TDD/commit loop until no `defer` remain

---

## Progress Report (End of Session or Milestone)

Run:
```bash
cargo run --example check
cat summary.verter.json | python3 -m json.tool
```

Append to `.results/differences_compare.md` (or a separate log) a summary:

```md
# Progress Report — YYYY-MM-DD

## Summary.verter.json
- Total pairs: ...
- Matched: ...
- Previously matched: ...
- Mismatched: ...
- Parse errors: vue ... / verter ...

## Tier breakdown
- render.dev: ...
- script.dev: ...
- render.ssr: ...
- script.prod: ...
- script.ssr: ...
- styles: ...
- custom: ...

## Commits this session
- (list commit messages)

## Deferred items remaining
- (list file basenames)
```

---

## Quick Commands

```bash
# regenerate + auto-compare
cargo run --example check

# full tests (mandatory gate)
cargo test

# run a specific E2E test
cargo test -p verter_core e2e_test_name

# quick diff for one file
diff generated/{name}.render.dev.vue.js generated/{name}.render.dev.verter.js

# show pending files in a tier (Python, defer-aware)
python3 - <<'PY'
import glob, json
from pathlib import Path

pattern = "generated/*.render.dev.vue.js"
done = {"auto_match","match","cosmetic","fixed","skip"}

def st(p):
    mp = Path(p + ".match")
    if not mp.exists(): return None
    try: return json.loads(mp.read_text()).get("status")
    except Exception: return "corrupt"

files = sorted(glob.glob(pattern), key=lambda p: int(Path(p).name.split("_",1)[0]))
pending = [p for p in files if st(p) not in done]
print("\n".join(pending[:50]))
print("PENDING:", len(pending))
PY
```
