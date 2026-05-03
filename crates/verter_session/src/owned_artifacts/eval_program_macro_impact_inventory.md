# Eval-program macro-impact inventory

**Plan reference:** Tier 0 Step 0.0 (D116) of the legacy → graph + dispatch migration.

**Built from real codebase baseline**: this inventory is grounded in inspection of
`crates/verter_parser/src/utils/oxc/vue/script/{bindings,setup,macros,resolve_type}.rs`
and `crates/verter_session/src/host_manage/eval_env.rs` at the integration-trunk
HEAD. Patterns currently passing in the resolver are in the **Supported** list;
the **FAIL** list is restricted to constructs the resolver also currently rejects.

**Tier 1A use:** the `OwnedEvalProgram::LoweredStmt::Unsupported` and
`LoweredExpr::Unsupported` variants are emitted only for kinds in the
"Diagnostic-only on Unsupported" rows below. Kinds in the
"FAIL on Unsupported" rows produce `LoweringError::*` per D117 instead of a
silent skip.

## Categorization rules

- **Supported** — the resolver explicitly handles this kind and produces a useful
  binding/type/diagnostic. Patterns currently passing in production must remain
  here post-Tier-1A.
- **Diagnostic-only** — the resolver currently silently skips this kind. Tier 1A
  preserves silent-skip; lowering emits `LoweredStmt::Unsupported { kind, span }`
  but does not abort the program. Used for non-macro-impacting constructs at the
  module top level (e.g., `if` / `for` / `try` outside `<script setup>` macros).
- **FAIL on Unsupported** — the resolver currently silently skips OR rejects this
  kind in a macro-impacting position. Tier 1A converts the silent skip to
  `LoweringError::UnsupportedMacroArgumentShape { macro_name, span, kind }` so
  callers see a typed error instead of an empty result.

## Top-level statements

Source: `crates/verter_parser/src/utils/oxc/vue/script/bindings.rs::classify_statement`
(lines 41-76 at validation SHA).

| OXC AST kind | Current behavior | Macro-impact |
|---|---|---|
| `Statement::VariableDeclaration` | Supported (binding extraction; const/let/var classified) | n/a |
| `Statement::ImportDeclaration` | Supported (binding extraction; named/default/namespace) | n/a |
| `Statement::ExpressionStatement` | Supported (top-level macro calls live here) | n/a |
| `Statement::FunctionDeclaration` | Supported (`SetupConst` binding) | n/a |
| `Statement::ClassDeclaration` | Supported (`SetupConst` binding) | n/a |
| `Statement::TSEnumDeclaration` | Supported (`SetupConst` binding; runtime JS form) | n/a |
| `Statement::TSTypeAliasDeclaration` | Supported (no runtime binding; type-only) | n/a |
| `Statement::TSInterfaceDeclaration` | Supported (no runtime binding; type-only) | n/a |
| `Statement::IfStatement` | Diagnostic-only | non-macro-impacting at module top |
| `Statement::ForStatement` / `ForInStatement` / `ForOfStatement` | Diagnostic-only | non-macro-impacting at module top |
| `Statement::WhileStatement` / `DoWhileStatement` | Diagnostic-only | non-macro-impacting at module top |
| `Statement::TryStatement` | Diagnostic-only | non-macro-impacting at module top |
| `Statement::ThrowStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::SwitchStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::LabeledStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::BlockStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::DebuggerStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::ReturnStatement` | Diagnostic-only | non-macro-impacting at module top |
| `Statement::BreakStatement` / `ContinueStatement` | Diagnostic-only | non-macro-impacting |
| `Statement::EmptyStatement` | Diagnostic-only | benign |

## Macro-call argument expressions

Source: `crates/verter_parser/src/utils/oxc/vue/script/setup.rs::extract_object_arg_from_call`,
`extract_array_arg_from_call`, `extract_prop_type_annotation`, `extract_runtime_types_from_expr`.

Macro-impacting positions: argument 0/1 of `defineProps`, `defineEmits`, `defineModel`,
`defineSlots`, `withDefaults`, and the type-arguments slot of any of the above.

