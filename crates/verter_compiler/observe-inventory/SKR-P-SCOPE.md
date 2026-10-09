# Template lexical-scope inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_compiler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `OxcParsedAst::scopes` (`LexicalScopes`): append-only frames, each holding only the `v-for` aliases or `v-slot` parameters one element declares, linked to its enclosing frame | Template binding resolution (VDOM, Vapor, SSR, IDE), slot `hasScopeRef` decision, IDE broken-interpolation recovery | REQUIRED | Owned by the `OxcParsedAst` of one template parse; dropped with it | `src/template/oxc/scope.rs` | always |
| `OxcParsedAst::children_scopes`: NodeId-aligned `LexicalScopeId` handle each node's children see | Scope lookup for any node without an ancestor walk (`children_scope`, `scope_of`, slot `hasScopeRef`) | REQUIRED | Same as the owning `OxcParsedAst` | `src/template/oxc/types.rs` | always |
| `OxcParsedElement::props_scope`: scope of an element's props, dynamic slot name and `v-slot` value | Slot `hasScopeRef` outer-scope boundary | REQUIRED | Same as the owning `OxcParsedAst` | `src/template/oxc/types.rs` | always |
| `OxcParsedExpression::ide_recovery_scope`: scope handle kept for an IDE expression that did not parse | IDE broken-interpolation recovery (scoped locals stay bare) | REQUIRED | Same as the owning `OxcParsedAst` | `src/template/oxc/types.rs` | always |
| `ActiveScope`: multiset of names visible at the node the forward pass is parsing | Template-scope name lookups during binding extraction | REQUIRED | One `parse_template_expressions` call; dropped when it returns | `src/template/oxc/scope.rs` | always |
| `SCOPE_WORK` thread-local counter, `take_scope_work` reader and `record_scope_work` hook: frames entered/left plus names added/removed while switching scopes | none; tests and measurement builds only | OPTIONAL | Per thread; the reader resets it | `src/template/oxc/scope.rs` | `cfg(any(test, feature = "semantic-observe"))` |

## verter_parser

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `BindingContext::enclosing`: shared handle to the enclosing template-scope names | `should_ignore` (template-local identifiers stay unprefixed) | REQUIRED | One binding extraction; child contexts share the handle | `src/utils/oxc/bindings/types.rs` | always |

Default builds compile `record_scope_work` to an empty inline function; there is
no per-operation enabled check.