| OXC AST kind | Current behavior | Macro-impact |
|---|---|---|
| `Expression::ObjectExpression` | Supported (extract via `extract_object_arg`) | FAIL on Unsupported nested kinds |
| `Expression::ArrayExpression` | Supported (extract via `extract_array_arg`) | FAIL on Unsupported nested kinds |
| `Expression::TSAsExpression` (with `as PropType<T>`) | Supported (extract via `extract_prop_type_annotation`) | FAIL on Unsupported inner shape |
| `Expression::Identifier` (callee position) | Supported | n/a |
| `Expression::TemplateLiteral` (in macro property values, e.g., `withDefaults` runtime defaults) | Supported (treated as runtime expression; passed through) | n/a |
| `Expression::StringLiteral` (in macro property keys/values) | Supported | n/a |
| `Expression::NumericLiteral` (in macro property values) | Supported | n/a |
| `Expression::BooleanLiteral` (in macro property values) | Supported | n/a |
| `Expression::NullLiteral` (in macro property values) | Supported | n/a |
| `Expression::ArrowFunctionExpression` (in macro property values, e.g., default factories) | Supported (treated as runtime expression) | n/a |
| `Expression::FunctionExpression` (in macro property values) | Supported (treated as runtime expression) | n/a |
| `Expression::CallExpression` (in macro property values) | Supported (treated as runtime expression) | n/a |
| `Expression::SpreadElement` in `ObjectExpression` properties | Currently silent-skip | **FAIL on Unsupported** (semantic loss in macro shape) |
| `Expression::ComputedMemberExpression` as property key | Currently silent-skip via `extract_property_key` `_ => None` | **FAIL on Unsupported** (semantic loss) |
| `Expression::ConditionalExpression` (ternary) at macro arg root | Currently silent-skip | **FAIL on Unsupported** (resolver cannot determine shape) |
| `Expression::SequenceExpression` (comma) at macro arg root | Currently silent-skip | **FAIL on Unsupported** |
| `Expression::AwaitExpression` at macro arg root | Currently silent-skip | **FAIL on Unsupported** (`<script setup>` async shapes) |
| `Expression::YieldExpression` at macro arg root | Currently silent-skip | **FAIL on Unsupported** |
| `Expression::ParenthesizedExpression` | Supported (transparent) | n/a |

## Property keys (in macro `ObjectExpression` arguments)

Source: `crates/verter_parser/src/utils/oxc/vue/script/setup.rs::extract_property_key`
(lines 1093-1108 at validation SHA).

| OXC AST kind | Current behavior | Macro-impact |
|---|---|---|
| `PropertyKey::StaticIdentifier` | Supported | n/a |
| `PropertyKey::StringLiteral` | Supported | n/a |
| `PropertyKey::NumericLiteral` | Currently silent-skip (returns `None`; comment: "rare in Vue macros") | Diagnostic-only (genuinely rare; non-impacting in known fixtures) |
| `PropertyKey::PrivateIdentifier` | Currently silent-skip | Diagnostic-only (invalid in macro shape; never a Vue prop key) |
| `PropertyKey::TemplateLiteral` | Currently silent-skip | **FAIL on Unsupported** (could mask intent) |
| `PropertyKey::Computed*` | Currently silent-skip | **FAIL on Unsupported** (semantic loss) |

## Type-position arguments

Source: `crates/verter_parser/src/utils/oxc/vue/script/resolve_type.rs` (5597 LOC; comprehensive
TS type traversal). Inventory here only highlights kinds whose handling is macro-relevant.

| OXC AST kind | Current behavior | Macro-impact |
|---|---|---|
| `TSType::TSTypeReference` | Supported (full resolver) | n/a |
| `TSType::TSTypeLiteral` (inline object type) | Supported | n/a |
| `TSType::TSIntersectionType` | Supported (recursive arms) | n/a |
| `TSType::TSUnionType` | Supported (recursive arms) | n/a |
| `TSType::TSConditionalType` | Supported (per `/type-resolution` skill) | n/a |
| `TSType::TSMappedType` | Supported (per resolver) | n/a |
| `TSType::TSIndexedAccessType` | Supported (per resolver) | n/a |
| `TSType::TSTupleType` | Supported | n/a |
| `TSType::TSArrayType` | Supported | n/a |
| `TSType::TSImportType` | Supported (cross-file) | n/a |
| `TSType::TSLiteralType` (string/number/boolean) | Supported | n/a |
| `TSType::TSFunctionType` | Supported (used in event/slot types) | n/a |
| `TSType::TSConstructorType` | Currently silent-skip in macro positions | **FAIL on Unsupported** in `defineEmits<T>` / `defineSlots<T>` arg |
| `TSType::TSInferType` outside `TSConditionalType.extendsType` | Currently silent-skip | **FAIL on Unsupported** |
| `TSType::TSTypeOperator` (`keyof`/`readonly`/`unique`) | Supported (`keyof`/`readonly`); `unique` silently skipped | Diagnostic-only on `unique` (rare) |
| `TSType::TSTypePredicate` | Currently silent-skip | Diagnostic-only (not meaningful in macro pos) |

## Top-level imports

Source: `crates/verter_parser/src/utils/oxc/vue/script/bindings.rs::classify_import`.

| OXC AST kind | Current behavior | Macro-impact |
|---|---|---|
| `ImportDeclaration` named import | Supported | n/a |
| `ImportDeclaration` default import | Supported | n/a |
| `ImportDeclaration` namespace import (`import * as X`) | Supported | n/a |
| `ImportDeclaration` side-effect-only (`import "x"`) | Supported (no binding) | n/a |
| `ImportDeclaration` type-only (`import type { X }`) | Supported (type binding) | n/a |
| `ImportDeclaration` mixed value+type | Supported | n/a |

No FAIL rows: every Vue/JS/TS import shape is recognized.

## Macro-detection (`detect_macro_kind`)

Source: `crates/verter_parser/src/utils/oxc/vue/script/macros.rs::detect_macro_kind`.

| Macro callee name | Supported `VueMacroKind` |
|---|---|
| `defineProps` | `DefineProps` |
| `defineEmits` | `DefineEmits` |
| `defineModel` | `DefineModel` |
| `defineSlots` | `DefineSlots` |
| `defineExpose` | `DefineExpose` |
| `defineOptions` | `DefineOptions` |
| `withDefaults` | `WithDefaults` |
| `defineComponent` | `DefineComponent` (script-only path) |
| Other identifiers | Not a macro (passed through to runtime) |

## Tier 1A discriminating-test bridge

The Tier 1A architecture guard `macro_impacting_constructs_fail_lowering_not_silent_skip`
(D107) loads this inventory and asserts:

1. Every row marked **FAIL on Unsupported** above corresponds to a
   `LoweringError::*` variant emitted by `OwnedEvalProgram` lowering.
2. Every row marked **Diagnostic-only** corresponds to a `LoweredStmt::Unsupported`
   or `LoweredExpr::Unsupported` instance with no `LoweringError`.
3. Every row marked **Supported** has at least one fixture in
   `crates/verter_session/tests/fixtures/macro_impact/` that resolves successfully
   post-Tier-1A.

## Source provenance (D116)

This inventory was authored at validation SHA `60b1295a` from direct reading of:

- `crates/verter_parser/src/utils/oxc/vue/script/bindings.rs:41-197` — `classify_statement`,
  `classify_call_expression`.
- `crates/verter_parser/src/utils/oxc/vue/script/setup.rs:880-1108` —
  `extract_object_arg_from_call`, `extract_property_key`, `extract_prop_type_annotation`.
- `crates/verter_parser/src/utils/oxc/vue/script/macros.rs` — `detect_macro_kind`,
  `ScriptMacro` enum, `MacroObjectArg` / `MacroProperty` shapes.

No new behaviors were inferred. New rows must cite a specific function and line
range in the production parser.
